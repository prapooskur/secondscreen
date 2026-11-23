mod app_icons;
mod audio;
pub mod autostart;
mod discord;
pub mod launch_display;
mod media;

use async_channel::{RecvError, Sender};
use async_io::Timer;
use futures_lite::future;
use std::{
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
    let audio = worker("audio-mixer", updates.clone(), audio::Mixer::default)?;
    let media = worker("media-player", updates.clone(), media::Media::default)?;
    let discord = worker("discord-rpc", updates, move || {
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
