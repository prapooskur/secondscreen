use std::time::Duration;

use gpui::{Context, Task, Window, WindowBackgroundAppearance};
use rustix::time::{ClockId, clock_gettime};

use crate::platform::launch_display::{self, Display, Displays};

use super::{Dashboard, WINDOW_TITLE, settings::Section};

const SCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct DisplayWatch {
    pub fullscreen: bool,
    pub placing_at_startup: bool,
    pub displays: Option<Result<Vec<Display>, String>>,
    pub placement_error: Option<(String, String)>,
    task: Option<Task<()>>,
}

impl DisplayWatch {
    pub fn new(fullscreen: bool, placing_at_startup: bool) -> Self {
        Self {
            fullscreen,
            placing_at_startup,
            ..Self::default()
        }
    }

    fn reveal(&mut self, window: &mut Window) -> bool {
        if !std::mem::take(&mut self.placing_at_startup) {
            return false;
        }
        window.set_window_title(WINDOW_TITLE);
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        true
    }
}

fn time_suspended() -> Duration {
    // Boottime includes time spent suspended. Monotonic time does not.
    let elapsed = |clock| Duration::try_from(clock_gettime(clock)).unwrap_or_default();
    elapsed(ClockId::Boottime).saturating_sub(elapsed(ClockId::Monotonic))
}

impl Dashboard {
    pub(super) fn sync_display(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = (self.preferences.launch_display.is_some()
            || self.showing_settings(Section::General))
            && launch_display::supported();
        if !active {
            self.display.task = None;
            self.display.displays = None;
            self.display.placement_error = None;
            self.display.reveal(window);
            return;
        }
        if self.display.task.is_some() {
            return;
        }

        self.display.task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut tracker = None;
            let mut placed = None;
            let mut suspended = time_suspended();
            loop {
                let now = time_suspended();
                if now.saturating_sub(suspended) > Duration::from_millis(250) {
                    placed = None;
                    suspended = now;
                }

                let (next, displays) = blocking::unblock(move || {
                    let mut next = tracker;
                    let result = match &mut next {
                        Some(tracker) => Displays::scan(tracker),
                        None => Displays::open().and_then(|tracker| next.insert(tracker).scan()),
                    };
                    if result.is_err() {
                        next = None;
                    }
                    (next, result.map_err(|error| format!("{error:#}")))
                })
                .await;
                tracker = next;
                let Ok(target) = this.update_in(cx, |this, window, cx| {
                    let selected = this.preferences.launch_display.as_deref();
                    let target = displays
                        .as_ref()
                        .ok()
                        .and_then(|displays| {
                            displays.iter().find(|d| Some(d.name.as_str()) == selected)
                        })
                        .map(|display| (display.name.clone(), display.id));
                    let error = &mut this.display.placement_error;
                    let clear_error = error.as_ref().is_some_and(|(name, _)| {
                        target.as_ref().is_none_or(|(target, _)| target != name)
                    });
                    if clear_error {
                        *error = None;
                    }
                    let changed = this.display.displays.as_ref() != Some(&displays);
                    if changed {
                        this.display.displays = Some(displays);
                    }
                    let revealed = target.is_none() && this.display.reveal(window);
                    if revealed
                        || (changed || clear_error) && this.showing_settings(Section::General)
                    {
                        cx.notify();
                    }
                    target.map(|target| (target, this.display.fullscreen))
                }) else {
                    break;
                };

                if let Some((target, fullscreen)) = target {
                    if placed.as_ref() != Some(&target) {
                        let name = target.0.clone();
                        let error = blocking::unblock(move || {
                            launch_display::place_window(&name, fullscreen)
                                .err()
                                .map(|error| (name, format!("{error:#}")))
                        })
                        .await;
                        let Ok(current) = this.update_in(cx, |this, window, cx| {
                            if this.preferences.launch_display.as_ref() != Some(&target.0) {
                                return false;
                            }
                            if this.display.placement_error != error {
                                if let Some((name, error)) = &error {
                                    log::warn!("Could not place the dashboard on {name}: {error}");
                                }
                                this.display.placement_error = error.clone();
                                if this.showing_settings(Section::General) {
                                    cx.notify();
                                }
                            }
                            if this.display.reveal(window) {
                                cx.notify();
                            }
                            error.is_none() && this.display.fullscreen == fullscreen
                        }) else {
                            break;
                        };
                        placed = current.then_some(target);
                    }
                } else {
                    placed = None;
                }
                cx.background_executor().timer(SCAN_INTERVAL).await;
            }
        }));
    }
}
