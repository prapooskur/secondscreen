use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_QUALITY: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";

#[derive(Clone, Deserialize)]
pub(super) struct Place {
    pub name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    country: String,
}

pub(in crate::ui) fn coordinates(input: &str) -> Result<Option<(f64, f64)>> {
    let Some((lat, lon)) = input.split_once(',') else {
        return Ok(None);
    };
    let lat: f64 = lat
        .trim()
        .parse()
        .context("Use a city name or latitude, longitude.")?;
    let lon: f64 = lon
        .trim()
        .parse()
        .context("Use a city name or latitude, longitude.")?;
    ensure!(
        (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon),
        "Latitude must be within ±90° and longitude within ±180°."
    );
    Ok(Some((lat, lon)))
}

#[derive(Deserialize)]
struct Matches {
    #[serde(default)]
    results: Vec<Place>,
}

fn locate(client: &Client, query: &str) -> Result<Place> {
    if let Some((latitude, longitude)) = coordinates(query)? {
        return Ok(Place {
            name: query.to_owned(),
            latitude,
            longitude,
            country: String::new(),
        });
    }
    let mut found = client
        .get(GEOCODING)
        .query(&[("name", query), ("count", "1")])
        .send()
        .and_then(|r| r.error_for_status())
        .context("Location lookup unavailable. Try again later or enter coordinates.")?
        .json::<Matches>()
        .context("Invalid response from the location service.")?
        .results
        .into_iter()
        .next()
        .context("City not found. Try coordinates in Settings.")?;
    if !found.country.is_empty() {
        found.name = format!("{}, {}", found.name, found.country);
    }
    Ok(found)
}

#[derive(Deserialize)]
struct Current<T> {
    current: T,
}
#[derive(Deserialize)]
pub(super) struct Conditions {
    pub temperature_2m: f32,
    pub weather_code: Option<u8>,
}
#[derive(Deserialize)]
struct Air {
    us_aqi: f32,
}

pub(super) struct Report {
    pub place: Place,
    pub weather: Option<Conditions>,
    pub aqi: Option<f32>,
}

pub(super) fn fetch(query: &str, known: Option<Place>) -> Result<Report> {
    let client = Client::builder().timeout(Duration::from_secs(8)).build()?;
    let place = match known {
        Some(p) => p,
        None => locate(&client, query)?,
    };
    fetch_at(&client, place, FORECAST, AIR_QUALITY)
}

fn fetch_at(client: &Client, place: Place, forecast: &str, air_quality: &str) -> Result<Report> {
    let request = |url: &str, current: &str| {
        client
            .get(url)
            .query(&[("latitude", place.latitude), ("longitude", place.longitude)])
            .query(&[("current", current)])
            .send()?
            .error_for_status()
    };
    let weather = request(forecast, "temperature_2m,weather_code")
        .and_then(|r| r.json::<Current<Conditions>>())
        .ok()
        .map(|v| v.current);
    let aqi = request(air_quality, "us_aqi")
        .and_then(|r| r.json::<Current<Air>>())
        .ok()
        .map(|v| v.current.us_aqi);
    ensure!(
        weather.is_some() || aqi.is_some(),
        "Weather and air quality are unavailable."
    );
    Ok(Report {
        place,
        weather,
        aqi,
    })
}

pub(super) fn condition(code: Option<u8>) -> &'static str {
    match code {
        Some(0) => "Clear sky",
        Some(1) => "Mostly clear",
        Some(2) => "Partly cloudy",
        Some(3) => "Overcast",
        Some(45 | 48) => "Fog",
        Some(51..=57) => "Drizzle",
        Some(61..=65) => "Rain",
        Some(66 | 67) => "Freezing rain",
        Some(71..=77) => "Snow",
        Some(80..=82) => "Rain showers",
        Some(85 | 86) => "Snow showers",
        Some(95..=99) => "Thunderstorms",
        _ => "Current temperature",
    }
}

pub(super) fn air_category(aqi: f32) -> &'static str {
    match aqi.round() as u32 {
        0..=50 => "Good",
        51..=100 => "Moderate",
        101..=150 => "Sensitive groups at risk",
        151..=200 => "Unhealthy",
        201..=300 => "Very unhealthy",
        _ => "Hazardous",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
    };

    #[test]
    fn weather_and_air_quality_can_update_independently() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for response in [
                r#"{"current":{"weather_code":2,"temperature_2m":12.5}}"#,
                r#"{"current":{"us_aqi":42}}"#,
                r#"{"current":{"weather_code":2,"temperature_2m":12.5}}"#,
                r#"{"current":{"us_aqi":null}}"#,
            ] {
                let (mut conn, _) = server.accept().unwrap();
                let mut request = String::new();
                BufReader::new(&conn).read_line(&mut request).unwrap();
                assert!(request.contains("latitude=51.5") && request.contains("longitude=-0.12"));
                if response.contains("us_aqi") {
                    assert!(request.contains("current=us_aqi"));
                } else {
                    assert!(request.contains("current=temperature_2m%2Cweather_code"));
                }
                write!(
                    &mut conn,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let place = locate(&client, "51.5, -0.12").unwrap();
        for expected in [Some(42.), None] {
            let report = fetch_at(&client, place.clone(), &base, &base).unwrap();
            assert_eq!(report.weather.unwrap().temperature_2m, 12.5);
            assert_eq!(report.aqi, expected);
        }
        worker.join().unwrap();
        assert!(coordinates("91, 10").is_err());
    }
}
