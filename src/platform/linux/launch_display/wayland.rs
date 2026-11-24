use std::collections::BTreeMap;

use anyhow::{Context, Result};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    globals::{GlobalList, GlobalListContents, registry_queue_init},
    protocol::{wl_output, wl_registry},
};

use super::Display;

pub struct Displays {
    globals: GlobalList,
    queue: EventQueue<Outputs>,
    outputs: Outputs,
}

impl Displays {
    pub fn open() -> Result<Self> {
        let connection =
            Connection::connect_to_env().context("Could not connect to the Wayland display")?;
        let (globals, queue) = registry_queue_init::<Outputs>(&connection)?;
        Ok(Self {
            globals,
            queue,
            outputs: Outputs::default(),
        })
    }

    pub fn scan(&mut self) -> Result<Vec<Display>> {
        self.queue.roundtrip(&mut self.outputs)?;
        let names: Vec<_> = self.globals.contents().with_list(|globals| {
            globals
                .iter()
                .filter(|global| {
                    global.interface == wl_output::WlOutput::interface().name && global.version >= 4
                })
                .map(|global| global.name)
                .collect()
        });
        self.outputs.0.retain(|id, (proxy, _)| {
            let present = names.contains(id);
            if !present {
                proxy.release();
            }
            present
        });
        let handle = self.queue.handle();
        let mut bound = false;
        for id in names {
            self.outputs.0.entry(id).or_insert_with(|| {
                bound = true;
                let proxy = self.globals.registry().bind(id, 4, &handle, id);
                (proxy, Output::default())
            });
        }
        if bound {
            self.queue.roundtrip(&mut self.outputs)?;
        }
        let mut displays: Vec<_> = self.globals.contents().with_list(|globals| {
            self.outputs
                .0
                .iter()
                .filter_map(|(&id, (_, output))| {
                    let name = output.name.as_ref()?;
                    globals.iter().any(|global| global.name == id).then(|| {
                        Display::with_size(
                            id,
                            name.clone(),
                            &output.make,
                            &output.model,
                            output.width,
                            output.height,
                        )
                    })
                })
                .collect()
        });
        displays.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(displays)
    }
}

#[derive(Default)]
struct Outputs(BTreeMap<u32, (wl_output::WlOutput, Output)>);

#[derive(Default)]
struct Output {
    name: Option<String>,
    make: String,
    model: String,
    width: u32,
    height: u32,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Outputs {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Outputs {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, output)) = state.0.get_mut(id) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Geometry { make, model, .. } => {
                output.make = make;
                output.model = model;
            }
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) && width > 0 && height > 0 => {
                output.width = width as u32;
                output.height = height as u32;
            }
            _ => {}
        }
    }
}
