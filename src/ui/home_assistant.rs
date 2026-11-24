mod api;
mod controls;
mod settings;
mod widget;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use async_channel::Sender;
use futures_lite::future;
use gpui::{Context, Entity, Task};
use serde::{Deserialize, Serialize};

use super::{Dashboard, Section, home::Kind, text_input::TextInput};
use crate::config::HomeAssistantCredentials;
use api::{Action, Connection, Device};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Binding {
    instance: String,
    entity: String,
}

struct Picker {
    profile: usize,
    widget: u32,
    search: Entity<TextInput>,
}

struct ConnectionFields {
    url: Entity<TextInput>,
    token: Entity<TextInput>,
}

struct Worker {
    commands: Sender<Command>,
    _task: Task<()>,
}

struct Command {
    id: String,
    action: Action,
}

#[derive(Default)]
pub(super) struct HomeAssistant {
    fields: Option<ConnectionFields>,
    picker: Option<Picker>,
    verification: Option<Task<()>>,
    verification_error: Option<String>,
    worker: Option<Worker>,
    devices: Option<Vec<Device>>,
    connection_error: Option<String>,
    pending: HashSet<String>,
    action_errors: HashMap<String, String>,
}

impl HomeAssistant {
    pub(super) fn dismiss_picker(&mut self) {
        self.picker = None;
    }

    pub(super) fn close_settings(&mut self) {
        self.dismiss_picker();
        self.fields = None;
    }

    fn stop_worker(&mut self) {
        self.worker = None;
        self.devices = None;
        self.connection_error = None;
        self.pending.clear();
        self.action_errors.clear();
    }

    fn record_devices(&mut self, result: anyhow::Result<Vec<Device>>) -> bool {
        match result {
            Ok(devices) => {
                let changed = self.connection_error.take().is_some()
                    || self.devices.as_ref() != Some(&devices);
                if changed {
                    self.devices = Some(devices);
                }
                changed
            }
            Err(error) => {
                let message = error.to_string();
                let changed = self.connection_error.as_ref() != Some(&message);
                self.connection_error = Some(message);
                changed
            }
        }
    }

    fn finish_command(&mut self, id: &str, result: anyhow::Result<Device>) {
        self.pending.remove(id);
        match result {
            Ok(device) => {
                if let Some(previous) = self.devices.iter_mut().flatten().find(|item| item.id == id)
                {
                    *previous = device;
                }
            }
            Err(error) => {
                self.action_errors.insert(id.to_owned(), error.to_string());
            }
        }
    }
}

impl Dashboard {
    pub(super) fn sync_home_assistant(&mut self, cx: &mut Context<Self>) {
        let needed = self.home.contains(Kind::HomeAssistant)
            || self.showing_settings(Section::HomeAssistant)
            || !self.home_assistant.pending.is_empty();
        let Some(credentials) = self.preferences.home_assistant.as_ref().filter(|_| needed) else {
            if self.home_assistant.worker.is_some() {
                self.home_assistant.stop_worker();
            }
            return;
        };
        if self.home_assistant.worker.is_none() {
            self.start_home_assistant(credentials.clone(), None, cx);
        }
    }

    fn start_home_assistant(
        &mut self,
        credentials: HomeAssistantCredentials,
        verified: Option<(Connection, Vec<Device>)>,
        cx: &mut Context<Self>,
    ) {
        self.home_assistant.stop_worker();
        let (commands, receiver) = async_channel::unbounded();
        let connection = verified.map(|(connection, devices)| {
            self.home_assistant.devices = Some(devices);
            connection
        });
        let task = cx.spawn(async move |this, cx| {
            let mut refresh_at = Instant::now();
            let connection = match connection {
                Some(connection) => {
                    refresh_at += REFRESH_INTERVAL;
                    Ok(connection)
                }
                None => blocking::unblock(move || Connection::new(&credentials)).await,
            };
            let connection = match connection {
                Ok(connection) => Arc::new(connection),
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.home_assistant.record_devices(Err(error));
                        cx.notify();
                    });
                    return;
                }
            };
            loop {
                let delay = refresh_at.saturating_duration_since(Instant::now());
                let timer = cx.background_executor().timer(delay);
                let command = future::or(async { Some(receiver.recv().await) }, async {
                    timer.await;
                    None
                })
                .await;
                match command {
                    Some(Ok(Command { id, action })) => {
                        let connection = connection.clone();
                        let (id, result) = blocking::unblock(move || {
                            let result = connection.act(&id, action).and_then(|()| {
                                connection.entity(&id).map_err(|error| {
                                    anyhow::anyhow!("Command sent. State refresh failed. {error}")
                                })
                            });
                            (id, result)
                        })
                        .await;
                        if this
                            .update(cx, |this, cx| {
                                this.home_assistant.finish_command(&id, result);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(Err(_)) => break,
                    None => {
                        let connection = connection.clone();
                        let result = blocking::unblock(move || connection.entities()).await;
                        refresh_at = Instant::now() + REFRESH_INTERVAL;
                        if this
                            .update(cx, |this, cx| {
                                if this.home_assistant.record_devices(result) {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        self.home_assistant.worker = Some(Worker {
            commands,
            _task: task,
        });
    }

    fn send_home_assistant(&mut self, id: String, action: Action, cx: &mut Context<Self>) {
        let integration = &mut self.home_assistant;
        if integration.connection_error.is_some()
            || integration.pending.contains(&id)
            || !integration
                .devices
                .iter()
                .flatten()
                .any(|device| device.id == id && device.available())
        {
            return;
        }
        if let Some(worker) = &integration.worker {
            if worker
                .commands
                .try_send(Command {
                    id: id.clone(),
                    action,
                })
                .is_ok()
            {
                integration.action_errors.remove(&id);
                integration.pending.insert(id);
            } else {
                integration.connection_error = Some("Home Assistant connection stopped.".into());
            }
            cx.notify();
        }
    }
}
