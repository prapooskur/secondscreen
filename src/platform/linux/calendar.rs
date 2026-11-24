use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};

use jiff::{Span, Timestamp, Zoned, civil::Date, tz::TimeZone};

const UNAVAILABLE: &str = "System calendar unavailable";

pub struct Event {
    pub title: String,
    pub start: Zoned,
    pub all_day: bool,
    last_day: Date,
}

impl Event {
    pub fn occurs_on(&self, date: Date) -> bool {
        self.start.date() <= date && date <= self.last_day
    }

    fn from_instance(instance: Instance, local_zone: &TimeZone) -> Option<Self> {
        let zone = if instance.all_day {
            &TimeZone::UTC
        } else {
            local_zone
        };
        let start = Timestamp::from_second(instance.start)
            .ok()?
            .to_zoned(zone.clone());
        let last_day = Timestamp::from_second(instance.end.saturating_sub(1).max(instance.start))
            .ok()?
            .to_zoned(zone.clone())
            .date();
        Some(Self {
            title: instance.title,
            start,
            all_day: instance.all_day,
            last_day,
        })
    }
}

struct Instance {
    title: String,
    start: i64,
    end: i64,
    all_day: bool,
}

#[derive(Default)]
pub struct Month {
    pub events: Vec<Event>,
    pub issue: Option<&'static str>,
}

unsafe extern "C" {
    fn secondscreen_calendar_events(
        start: i64,
        end: i64,
        timezone: *const c_char,
        callback: extern "C" fn(*mut c_void, *const c_char, i64, i64, c_int),
        data: *mut c_void,
    ) -> c_int;
}

extern "C" fn collect(
    data: *mut c_void,
    title: *const c_char,
    start: i64,
    end: i64,
    all_day: c_int,
) {
    // The native bridge calls back synchronously and supplies a borrowed, nullable title.
    let instances = unsafe { &mut *data.cast::<Vec<Instance>>() };
    let title = if title.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(title) }
            .to_string_lossy()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    instances.push(Instance {
        title: if title.is_empty() {
            "Untitled event".into()
        } else {
            title
        },
        start,
        end,
        all_day: all_day != 0,
    });
}

pub fn fetch(first: Date) -> Month {
    let zone = TimeZone::system();
    let result = (|| -> Result<Month, jiff::Error> {
        let next = first.checked_add(Span::new().months(1))?;
        // EDS can index floating events in a different zone than the local display.
        let start = first.yesterday()?.to_zoned(zone.clone())?.timestamp();
        let end = next.tomorrow()?.to_zoned(zone.clone())?.timestamp();
        if end.as_second() <= 0 {
            return Ok(Month::default());
        }
        let timezone = zone.iana_name().and_then(|name| CString::new(name).ok());
        let mut instances = Vec::<Instance>::new();
        // The accumulator and timezone string live through all callbacks.
        let status = unsafe {
            secondscreen_calendar_events(
                start.as_second().max(0),
                end.as_second(),
                timezone.as_ref().map_or(ptr::null(), |name| name.as_ptr()),
                collect,
                ptr::from_mut(&mut instances).cast(),
            )
        };
        let mut events: Vec<_> = instances
            .into_iter()
            .filter_map(|instance| Event::from_instance(instance, &zone))
            .filter(|event| event.last_day >= first && event.start.date() < next)
            .collect();
        events.sort_by(|a, b| {
            (!a.all_day, &a.start, &a.title).cmp(&(!b.all_day, &b.start, &b.title))
        });
        Ok(Month {
            events,
            issue: match status {
                0 => None,
                1 => Some("Some calendars are unavailable"),
                _ => Some(UNAVAILABLE),
            },
        })
    })();
    result.unwrap_or(Month {
        events: Vec::new(),
        issue: Some(UNAVAILABLE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_dates_preserve_all_day_dates_and_exclude_midnight_ends() {
        let zone = TimeZone::get("America/Los_Angeles").unwrap();
        let event = |start: &str, end: &str, all_day| {
            Event::from_instance(
                Instance {
                    title: String::new(),
                    start: start.parse::<Timestamp>().unwrap().as_second(),
                    end: end.parse::<Timestamp>().unwrap().as_second(),
                    all_day,
                },
                &zone,
            )
            .unwrap()
        };
        let first = Date::new(2026, 11, 23).unwrap();
        let next = first.tomorrow().unwrap();
        let all_day = event("2026-11-23T00:00:00Z", "2026-11-25T00:00:00Z", true);
        assert!(!all_day.occurs_on(first.yesterday().unwrap()));
        assert!(all_day.occurs_on(first));
        assert!(all_day.occurs_on(next));
        assert!(!all_day.occurs_on(next.tomorrow().unwrap()));

        let timed = event("2026-11-24T06:00:00Z", "2026-11-24T08:00:00Z", false);
        assert_eq!(timed.start.hour(), 22);
        assert!(timed.occurs_on(first));
        assert!(!timed.occurs_on(next));
    }
}
