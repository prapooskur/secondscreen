mod app_icons;
mod audio;
pub mod autostart;
pub mod calendar;
pub mod desktop_apps;
mod discord;
pub mod launch_display;
mod media;
pub mod touch;

use async_channel::{RecvError, Sender};
use async_io::Timer;
use futures_lite::future;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use crate::{
    config::DiscordCredentials,
    model::Update,
    monitor::{Monitor, Sources, storage},
    platform::Controls,
};

const STORAGE_INTERVAL: Duration = Duration::from_secs(5);

pub fn start(
    updates: Sender<Update>,
    discord_credentials: Option<DiscordCredentials>,
) -> std::io::Result<Controls> {
    let monitor = monitor_worker(updates.clone())?;
    let storage = storage_worker(updates.clone())?;
    let audio = worker("audio-mixer", updates.clone(), audio::Mixer::default)?;
    let media = worker("media-player", updates.clone(), media::Media::default)?;
    let discord = worker("discord-rpc", updates, move || {
        discord::Discord::new(discord_credentials)
    })?;
    Ok(Controls {
        monitor,
        storage,
        audio,
        media,
        discord,
    })
}

fn monitor_worker(updates: Sender<Update>) -> std::io::Result<mpsc::Sender<Option<Sources>>> {
    let (commands, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("system-monitor".into())
        .spawn(move || {
            while let Ok(request) = receiver.recv() {
                let Some(mut sources) = request else { continue };
                let mut monitor = match Monitor::new() {
                    Ok(monitor) => monitor,
                    Err(error) => {
                        log::error!("Could not initialize system monitoring: {error}");
                        let _ = updates.send_blocking(Update::ActionError(
                            "System monitoring is unavailable.".into(),
                        ));
                        return;
                    }
                };
                'sampling: loop {
                    if updates
                        .send_blocking(Update::Metrics(monitor.sample(sources)))
                        .is_err()
                    {
                        return;
                    }
                    let deadline = Instant::now() + Duration::from_secs(1);
                    loop {
                        match receiver
                            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        {
                            Ok(Some(next)) => sources = next,
                            Ok(None) => break 'sampling,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
            }
        })?;
    Ok(commands)
}

fn storage_worker(updates: Sender<Update>) -> std::io::Result<mpsc::Sender<bool>> {
    let (commands, receiver) = mpsc::channel::<bool>();
    thread::Builder::new()
        .name("storage-monitor".into())
        .spawn(move || {
            while let Ok(active) = receiver.recv() {
                if !active {
                    continue;
                }
                loop {
                    if updates
                        .send_blocking(Update::Storage(storage::sample()))
                        .is_err()
                    {
                        return;
                    }
                    match receiver.recv_timeout(STORAGE_INTERVAL) {
                        Ok(true) | Err(RecvTimeoutError::Timeout) => {}
                        Ok(false) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        })?;
    Ok(commands)
}

trait Integration {
    type Action: Send + 'static;

    fn poll(&mut self) -> Update;
    fn handle(&mut self, action: Self::Action, updates: &Sender<Update>) -> anyhow::Result<()>;

    fn poll_interval(&self) -> Duration;

    async fn wait_for_event(&mut self);

    fn on_event(&mut self) -> Option<Update> {
        Some(self.poll())
    }
}

enum Wake<A> {
    Action(Result<A, RecvError>),
    Poll,
    Event,
}

fn worker<I: Integration>(
    name: &str,
    updates: Sender<Update>,
    create: impl FnOnce() -> I + Send + 'static,
) -> std::io::Result<Sender<I::Action>> {
    let (commands, receiver) = async_channel::unbounded();
    thread::Builder::new().name(name.into()).spawn(move || {
        let mut integration = create();
        if updates.send_blocking(integration.poll()).is_err() {
            return;
        }
        let interval = integration.poll_interval();
        let mut next_poll = Instant::now() + interval;
        while !updates.is_closed() {
            let wake = future::block_on(future::or(
                async { Wake::Action(receiver.recv().await) },
                future::or(
                    async {
                        Timer::at(next_poll).await;
                        Wake::Poll
                    },
                    async {
                        integration.wait_for_event().await;
                        Wake::Event
                    },
                ),
            ));
            let event = match wake {
                Wake::Action(Ok(action)) => {
                    if let Err(error) = integration.handle(action, &updates)
                        && updates
                            .send_blocking(Update::ActionError(error.to_string()))
                            .is_err()
                    {
                        break;
                    }
                    false
                }
                Wake::Action(Err(_)) => break,
                Wake::Poll => false,
                Wake::Event => true,
            };
            let update = if event {
                integration.on_event()
            } else {
                Some(integration.poll())
            };
            let interval = integration.poll_interval();
            let deadline = Instant::now() + interval;
            next_poll = if event {
                next_poll.min(deadline)
            } else {
                deadline
            };
            if let Some(update) = update
                && updates.send_blocking(update).is_err()
            {
                break;
            }
        }
    })?;
    Ok(commands)
}
