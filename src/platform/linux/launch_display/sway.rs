use std::{
    process::{self, Command},
    thread,
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, de::DeserializeOwned};

use crate::platform::APP_ID;

use super::{RETRY_INTERVAL, WINDOW_TIMEOUT};

pub(super) fn place_window(display: &str, fullscreen: bool) -> Result<()> {
    let deadline = Instant::now() + WINDOW_TIMEOUT;
    loop {
        let tree: Node = request("get_tree", None)?;
        if let Some((window, output)) = tree.dashboard(process::id(), None) {
            let id = window.id;
            if output != Some(display) {
                let quoted = display.replace('\\', "\\\\").replace('"', "\\\"");
                command(&format!(
                    "[con_id={id}] move container to output \"{quoted}\""
                ))?;
                let tree: Node = request("get_tree", None)?;
                let (window, output) = tree
                    .dashboard(process::id(), None)
                    .context("Sway no longer reports the dashboard window")?;
                ensure!(output == Some(display), "Sway did not move the dashboard");
                return restore_fullscreen(window, fullscreen);
            }
            return restore_fullscreen(window, fullscreen);
        }
        ensure!(
            Instant::now() < deadline,
            "Sway did not report the dashboard window"
        );
        thread::sleep(RETRY_INTERVAL);
    }
}

fn restore_fullscreen(window: &Node, fullscreen: bool) -> Result<()> {
    if (window.fullscreen_mode != 0) != fullscreen {
        let mode = if fullscreen { "enable" } else { "disable" };
        command(&format!("[con_id={}] fullscreen {mode}", window.id))?;
    }
    Ok(())
}

fn command(command: &str) -> Result<()> {
    let replies: Vec<CommandReply> = request("command", Some(command))?;
    ensure!(
        !replies.is_empty() && replies.iter().all(|reply| reply.success),
        "Sway declined the dashboard command"
    );
    Ok(())
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
    #[serde(default)]
    fullscreen_mode: u8,
    nodes: Vec<Node>,
    floating_nodes: Vec<Node>,
}

impl Node {
    fn dashboard<'a>(
        &'a self,
        pid: u32,
        output: Option<&'a str>,
    ) -> Option<(&'a Self, Option<&'a str>)> {
        let output = if self.kind == "output" {
            self.name.as_deref()
        } else {
            output
        };
        if self.pid == Some(pid) && self.app_id.as_deref() == Some(APP_ID) {
            return Some((self, output));
        }
        self.nodes
            .iter()
            .chain(&self.floating_nodes)
            .find_map(|node| node.dashboard(pid, output))
    }
}
