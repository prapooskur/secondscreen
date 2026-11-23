use std::{
    process::{self, Command},
    thread,
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, de::DeserializeOwned};

use crate::platform::APP_ID;

use super::{Display, RETRY_INTERVAL, WINDOW_TIMEOUT};

#[derive(Deserialize)]
struct Output {
    name: String,
    make: String,
    model: String,
    rect: Size,
    active: bool,
}

#[derive(Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

pub(super) fn connected() -> Result<Vec<Display>> {
    let outputs: Vec<Output> = request("get_outputs", None)?;
    Ok(outputs
        .into_iter()
        .filter(|output| output.active)
        .map(|output| {
            Display::with_size(
                output.name,
                &output.make,
                &output.model,
                output.rect.width,
                output.rect.height,
            )
        })
        .collect())
}

pub(super) fn place_window(display: &str) -> Result<()> {
    let deadline = Instant::now() + WINDOW_TIMEOUT;
    loop {
        let tree: Node = request("get_tree", None)?;
        if let Some((id, output)) = tree.dashboard(process::id(), None) {
            if output != Some(display) {
                let display = display.replace('\\', "\\\\").replace('"', "\\\"");
                let command = format!("[con_id={id}] move container to output \"{display}\"");
                let replies: Vec<CommandReply> = request("command", Some(&command))?;
                ensure!(
                    !replies.is_empty() && replies.iter().all(|reply| reply.success),
                    "Sway declined the window move"
                );
            }
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Sway did not report the dashboard window"
        );
        thread::sleep(RETRY_INTERVAL);
    }
}

fn request<T: DeserializeOwned>(kind: &str, command: Option<&str>) -> Result<T> {
    let output = Command::new("swaymsg")
        .args(["-r", "-t", kind])
        .args(command)
        .output()
        .context("Could not run swaymsg")?;
    ensure!(
        output.status.success(),
        "Sway {kind} request failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    serde_json::from_slice(&output.stdout).context("Could not read Sway's response")
}

#[derive(Deserialize)]
struct CommandReply {
    success: bool,
}

#[derive(Deserialize)]
struct Node {
    id: u64,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    app_id: Option<String>,
    pid: Option<u32>,
    nodes: Vec<Node>,
    floating_nodes: Vec<Node>,
}

impl Node {
    fn dashboard<'a>(
        &'a self,
        pid: u32,
        output: Option<&'a str>,
    ) -> Option<(u64, Option<&'a str>)> {
        let output = if self.kind == "output" {
            self.name.as_deref()
        } else {
            output
        };
        if self.pid == Some(pid) && self.app_id.as_deref() == Some(APP_ID) {
            return Some((self.id, output));
        }
        self.nodes
            .iter()
            .chain(&self.floating_nodes)
            .find_map(|node| node.dashboard(pid, output))
    }
}
