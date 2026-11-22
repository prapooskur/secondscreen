use anyhow::{Context, Result, ensure};
use jiff::civil::{Date, DateTime, Time};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_QUALITY: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";
const CURRENT_FIELDS: &str = "temperature_2m,weather_code,is_day,wind_speed_10m";
const DAILY_FIELDS: &str = "weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset";
pub(super) const FORECAST_DAYS: usize = 10;

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
#[derive(Default, Deserialize)]
pub(super) struct Conditions {
    pub temperature_2m: Option<f32>,
    pub weather_code: Option<u8>,
    pub is_day: Option<u8>,
    pub wind_speed_10m: Option<f32>,
}

pub(super) struct Day {
    pub date: Date,
    pub weather_code: Option<u8>,
    pub high: Option<f32>,
    pub low: Option<f32>,
    pub sunrise: Option<Time>,
    pub sunset: Option<Time>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct DailyReading {
    time: Vec<String>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_max: Vec<Option<f32>>,
    temperature_2m_min: Vec<Option<f32>>,
    sunrise: Vec<Option<String>>,
    sunset: Vec<Option<String>>,
}

impl DailyReading {
    fn days(&self) -> Vec<Day> {
        let local_time = |values: &[Option<String>], index: usize| {
            values
                .get(index)
                .and_then(Option::as_deref)
                .and_then(|value| value.parse::<DateTime>().ok())
                .map(|value| value.time())
        };
        self.time
            .iter()
            .take(FORECAST_DAYS)
            .enumerate()
            .filter_map(|(index, date)| {
                Some(Day {
                    date: date.parse().ok()?,
                    weather_code: self.weather_code.get(index).copied().flatten(),
                    high: self.temperature_2m_max.get(index).copied().flatten(),
                    low: self.temperature_2m_min.get(index).copied().flatten(),
                    sunrise: local_time(&self.sunrise, index),
                    sunset: local_time(&self.sunset, index),
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: Option<Conditions>,
    daily: Option<DailyReading>,
}

pub(super) struct Weather {
    pub current: Option<Conditions>,
    pub days: Vec<Day>,
    pub today: Option<Date>,
}

impl ForecastResponse {
    fn weather(self) -> Option<Weather> {
        let current = self.current.filter(|reading| {
            reading.temperature_2m.is_some()
                || reading.weather_code.is_some()
                || reading.wind_speed_10m.is_some()
        });
        let today = self
            .daily
            .as_ref()
            .and_then(|daily| daily.time.first()?.parse().ok());
        let days = self.daily.map_or_else(Vec::new, |daily| daily.days());
        (current.is_some() || !days.is_empty()).then_some(Weather {
            current,
            days,
            today,
        })
    }
}

#[derive(Deserialize)]
struct Air {
    us_aqi: f32,
}

pub(super) struct Report {
    pub place: Place,
    pub weather: Option<Weather>,
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
    let request = |url: &str| {
        client
            .get(url)
            .query(&[("latitude", place.latitude), ("longitude", place.longitude)])
    };
    let weather = request(forecast)
        .query(&[
            ("timezone", "auto"),
            ("current", CURRENT_FIELDS),
            ("daily", DAILY_FIELDS),
            ("temperature_unit", "celsius"),
            ("wind_speed_unit", "kmh"),
        ])
        .query(&[("forecast_days", FORECAST_DAYS)])
        .send()
        .and_then(|r| r.error_for_status()?.json::<ForecastResponse>())
        .ok()
        .and_then(ForecastResponse::weather);
    let aqi = request(air_quality)
        .query(&[("current", "us_aqi")])
        .send()
        .and_then(|r| r.error_for_status()?.json::<Current<Air>>())
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
        _ => "Conditions unavailable",
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
            let forecast = r#"{
                "current":{"weather_code":2,"temperature_2m":12.5,"is_day":1,"wind_speed_10m":16},
                "daily":{
                    "time":["2026-12-28","2026-12-29","2026-12-30","2026-12-31","2027-01-01",
                            "2027-01-02","2027-01-03","2027-01-04","2027-01-05","2027-01-06"],
                    "temperature_2m_max":[15,null,12],
                    "temperature_2m_min":[8,7,4],
                    "weather_code":[2,45,63],
                    "sunrise":["2026-12-28T08:04",null],
                    "sunset":["2026-12-28T15:58",null]
                }
            }"#;
            for response in [
                forecast,
                r#"{"current":{"us_aqi":42}}"#,
                forecast,
                r#"{"current":{"us_aqi":null}}"#,
                r#"{"current":null,"daily":null}"#,
                r#"{"current":{"us_aqi":52}}"#,
            ] {
                let (mut conn, _) = server.accept().unwrap();
                let mut request = String::new();
                BufReader::new(&conn).read_line(&mut request).unwrap();
                assert!(request.contains("latitude=51.5") && request.contains("longitude=-0.12"));
                if response.contains("us_aqi") {
                    assert!(request.contains("current=us_aqi"));
                } else {
                    assert!(
                        request.contains("timezone=auto") && request.contains("forecast_days=10")
                    );
                    assert!(
                        request.contains("current=temperature_2m%2Cweather_code%2Cis_day%2Cwind_speed_10m")
                            && request.contains("daily=weather_code%2Ctemperature_2m_max%2Ctemperature_2m_min%2Csunrise%2Csunset")
                    );
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
            assert_eq!(report.aqi, expected);
            let weather = report.weather.unwrap();
            let current = weather.current.unwrap();
            assert_eq!(current.temperature_2m, Some(12.5));
            assert_eq!(current.wind_speed_10m, Some(16.));
            assert_eq!(weather.days.len(), FORECAST_DAYS);
            assert_eq!(weather.today, Some(weather.days[0].date));
            let first = &weather.days[0];
            assert_eq!(
                first.sunrise.unwrap().strftime("%H:%M").to_string(),
                "08:04"
            );
            assert_eq!(
                first.sunset.unwrap().strftime("%-I:%M %p").to_string(),
                "3:58 PM"
            );
            assert_eq!(weather.days[1].high, None);
            assert_eq!(weather.days[1].low, Some(7.));
            assert_eq!(weather.days[2].high, Some(12.));
            assert_eq!(weather.days[2].weather_code, Some(63));
            assert_eq!(weather.days[9].date.to_string(), "2027-01-06");
        }
        let air_only = fetch_at(&client, place, &base, &base).unwrap();
        assert!(air_only.weather.is_none());
        assert_eq!(air_only.aqi, Some(52.));
        worker.join().unwrap();
        assert!(coordinates("91, 10").is_err());
    }
}
