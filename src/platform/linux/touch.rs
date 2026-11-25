use std::{collections::HashMap, net::Shutdown, os::unix::net::UnixStream, thread};

use anyhow::{Context, Result, bail};
use async_channel::{Receiver, Sender};
use gpui::Window;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use rustix::{
    event::{PollFd, PollFlags, poll},
    io::Errno,
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::{Backend, WaylandError},
    protocol::{wl_registry, wl_seat, wl_touch},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    pub seat: u32,
    id: i32,
}

pub enum Event {
    Down(Contact, f32, f32),
    Motion(Contact, f32, f32),
    Up(Contact),
    Cancel(u32),
}

pub struct TouchSource {
    stop: UnixStream,
    worker: Option<thread::JoinHandle<()>>,
}

impl TouchSource {
    pub fn start(window: &Window) -> Result<(Self, Receiver<Event>)> {
        let RawDisplayHandle::Wayland(display) = window.display_handle()?.as_raw() else {
            bail!("No Wayland display for touch input");
        };
        let RawWindowHandle::Wayland(surface) = HasWindowHandle::window_handle(window)?.as_raw()
        else {
            bail!("No Wayland surface for touch input");
        };
        let (stop, stopped) = UnixStream::pair()?;
        let (send, receive) = async_channel::unbounded();
        let surface = surface.surface.as_ptr() as usize;
        // GPUI owns the display. The dashboard joins this worker when its window closes.
        let connection = Connection::from_backend(unsafe {
            Backend::from_foreign_display(display.display.as_ptr().cast())
        });
        let worker = thread::Builder::new()
            .name("wayland-touch".into())
            .spawn(move || {
                if let Err(error) = run(connection, surface, stopped, send) {
                    log::debug!("Wayland touch input stopped: {error:#}");
                }
            })?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            receive,
        ))
    }
}

impl Drop for TouchSource {
    fn drop(&mut self) {
        let _ = self.stop.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct State {
    surface: usize,
    send: Sender<Event>,
    seats: HashMap<u32, (wl_seat::WlSeat, Option<wl_touch::WlTouch>)>,
}

fn run(
    connection: Connection,
    surface: usize,
    stopped: UnixStream,
    send: Sender<Event>,
) -> Result<()> {
    let mut queue = connection.new_event_queue();
    let registry = connection.display().get_registry(&queue.handle(), ());
    let mut state = State {
        surface,
        send,
        seats: HashMap::new(),
    };
    let result = (|| {
        loop {
            queue.dispatch_pending(&mut state)?;
            // Coordinate reads with GPUI's event loop. Each reader dispatches its own queue.
            let Some(read) = queue.prepare_read() else {
                continue;
            };
            let mut interest = PollFlags::IN;
            match queue.flush() {
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    interest |= PollFlags::OUT;
                }
                result => result?,
            }
            let mut fds = [
                PollFd::new(&stopped, PollFlags::IN),
                PollFd::new(&connection, interest),
            ];
            match poll(&mut fds, None) {
                Err(Errno::INTR) => continue,
                result => {
                    result.context("Could not wait for Wayland input")?;
                }
            }
            if !fds[0].revents().is_empty() {
                return Ok(());
            }
            if fds[1]
                .revents()
                .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
            {
                match read.read() {
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    result => {
                        result?;
                    }
                }
            }
        }
    })();
    for (name, (seat, touch)) in state.seats {
        if let Some(touch) = touch {
            touch.release();
            let _ = state.send.try_send(Event::Cancel(name));
        }
        seat.release();
    }
    let _ = connection.backend().destroy_object(&registry.id());
    let _ = connection.flush();
    result
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == wl_seat::WlSeat::interface().name && version >= 5 => {
                let seat = registry.bind(name, 5, queue, name);
                state.seats.insert(name, (seat, None));
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some((seat, touch)) = state.seats.remove(&name) {
                    if let Some(touch) = touch {
                        touch.release();
                        let _ = state.send.try_send(Event::Cancel(name));
                    }
                    seat.release();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, u32> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        name: &u32,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
            && let Some((_, touch)) = state.seats.get_mut(name)
        {
            if capabilities.contains(wl_seat::Capability::Touch) {
                if touch.is_none() {
                    *touch = Some(seat.get_touch(queue, *name));
                }
            } else if let Some(touch) = touch.take() {
                touch.release();
                let _ = state.send.try_send(Event::Cancel(*name));
            }
        }
    }
}

impl Dispatch<wl_touch::WlTouch, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        seat: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let contact = |id| Contact { seat: *seat, id };
        let event = match event {
            wl_touch::Event::Down {
                surface, id, x, y, ..
            } if surface.id().as_ptr() as usize == state.surface => {
                Event::Down(contact(id), x as f32, y as f32)
            }
            wl_touch::Event::Motion { id, x, y, .. } => {
                Event::Motion(contact(id), x as f32, y as f32)
            }
            wl_touch::Event::Up { id, .. } => Event::Up(contact(id)),
            wl_touch::Event::Cancel => Event::Cancel(*seat),
            _ => return,
        };
        let _ = state.send.try_send(event);
    }
}
