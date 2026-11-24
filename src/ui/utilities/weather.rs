use anyhow::{Context, Result, ensure};
use jiff::civil::{Date, DateTime, Time};
use reqwest::{
    StatusCode,
    blocking::{Client, RequestBuilder},
};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;

const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_QUALITY: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";
const CURRENT_FIELDS: &str = "temperature_2m,weather_code,is_day,wind_speed_10m";
const DAILY_FIELDS: &str = "weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset";
const SHORT_TIMEOUT: Duration = Duration::from_secs(8);
const FORECAST_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const FORECAST_DAYS: usize = 10;
pub(super) const HOURS_PER_DAY: usize = 24;

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

fn get_json<T: DeserializeOwned>(request: RequestBuilder, service: &str) -> Result<T> {
    request
        .send()
        .and_then(|response| response.error_for_status()?.json())
        .map_err(|error| {
            let message = if let Some(status) = error.status() {
                format!("{service} returned HTTP {}", status.as_u16())
            } else if error.is_timeout() {
                format!("{service} timed out")
            } else if error.is_decode() {
                format!("{service} returned an invalid response")
            } else {
                format!("Could not reach {service}")
            };
            let error = anyhow::Error::new(error.without_url());
            log::warn!("{message} · {error:#}");
            error.context(format!("{message}."))
        })
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
    let request = client
        .get(GEOCODING)
        .timeout(SHORT_TIMEOUT)
        .query(&[("name", query), ("count", "1")]);
    let mut found = get_json::<Matches>(request, "Open-Meteo location lookup")?
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
struct AirResponse {
    current: Option<Air>,
}
#[derive(Default, Deserialize)]
pub(super) struct Conditions {
    time: Option<String>,
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

#[derive(Default, Deserialize)]
#[serde(default)]
struct HourlyReading {
    time: Vec<String>,
    temperature_2m: Vec<Option<f32>>,
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: Option<Conditions>,
    daily: Option<DailyReading>,
    hourly: Option<HourlyReading>,
}

pub(super) struct Weather {
    pub current: Option<Conditions>,
    pub days: Vec<Day>,
    pub today: Option<Date>,
    pub time: Option<Time>,
    pub hourly: [Option<f32>; HOURS_PER_DAY],
}

impl Weather {
    pub(super) fn has_hourly_temperatures(&self) -> bool {
        self.hourly.iter().any(Option::is_some)
    }
}

impl ForecastResponse {
    fn weather(self) -> Option<Weather> {
        let current_time = self
            .current
            .as_ref()
            .and_then(|current| current.time.as_deref()?.parse::<DateTime>().ok());
        let today = current_time
            .map(|time| time.date())
            .or_else(|| self.daily.as_ref()?.time.first()?.parse().ok())
            .or_else(|| {
                self.hourly
                    .as_ref()?
                    .time
                    .first()?
                    .parse::<DateTime>()
                    .ok()
                    .map(|time| time.date())
            });
        let mut hourly = [None; HOURS_PER_DAY];
        if let (Some(today), Some(readings)) = (today, self.hourly) {
            for (time, temperature) in readings.time.iter().zip(readings.temperature_2m) {
                if let Some(temperature) = temperature.filter(|value| value.is_finite())
                    && let Ok(time) = time.parse::<DateTime>()
                    && time.date() == today
                {
                    hourly[time.hour() as usize] = Some(temperature);
                }
            }
        }
        let current = self.current.filter(|reading| {
            reading.temperature_2m.is_some()
                || reading.weather_code.is_some()
                || reading.wind_speed_10m.is_some()
        });
        let days = self.daily.map_or_else(Vec::new, |daily| daily.days());
        let weather = Weather {
            current,
            days,
            today,
            time: current_time.map(|time| time.time()),
            hourly,
        };
        (weather.current.is_some() || !weather.days.is_empty() || weather.has_hourly_temperatures())
            .then_some(weather)
    }
}

#[derive(Deserialize)]
struct Air {
    us_aqi: Option<f32>,
}

pub(super) struct Report {
    pub place: Place,
    pub weather: Option<Weather>,
    pub aqi: Option<f32>,
}

pub(super) fn fetch(query: &str, known: Option<Place>) -> Result<Report> {
    let client = Client::builder()
        .timeout(FORECAST_TIMEOUT)
        .build()
        .context("Could not initialize the weather connection.")?;
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
    let forecast_request = |days| {
        request(forecast)
            .query(&[
                ("timezone", "auto"),
                ("current", CURRENT_FIELDS),
                ("daily", DAILY_FIELDS),
                ("hourly", "temperature_2m"),
                ("temperature_unit", "celsius"),
                ("wind_speed_unit", "kmh"),
            ])
            .query(&[
                ("forecast_days", days),
                ("forecast_hours", HOURS_PER_DAY),
                ("initial_hours", 0),
            ])
    };
    let forecast_response =
        get_json::<ForecastResponse>(forecast_request(FORECAST_DAYS), "Open-Meteo forecast")
            .or_else(|error| {
                if error.downcast_ref::<reqwest::Error>().is_some_and(|cause| {
                    cause.is_timeout() || cause.status() == Some(StatusCode::GATEWAY_TIMEOUT)
                }) {
                    get_json(
                        forecast_request(1).timeout(SHORT_TIMEOUT),
                        "Open-Meteo one-day forecast",
                    )
                } else {
                    Err(error)
                }
            });
    let weather = forecast_response.and_then(|response| {
        response
            .weather()
            .context("Open-Meteo returned no forecast data.")
    });
    let air_request = request(air_quality)
        .timeout(SHORT_TIMEOUT)
        .query(&[("current", "us_aqi")]);
    let aqi = get_json::<AirResponse>(air_request, "Open-Meteo air quality")
        .ok()
        .and_then(|response| response.current?.us_aqi);
    let weather = if aqi.is_some() {
        weather.ok()
    } else {
        Some(weather?)
    };
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

pub(super) fn temperature(value: Option<f32>, fahrenheit: bool, show_unit: bool) -> String {
    let Some(celsius) = value else {
        return "—".into();
    };
    let value = if fahrenheit {
        celsius * 1.8 + 32.
    } else {
        celsius
    };
    let unit = if show_unit {
        temperature_unit(fahrenheit)
    } else {
        "°"
    };
    format!("{:.0}{unit}", value.round())
}

pub(super) fn temperature_unit(fahrenheit: bool) -> &'static str {
    if fahrenheit { "°F" } else { "°C" }
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
        net::{TcpListener, TcpStream},
    };

    fn request_line(conn: &TcpStream) -> String {
        let mut request = String::new();
        BufReader::new(conn).read_line(&mut request).unwrap();
        request
    }

    fn respond(mut conn: TcpStream, status: &str, response: &str) {
        write!(
            conn,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
    }

    #[test]
    fn weather_and_air_quality_can_update_independently() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let forecast = r#"{
                "current":{"time":"2026-12-28T14:15","weather_code":2,"temperature_2m":12.5,"is_day":1,"wind_speed_10m":16},
                "hourly":{
                    "time":["2026-12-28T00:00","2026-12-28T06:00","2026-12-28T14:00",
                            "2026-12-28T23:00","2026-12-29T00:00","2026-12-29T06:00"],
                    "temperature_2m":[9.1,null,12.3,8,2,0]
                },
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
            for (path, status, response) in [
                ("/forecast", "200 OK", forecast),
                ("/air", "200 OK", r#"{"current":{"us_aqi":42}}"#),
                ("/forecast", "200 OK", forecast),
                ("/air", "200 OK", r#"{"current":{"us_aqi":null}}"#),
                ("/forecast", "503 Service Unavailable", "{}"),
                ("/air", "200 OK", r#"{"current":{"us_aqi":52}}"#),
                ("/forecast", "403 Forbidden", "{}"),
                ("/air", "200 OK", r#"{"current":null}"#),
            ] {
                let (conn, _) = server.accept().unwrap();
                let request = request_line(&conn);
                assert!(request.starts_with(&format!("GET {path}?")));
                assert!(request.contains("latitude=51.5") && request.contains("longitude=-0.12"));
                if path == "/air" {
                    assert!(request.contains("current=us_aqi"));
                } else {
                    assert!(
                        request.contains("timezone=auto") && request.contains("forecast_days=10")
                    );
                    assert!(request.contains("hourly=temperature_2m"));
                    assert!(request.contains("forecast_hours=24"));
                    assert!(request.contains("initial_hours=0"));
                    assert!(
                        request.contains("current=temperature_2m%2Cweather_code%2Cis_day%2Cwind_speed_10m")
                            && request.contains("daily=weather_code%2Ctemperature_2m_max%2Ctemperature_2m_min%2Csunrise%2Csunset")
                    );
                }
                respond(conn, status, response);
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let place = locate(&client, "51.5, -0.12").unwrap();
        let forecast = format!("{base}/forecast");
        let air = format!("{base}/air");
        let mut utilities = super::super::Utilities::default();
        for expected in [Some(42.), None] {
            let report = fetch_at(&client, place.clone(), &forecast, &air).unwrap();
            assert_eq!(report.aqi, expected);
            let weather = report.weather.as_ref().unwrap();
            let current = weather.current.as_ref().unwrap();
            assert_eq!(current.temperature_2m, Some(12.5));
            assert_eq!(current.wind_speed_10m, Some(16.));
            assert_eq!(weather.days.len(), FORECAST_DAYS);
            assert_eq!(weather.today, Some(weather.days[0].date));
            assert_eq!(weather.time.unwrap().to_string(), "14:15:00");
            assert_eq!(weather.hourly[0], Some(9.1));
            assert_eq!(weather.hourly[6], None);
            assert_eq!(weather.hourly[14], Some(12.3));
            assert_eq!(weather.hourly[23], Some(8.));
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
            utilities.record_weather(report);
        }
        let air_only = fetch_at(&client, place.clone(), &forecast, &air).unwrap();
        assert!(air_only.weather.is_none());
        let forecast_at = utilities.weather_at;
        utilities.record_weather(air_only);
        let combined = utilities.report.unwrap();
        assert_eq!(combined.aqi, Some(52.));
        assert_eq!(combined.weather.unwrap().days.len(), FORECAST_DAYS);
        assert_eq!(utilities.weather_at, forecast_at);
        assert!(utilities.weather_error.is_some());
        let Err(error) = fetch_at(&client, place, &forecast, &air) else {
            panic!("a denied forecast and missing AQI should fail");
        };
        assert_eq!(error.to_string(), "Open-Meteo forecast returned HTTP 403.");
        assert!(!format!("{error:#}").contains("latitude="));
        worker.join().unwrap();
        assert!(coordinates("91, 10").is_err());
    }

    #[test]
    fn forecast_timeout_falls_back_to_today() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (stalled, _) = server.accept().unwrap();
            assert!(request_line(&stalled).contains("forecast_days=10&"));

            // Keep the first connection open until the client times out and retries.
            let (fallback, _) = server.accept().unwrap();
            drop(stalled);
            assert!(request_line(&fallback).contains("forecast_days=1&"));
            respond(
                fallback,
                "200 OK",
                r#"{"current":{"temperature_2m":12.5},"daily":{"time":["2026-12-28"]}}"#,
            );

            let (air, _) = server.accept().unwrap();
            assert!(request_line(&air).starts_with("GET /air?"));
            respond(air, "200 OK", r#"{"current":{"us_aqi":42}}"#);
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        let place = locate(&client, "51.5, -0.12").unwrap();
        let report = fetch_at(
            &client,
            place,
            &format!("{base}/forecast"),
            &format!("{base}/air"),
        )
        .unwrap();
        let weather = report.weather.unwrap();
        assert_eq!(weather.days.len(), 1);
        assert_eq!(weather.current.unwrap().temperature_2m, Some(12.5));
        assert_eq!(report.aqi, Some(42.));
        worker.join().unwrap();
    }

    #[test]
    fn hourly_forecast_can_render_without_current_or_daily_weather() {
        let response = serde_json::from_str::<ForecastResponse>(
            r#"{"hourly":{
                "time":["2026-12-28T00:00","2026-12-28T01:00","2026-12-28T02:00"],
                "temperature_2m":[-1.5,0]
            }}"#,
        )
        .unwrap();
        let weather = response.weather().unwrap();
        assert!(weather.current.is_none());
        assert!(weather.days.is_empty());
        assert!(weather.time.is_none());
        assert_eq!(weather.hourly[..3], [Some(-1.5), Some(0.), None]);
    }
}
