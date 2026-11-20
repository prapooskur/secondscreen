use anyhow::{Context, Result, bail, ensure};
use async_channel::Sender;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::Integration;
use crate::model::{DiscordAction, DiscordState, Update, VoiceChannel, VoiceMember, VoiceState};

const RPC_TIMEOUT: Duration = Duration::from_secs(2);
const AUTH_TIMEOUT: Duration = Duration::from_secs(90);
const SCOPES: &[&str] = &["identify", "rpc", "rpc.voice.read", "rpc.voice.write"];
const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";

struct Credentials {
    client_id: String,
    secret: Option<String>,
    access_token: Option<String>,
}

pub(super) struct Discord {
    credentials: Option<Credentials>,
    session: Option<Session>,
    error: Option<String>,
}

struct Session {
    rpc: Rpc,
    account: String,
}

impl Discord {
    pub fn new() -> Self {
        let credentials = setting("SECONDSCREEN_DISCORD_CLIENT_ID").and_then(|client_id| {
            let secret = setting("SECONDSCREEN_DISCORD_CLIENT_SECRET");
            let access_token = setting("SECONDSCREEN_DISCORD_ACCESS_TOKEN");
            if !client_id.bytes().all(|byte| byte.is_ascii_digit())
                || (secret.is_none() && access_token.is_none())
            {
                return None;
            }
            Some(Credentials {
                client_id,
                secret,
                access_token,
            })
        });
        Self {
            credentials,
            session: None,
            error: None,
        }
    }

    fn connect(&mut self) -> Result<Session> {
        let credentials = self
            .credentials
            .as_mut()
            .context("Configure Discord RPC before connecting.")?;
        let rpc = Rpc::connect(&credentials.client_id)?;
        credentials.authenticate_session(rpc, |id, secret, code| {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            exchange_code(&client, TOKEN_URL, id, secret, code)
        })
    }
}

impl Credentials {
    fn authenticate_session(
        &mut self,
        mut rpc: Rpc,
        exchange: impl FnOnce(&str, &str, &str) -> Result<String>,
    ) -> Result<Session> {
        if let Some(token) = &self.access_token {
            match authenticate(&mut rpc, token) {
                Ok(account) => return Ok(Session { rpc, account }),
                Err(error) if self.secret.is_none() => return Err(error),
                Err(_) => self.access_token = None,
            }
        }

        let secret = self.secret.as_deref().context(
            "A Discord application secret is required to exchange the authorization code.",
        )?;
        let authorized = rpc.call(
            "AUTHORIZE",
            json!({
                "client_id": self.client_id,
                "scopes": SCOPES,
            }),
            AUTH_TIMEOUT,
        )?;
        let code = authorized["code"]
            .as_str()
            .context("Discord did not return an authorization code.")?;
        let token = exchange(&self.client_id, secret, code)?;
        let account = authenticate(&mut rpc, &token)?;
        self.access_token = Some(token);
        Ok(Session { rpc, account })
    }
}

fn setting(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn exchange_code(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    id: &str,
    secret: &str,
    code: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct Token {
        access_token: String,
    }

    let response = client
        .post(endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", id),
            ("client_secret", secret),
            ("code", code),
        ])
        .send()
        .context("Could not reach Discord's OAuth2 endpoint.")?;
    ensure!(
        response.status().is_success(),
        "Discord rejected the OAuth2 code exchange ({}).",
        response.status()
    );
    Ok(response
        .json::<Token>()
        .context("Discord returned an invalid OAuth2 response.")?
        .access_token)
}

fn authenticate(rpc: &mut Rpc, token: &str) -> Result<String> {
    let result = rpc.call(
        "AUTHENTICATE",
        json!({ "access_token": token }),
        RPC_TIMEOUT,
    )?;
    Ok(result["user"]["global_name"]
        .as_str()
        .or_else(|| result["user"]["username"].as_str())
        .unwrap_or("Discord account")
        .to_owned())
}

impl Session {
    fn voice_settings(&mut self) -> Result<(bool, bool)> {
        let data = self
            .rpc
            .call("GET_VOICE_SETTINGS", json!({}), RPC_TIMEOUT)?;
        let mute = data["mute"]
            .as_bool()
            .context("Discord did not report microphone mute.")?;
        let deaf = data["deaf"]
            .as_bool()
            .context("Discord did not report deafen state.")?;
        Ok((mute, deaf))
    }

    fn poll(&mut self) -> Result<VoiceState> {
        let (muted, deafened) = self.voice_settings()?;
        let channel = self
            .rpc
            .call("GET_SELECTED_VOICE_CHANNEL", json!({}), RPC_TIMEOUT)?;
        let channel = channel["id"].as_str().map(|_| {
            let members = channel["voice_states"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|member| VoiceMember {
                    name: member["nick"]
                        .as_str()
                        .or_else(|| member["user"]["global_name"].as_str())
                        .or_else(|| member["user"]["username"].as_str())
                        .unwrap_or("Discord user")
                        .to_owned(),
                    muted: member["voice_state"]["self_mute"]
                        .as_bool()
                        .unwrap_or(false)
                        || member["voice_state"]["mute"].as_bool().unwrap_or(false),
                    deafened: member["voice_state"]["self_deaf"]
                        .as_bool()
                        .unwrap_or(false)
                        || member["voice_state"]["deaf"].as_bool().unwrap_or(false),
                })
                .collect();
            VoiceChannel {
                name: channel["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Voice call")
                    .to_owned(),
                members,
            }
        });
        Ok(VoiceState {
            account: self.account.clone(),
            muted,
            deafened,
            channel,
        })
    }
}

impl Integration for Discord {
    type Action = DiscordAction;

    fn poll(&mut self) -> Update {
        let state = match self.session.as_mut().map(Session::poll) {
            Some(Ok(voice)) => DiscordState::Connected(voice),
            Some(Err(error)) => {
                self.session = None;
                self.error = Some(error.to_string());
                DiscordState::Error(error.to_string())
            }
            None if self.credentials.is_none() => DiscordState::Unconfigured,
            None => self
                .error
                .clone()
                .map_or(DiscordState::Disconnected, DiscordState::Error),
        };
        Update::Discord(state)
    }

    fn handle(&mut self, action: DiscordAction, updates: &Sender<Update>) -> Result<()> {
        match action {
            DiscordAction::Connect => {
                if self.session.is_none() {
                    let _ = updates.send_blocking(Update::Discord(DiscordState::Authorizing));
                    self.error = None;
                    match self.connect() {
                        Ok(session) => self.session = Some(session),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
            }
            DiscordAction::Disconnect => {
                self.session = None;
                self.error = None;
            }
            DiscordAction::ToggleMute | DiscordAction::ToggleDeafen | DiscordAction::Leave => {
                let session = self
                    .session
                    .as_mut()
                    .context("Connect to Discord before using voice controls.")?;
                if matches!(action, DiscordAction::Leave) {
                    session.rpc.call(
                        "SELECT_VOICE_CHANNEL",
                        json!({ "channel_id": null }),
                        RPC_TIMEOUT,
                    )?;
                } else {
                    let (muted, deafened) = session.voice_settings()?;
                    let args = if matches!(action, DiscordAction::ToggleMute) {
                        json!({ "mute": !muted })
                    } else {
                        json!({ "deaf": !deafened })
                    };
                    session.rpc.call("SET_VOICE_SETTINGS", args, RPC_TIMEOUT)?;
                }
            }
        }
        Ok(())
    }
}

struct Rpc {
    socket: UnixStream,
    nonce: u64,
}

impl Rpc {
    fn connect(client_id: &str) -> Result<Self> {
        let mut directories: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
            .into_iter()
            .filter_map(env::var_os)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
            for client in [
                "com.discordapp.Discord",
                "com.discordapp.DiscordCanary",
                "com.discordapp.DiscordPTB",
                "dev.vencord.Vesktop",
            ] {
                directories.push(PathBuf::from(&runtime).join("app").join(client));
            }
            directories.push(PathBuf::from(runtime).join("snap.discord"));
        }
        directories.push(PathBuf::from("/tmp"));

        let socket = directories
            .iter()
            .flat_map(|path| (0..10).map(move |n| path.join(format!("discord-ipc-{n}"))))
            .find_map(|path| UnixStream::connect(path).ok())
            .context(
                "Discord desktop is not listening for local RPC. Open Discord and try again.",
            )?;
        socket.set_write_timeout(Some(RPC_TIMEOUT))?;
        let mut rpc = Self { socket, nonce: 0 };
        rpc.send(
            0,
            &serde_json::to_vec(&json!({ "v": 1, "client_id": client_id }))?,
        )?;
        let ready = rpc.receive(Instant::now() + RPC_TIMEOUT)?;
        ensure!(
            ready["evt"] == "READY",
            "Discord did not accept the RPC handshake."
        );
        Ok(rpc)
    }

    fn call(&mut self, command: &str, args: Value, timeout: Duration) -> Result<Value> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let payload = serde_json::to_vec(&json!({ "cmd": command, "args": args, "nonce": nonce }))?;
        self.send(1, &payload)?;
        let deadline = Instant::now() + timeout;
        loop {
            let response = self.receive(deadline)?;
            if response["nonce"].as_str() != Some(&nonce) {
                continue;
            }
            if response["evt"] == "ERROR" {
                let message = response["data"]["message"]
                    .as_str()
                    .unwrap_or("The command failed.");
                bail!("Discord RPC: {message}");
            }
            ensure!(
                response["cmd"] == command,
                "Discord returned an unexpected RPC reply."
            );
            return Ok(response["data"].clone());
        }
    }

    fn send(&mut self, opcode: u32, body: &[u8]) -> Result<()> {
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(&opcode.to_le_bytes());
        header[4..].copy_from_slice(&u32::try_from(body.len())?.to_le_bytes());
        self.socket
            .write_all(&header)
            .context("Could not write to Discord desktop.")?;
        self.socket
            .write_all(body)
            .context("Could not write to Discord desktop.")?;
        Ok(())
    }

    fn receive(&mut self, deadline: Instant) -> Result<Value> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "Discord RPC timed out.");
            self.socket.set_read_timeout(Some(remaining))?;
            let mut header = [0u8; 8];
            self.socket
                .read_exact(&mut header)
                .context("Discord desktop did not send a reply.")?;
            let opcode = u32::from_le_bytes(header[..4].try_into()?);
            let size = u32::from_le_bytes(header[4..].try_into()?) as usize;
            ensure!(
                size <= 8 * 1024 * 1024,
                "Discord sent an oversized RPC frame."
            );
            let mut payload = vec![0; size];
            self.socket
                .read_exact(&mut payload)
                .context("Discord desktop closed an RPC frame early.")?;
            match opcode {
                1 => {
                    return serde_json::from_slice(&payload)
                        .context("Discord sent invalid RPC data.");
                }
                2 => {
                    let message = serde_json::from_slice::<Value>(&payload)
                        .ok()
                        .and_then(|value| value["message"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| "Discord closed its RPC connection.".into());
                    bail!("{message}");
                }
                3 => self.send(4, &payload)?,
                4 => {}
                _ => bail!("Discord sent an unknown RPC frame."),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::{BufRead, BufReader},
        net::TcpListener,
        thread,
    };

    fn assert_rpc(rpc: &mut Rpc, command: &str, args: Value) -> Value {
        let request = rpc.receive(Instant::now() + RPC_TIMEOUT).unwrap();
        assert_eq!(request["cmd"], command);
        assert_eq!(request["args"], args);
        request
    }

    fn send(rpc: &mut Rpc, value: Value) {
        rpc.send(1, &serde_json::to_vec(&value).unwrap()).unwrap();
    }

    #[test]
    fn rejected_token_reauthorizes_and_caches_the_new_token() {
        let (client, server) = UnixStream::pair().unwrap();
        let ipc = thread::spawn(move || {
            let mut rpc = Rpc {
                socket: server,
                nonce: 0,
            };
            let request = assert_rpc(
                &mut rpc,
                "AUTHENTICATE",
                json!({ "access_token": "expired" }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHENTICATE", "evt": "ERROR", "nonce": request["nonce"],
                    "data": { "code": 4006, "message": "Token expired" }
                }),
            );
            let request = assert_rpc(
                &mut rpc,
                "AUTHORIZE",
                json!({
                    "client_id": "123456", "scopes": SCOPES
                }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHORIZE", "nonce": request["nonce"], "data": { "code": "test+code" }
                }),
            );
            let request = assert_rpc(
                &mut rpc,
                "AUTHENTICATE",
                json!({ "access_token": "new-token" }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHENTICATE", "nonce": request["nonce"],
                    "data": { "user": { "global_name": "Test account" } }
                }),
            );
        });

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/api/oauth2/token", listener.local_addr().unwrap());
        let http = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(RPC_TIMEOUT)).unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim_end(), "POST /api/oauth2/token HTTP/1.1");
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert_eq!(headers["content-type"], "application/x-www-form-urlencoded");
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            reader.read_exact(&mut body).unwrap();
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&body).into_owned().collect();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["client_id"], "123456");
            assert_eq!(form["client_secret"], "app+&secret");
            assert_eq!(form["code"], "test+code");
            let body = r#"{"access_token":"new-token"}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });

        let mut credentials = Credentials {
            client_id: "123456".into(),
            secret: Some("app+&secret".into()),
            access_token: Some("expired".into()),
        };
        let http_client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(RPC_TIMEOUT)
            .build()
            .unwrap();
        let session = credentials
            .authenticate_session(
                Rpc {
                    socket: client,
                    nonce: 0,
                },
                |id, secret, code| exchange_code(&http_client, &endpoint, id, secret, code),
            )
            .unwrap();
        assert_eq!(session.account, "Test account");
        assert_eq!(credentials.access_token.as_deref(), Some("new-token"));
        http.join().unwrap();
        ipc.join().unwrap();
    }
}
