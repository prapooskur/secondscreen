use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{
    StatusCode,
    blocking::{Client, RequestBuilder, Response},
    redirect::Policy,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::time::Duration;
use url::Url;

use crate::config::HomeAssistantCredentials;

pub(super) struct Connection {
    client: Client,
    base: Url,
    token: String,
}

impl Connection {
    pub fn new(credentials: &HomeAssistantCredentials) -> Result<Self> {
        let credentials =
            HomeAssistantCredentials::from_input(&credentials.url, &credentials.token)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(6))
            .redirect(Policy::none())
            .build()
            .context("Could not initialize the Home Assistant connection.")?;
        Ok(Self {
            client,
            base: Url::parse(&credentials.url)?,
            token: credentials.token,
        })
    }

    pub fn entities(&self) -> Result<Vec<Device>> {
        let response = self.send(self.client.get(self.endpoint(&["states"])))?;
        let mut devices: Vec<Device> = response.json().map_err(request_error)?;
        devices.retain(Device::supported);
        devices.sort_by_cached_key(|device| (device.name().to_lowercase(), device.id.clone()));
        Ok(devices)
    }

    pub fn entity(&self, id: &str) -> Result<Device> {
        let device: Device = self
            .send(self.client.get(self.endpoint(&["states", id])))?
            .json()
            .map_err(request_error)?;
        ensure!(
            device.id == id,
            "Home Assistant returned a different entity."
        );
        Ok(device)
    }

    pub fn act(&self, id: &str, action: Action) -> Result<()> {
        let (domain, _) = id.split_once('.').context("Invalid entity ID.")?;
        let mut data = Map::from_iter([("entity_id".into(), Value::String(id.to_owned()))]);
        if let Some((key, value)) = action.field {
            data.insert(key.into(), value);
        }
        let url = self.endpoint(&["services", domain, action.service]);
        self.send(self.client.post(url).json(&data))?;
        Ok(())
    }

    fn endpoint(&self, path: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("HTTP URLs have path segments")
            .pop_if_empty()
            .push("api")
            .extend(path);
        url
    }

    fn send(&self, request: RequestBuilder) -> Result<Response> {
        let response = request
            .bearer_auth(&self.token)
            .send()
            .map_err(request_error)?;
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::UNAUTHORIZED => bail!("Home Assistant rejected the access token."),
            StatusCode::FORBIDDEN => bail!("Home Assistant denied access."),
            status if status.is_redirection() => {
                bail!("Home Assistant redirected the request. Check the base URL.")
            }
            status => bail!("Home Assistant returned HTTP {}.", status.as_u16()),
        }
    }
}

fn request_error(error: reqwest::Error) -> anyhow::Error {
    if error.is_timeout() {
        anyhow!("Home Assistant did not respond in time.")
    } else if error.is_decode() {
        anyhow!("Home Assistant returned an invalid response.")
    } else {
        anyhow!("Could not reach Home Assistant. Check the server address and network.")
    }
}

#[derive(Clone, Deserialize, PartialEq)]
pub(super) struct Device {
    #[serde(rename = "entity_id")]
    pub id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: Map<String, Value>,
}

impl Device {
    pub fn domain(&self) -> &str {
        self.id.split_once('.').map_or("", |(domain, _)| domain)
    }

    pub fn name(&self) -> &str {
        self.attributes
            .get("friendly_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.id)
    }

    pub fn available(&self) -> bool {
        self.state != "unavailable"
    }

    pub fn supports(&self, feature: u64) -> bool {
        self.attributes
            .get("supported_features")
            .and_then(Value::as_u64)
            .is_some_and(|features| features & feature != 0)
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.attributes.get(key).and_then(Value::as_f64)
    }
}

#[derive(Clone)]
pub(super) struct Action {
    pub label: String,
    pub service: &'static str,
    pub field: Option<(&'static str, Value)>,
}

impl Action {
    pub fn new(label: impl Into<String>, service: &'static str) -> Self {
        Self {
            label: label.into(),
            service,
            field: None,
        }
    }

    pub fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.field = Some((key, value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn sends_authenticated_service_request_for_the_selected_entity() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/home/", listener.local_addr().unwrap());
        let credentials = HomeAssistantCredentials::from_input(&base, "test-token").unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(
                line.trim(),
                "POST /home/api/services/light/turn_on HTTP/1.1"
            );
            let mut length = None;
            let mut authorized = false;
            loop {
                line.clear();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "authorization" => authorized = value.trim() == "Bearer test-token",
                    "content-length" => length = Some(value.trim().parse::<usize>().unwrap()),
                    _ => {}
                }
            }
            assert!(authorized);
            let mut body = vec![0; length.unwrap()];
            reader.read_exact(&mut body).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                serde_json::json!({"entity_id": "light.desk", "brightness": 128}),
            );
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]")
                .unwrap();
        });
        Connection::new(&credentials)
            .unwrap()
            .act(
                "light.desk",
                Action::new("Dim", "turn_on").with("brightness", 128),
            )
            .unwrap();
        server.join().unwrap();
    }
}
