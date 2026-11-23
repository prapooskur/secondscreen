use anyhow::{Context, Result};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_output, wl_registry},
};

use super::Display;

pub(super) fn connected() -> Result<Vec<Display>> {
    let connection =
        Connection::connect_to_env().context("Could not connect to the Wayland display")?;
    let (globals, mut queue) = registry_queue_init::<Outputs>(&connection)?;
    let handle = queue.handle();
    let mut outputs = Outputs::default();
    for global in globals.contents().clone_list() {
        if global.interface == wl_output::WlOutput::interface().name && global.version >= 4 {
            globals.registry().bind::<wl_output::WlOutput, _, _>(
                global.name,
                4,
                &handle,
                outputs.0.len(),
            );
            outputs.0.push(Output::default());
        }
    }
    queue.roundtrip(&mut outputs)?;
    Ok(outputs
        .0
        .into_iter()
        .filter_map(|output| {
            let name = output.name?;
            Some(Display::with_size(
                name,
                &output.make,
                &output.model,
                output.width,
                output.height,
            ))
        })
        .collect())
}

#[derive(Default)]
struct Outputs(Vec<Output>);

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

impl Dispatch<wl_output::WlOutput, usize> for Outputs {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let output = &mut state.0[*index];
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
