mod app_icons;
mod audio;
mod discord;
mod media;

use async_channel::Sender;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use crate::{config::DiscordCredentials, model::Update, monitor::Monitor, platform::Controls};

pub fn start(
    updates: Sender<Update>,
    discord_credentials: Option<DiscordCredentials>,
) -> std::io::Result<Controls> {
    let monitor_tx = updates.clone();
    let monitor_thread = thread::Builder::new().name("system-monitor".into());
    monitor_thread.spawn(move || {
        let mut monitor = match Monitor::new() {
            Ok(monitor) => monitor,
            Err(error) => {
                log::error!("Could not initialize system monitoring: {error}");
                let _ = monitor_tx.send_blocking(Update::ActionError(
                    "System monitoring is unavailable.".into(),
                ));
                return;
            }
        };
        loop {
            if monitor_tx
                .send_blocking(Update::Metrics(monitor.sample()))
                .is_err()
            {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
    })?;
    let audio = worker(
        "audio-mixer",
        Duration::from_millis(1250),
        updates.clone(),
        audio::Mixer::default,
    )?;
    let media = worker(
        "media-player",
        Duration::from_millis(900),
        updates.clone(),
        media::Media::default,
    )?;

    let discord = worker("discord-rpc", Duration::from_secs(2), updates, move || {
        discord::Discord::new(discord_credentials)
    })?;
    Ok(Controls {
        audio,
        media,
        discord,
    })
}

trait Integration {
    type Action: Send + 'static;

    fn poll(&mut self) -> Update;
    fn handle(&mut self, action: Self::Action, updates: &Sender<Update>) -> anyhow::Result<()>;
}

fn worker<I: Integration>(
    name: &str,
    interval: Duration,
    updates: Sender<Update>,
    create: impl FnOnce() -> I + Send + 'static,
) -> std::io::Result<mpsc::Sender<I::Action>> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new().name(name.into()).spawn(move || {
        let mut integration = create();
        let mut next_poll = Instant::now();
        while !updates.is_closed() {
            match receiver.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                Ok(action) => {
                    if let Err(error) = integration.handle(action, &updates) {
                        let _ = updates.send_blocking(Update::ActionError(error.to_string()));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if updates.send_blocking(integration.poll()).is_err() {
                break;
            }
            next_poll = Instant::now() + interval;
        }
    })?;
    Ok(commands)
}
