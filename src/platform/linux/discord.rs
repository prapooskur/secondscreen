mod auth;

use anyhow::{Context, Result, bail, ensure};
use async_channel::Sender;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    env,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::Integration;
use crate::{
    config::{self, DiscordCredentials as Credentials},
    model::{DiscordAction, DiscordState, Update, VoiceChannel, VoiceMember, VoiceState},
};

const RPC_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const EVENT_INTERVAL: Duration = Duration::from_millis(50);
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const CHANNEL_EVENTS: &[&str] = &[
    "SPEAKING_START",
    "SPEAKING_STOP",
    "VOICE_STATE_CREATE",
    "VOICE_STATE_UPDATE",
    "VOICE_STATE_DELETE",
];
const CDN_URL: &str = "https://cdn.discordapp.com";

pub(super) struct Discord {
    credentials: Option<Credentials>,
    auth: auth::Authorization,
    resume_at: Option<Instant>,
    session: Option<Session>,
    error: Option<String>,
}

struct Session {
    rpc: Rpc,
    voice: VoiceState,
    last_poll_succeeded: bool,
    subscriptions: Vec<&'static str>,
}

impl Discord {
    pub fn new(credentials: Option<Credentials>) -> Self {
        Self {
            credentials: effective_credentials(credentials),
            auth: auth::Authorization::new(config::path("discord-session.lock")),
            resume_at: Some(Instant::now()),
            session: None,
            error: None,
        }
    }

    fn connect(&mut self, interactive: Option<&Sender<Update>>) -> Result<()> {
        let credentials = self
            .credentials
            .as_ref()
            .context("Configure Discord RPC before connecting.")?;
        let paths = Rpc::socket_paths();
        if interactive.is_none() && !paths.iter().any(|path| path.exists()) {
            return Ok(());
        }
        let mut login = self.auth.login(credentials, interactive.is_some())?;
        if interactive.is_none() && !login.can_resume() {
            self.resume_at = None;
            return Ok(());
        }

        let Some(rpc) = Rpc::connect_paths(&credentials.client_id, &paths, HANDSHAKE_TIMEOUT)?
        else {
            if interactive.is_some() {
                bail!(
                    "Discord desktop is not listening for local RPC. Open Discord and try again."
                );
            }
            return Ok(());
        };

        self.resume_at = None;
        self.session = Some(login.authenticate(rpc, auth::TOKEN_URL, interactive)?);
        self.error = None;
        Ok(())
    }

    fn configure(&mut self, credentials: Option<Credentials>) {
        let next = effective_credentials(credentials);
        if self.credentials != next {
            if let Some(old) = &self.credentials {
                self.auth.clear(old);
            }
            self.credentials = next;
            self.session = None;
            self.error = None;
            self.resume_at = Some(Instant::now());
        }
    }

    fn disconnect(&mut self, error: anyhow::Error) -> DiscordState {
        self.session = None;
        self.resume_at = transport_failure(&error).then(|| Instant::now() + RECONNECT_DELAY);
        let message = error.to_string();
        self.error = Some(message.clone());
        DiscordState::Error(message)
    }
}

fn effective_credentials(saved: Option<Credentials>) -> Option<Credentials> {
    saved
        .or_else(Credentials::from_env)
        .filter(Credentials::valid)
}

fn avatar_url(user: &Value) -> Option<String> {
    let id: u64 = user["id"].as_str()?.parse().ok()?;
    if let Some(avatar) = user["avatar"].as_str().filter(|avatar| {
        let hash = avatar.strip_prefix("a_").unwrap_or(avatar);
        hash.len() == 32 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Some(format!("{CDN_URL}/avatars/{id}/{avatar}.png?size=64"));
    }

    let discriminator = user["discriminator"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let index = if discriminator == 0 {
        (id >> 22) % 6
    } else {
        discriminator % 5
    };
    Some(format!("{CDN_URL}/embed/avatars/{index}.png"))
}

fn member_id(member: &Value) -> Option<&str> {
    member["user"]["id"]
        .as_str()
        .or_else(|| member["user_id"].as_str())
}

fn member_streaming(member: &Value) -> Option<bool> {
    member["voice_state"]["self_stream"]
        .as_bool()
        .or_else(|| member["self_stream"].as_bool())
}

fn voice_member(member: &Value) -> VoiceMember {
    let state = &member["voice_state"];
    let flag = |name: &str| state[name].as_bool().unwrap_or(false);
    VoiceMember {
        id: member_id(member).map(str::to_owned),
        name: member["nick"]
            .as_str()
            .or_else(|| member["user"]["global_name"].as_str())
            .or_else(|| member["user"]["username"].as_str())
            .unwrap_or("Discord user")
            .to_owned(),
        avatar_url: avatar_url(&member["user"]),
        muted: flag("self_mute") || flag("mute"),
        deafened: flag("self_deaf") || flag("deaf"),
        speaking: false,
        streaming: member_streaming(member).unwrap_or(false),
    }
}

impl Session {
    fn new(rpc: Rpc, account: String) -> Self {
        Self {
            rpc,
            voice: VoiceState {
                account,
                muted: false,
                deafened: false,
                channel: None,
            },
            last_poll_succeeded: false,
            subscriptions: Vec::new(),
        }
    }

    fn voice_settings(&mut self) -> Result<(bool, bool)> {
        let data = self
            .rpc
            .call("GET_VOICE_SETTINGS", json!({}), RPC_TIMEOUT)?;
        let mute = data["mute"]
            .as_bool()
            .context("Discord did not report microphone mute.")?;
        let deaf = data["deaf"]
            .as_bool()
            .context("Discord did not report deafen state.")?;
        Ok((mute, deaf))
    }

    fn poll(&mut self) -> Result<VoiceState> {
        let result = self.poll_snapshot();
        let previously_current = std::mem::replace(&mut self.last_poll_succeeded, result.is_ok());
        match result {
            Err(error)
                if previously_current && io_kind(&error) == Some(io::ErrorKind::TimedOut) =>
            {
                log::debug!("Using the last Discord voice snapshot: {error:#}");
                self.apply_events();
                Ok(self.voice.clone())
            }
            result => result,
        }
    }

    fn poll_snapshot(&mut self) -> Result<VoiceState> {
        let (muted, deafened) = self.voice_settings()?;
        let selected = self
            .rpc
            .call("GET_SELECTED_VOICE_CHANNEL", json!({}), RPC_TIMEOUT)?;

        let previous_id = self
            .voice
            .channel
            .as_ref()
            .map(|channel| channel.id.as_str());
        let next_id = selected["id"].as_str();
        let switching = previous_id != next_id;
        if switching {
            if let Some(id) = previous_id {
                let args = json!({ "channel_id": id });
                for event in self.subscriptions.drain(..) {
                    if let Err(error) = self.rpc.subscription("UNSUBSCRIBE", event, &args) {
                        log::debug!("Could not unsubscribe from Discord {event}: {error:#}");
                    }
                }
            }
            self.rpc.events.clear();
        } else {
            self.apply_events();
        }

        let previous = self.voice.channel.as_ref().filter(|_| !switching);
        let subscribed_to_voice = self.subscriptions.contains(&"VOICE_STATE_UPDATE");
        self.voice.muted = muted;
        self.voice.deafened = deafened;
        self.voice.channel = next_id.map(|id| {
            let members = selected["voice_states"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|data| {
                    let mut member = voice_member(data);
                    if let Some(old) = member.id.as_ref().and_then(|id| {
                        previous?
                            .members
                            .iter()
                            .find(|member| member.id.as_ref() == Some(id))
                    }) {
                        member.speaking = old.speaking;
                        // Some desktop clients omit self_stream in channel snapshots.
                        if subscribed_to_voice && member_streaming(data).is_none() {
                            member.streaming = old.streaming;
                        }
                    }
                    member
                })
                .collect();
            VoiceChannel {
                id: id.to_owned(),
                name: selected["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Voice call")
                    .to_owned(),
                members,
            }
        });
        if switching && let Some(id) = next_id {
            let args = json!({ "channel_id": id });
            for &event in CHANNEL_EVENTS {
                match self.rpc.subscription("SUBSCRIBE", event, &args) {
                    Ok(()) => self.subscriptions.push(event),
                    Err(error) => log::debug!("Could not subscribe to Discord {event}: {error:#}"),
                }
            }
            self.apply_events();
        }
        Ok(self.voice.clone())
    }

    fn poll_events(&mut self) -> Result<Option<VoiceState>> {
        self.rpc.drain_events()?;
        Ok(self.apply_events().then(|| self.voice.clone()))
    }

    fn apply_events(&mut self) -> bool {
        let mut changed = false;
        for event in std::mem::take(&mut self.rpc.events) {
            if let Some(channel) = &mut self.voice.channel {
                changed |= channel.apply_event(&event);
            }
        }
        changed
    }
}

impl VoiceChannel {
    fn apply_event(&mut self, event: &Value) -> bool {
        let data = &event["data"];
        if data["channel_id"]
            .as_str()
            .or_else(|| data["voice_state"]["channel_id"].as_str())
            .is_some_and(|id| id != self.id)
        {
            return false;
        }
        let Some(id) = member_id(data) else {
            return false;
        };
        let index = self
            .members
            .iter()
            .position(|member| member.id.as_deref() == Some(id));
        match (event["evt"].as_str(), index) {
            (Some("SPEAKING_START" | "SPEAKING_STOP"), Some(index)) => {
                let speaking = event["evt"] == "SPEAKING_START";
                let member = &mut self.members[index];
                std::mem::replace(&mut member.speaking, speaking) != speaking
            }
            (Some("VOICE_STATE_UPDATE"), Some(index)) => {
                // A full voice state with no Go Live flag means the share stopped.
                let streaming = member_streaming(data)
                    .or_else(|| data["voice_state"].is_object().then_some(false));
                streaming.is_some_and(|streaming| {
                    let member = &mut self.members[index];
                    std::mem::replace(&mut member.streaming, streaming) != streaming
                })
            }
            (Some("VOICE_STATE_CREATE"), None) => {
                self.members.push(voice_member(data));
                true
            }
            (Some("VOICE_STATE_DELETE"), Some(index)) => {
                self.members.remove(index);
                true
            }
            _ => false,
        }
    }
}

impl Integration for Discord {
    type Action = DiscordAction;

    fn poll(&mut self) -> Update {
        if self.resume_at.is_some_and(|at| Instant::now() >= at)
            && self.session.is_none()
            && self.credentials.is_some()
            && let Err(error) = self.connect(None)
        {
            self.disconnect(error);
        }
        let state = match self.session.as_mut().map(Session::poll) {
            Some(Ok(voice)) => DiscordState::Connected(voice),
            Some(Err(error)) => self.disconnect(error),
            None if self.credentials.is_none() => DiscordState::Unconfigured,
            None => self
                .error
                .clone()
                .map_or(DiscordState::Disconnected, DiscordState::Error),
        };
        Update::Discord(state)
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs(2)
    }

    async fn wait_for_event(&mut self) {
        if self
            .session
            .as_ref()
            .is_some_and(|session| !session.subscriptions.is_empty())
        {
            async_io::Timer::after(EVENT_INTERVAL).await;
        } else {
            futures_lite::future::pending().await
        }
    }

    fn on_event(&mut self) -> Option<Update> {
        match self.session.as_mut()?.poll_events() {
            Ok(voice) => voice.map(|voice| Update::Discord(DiscordState::Connected(voice))),
            Err(error) => Some(Update::Discord(self.disconnect(error))),
        }
    }

    fn handle(&mut self, action: DiscordAction, updates: &Sender<Update>) -> Result<()> {
        match action {
            DiscordAction::Configure(credentials) => self.configure(credentials),
            DiscordAction::Connect => {
                if self.session.is_none() {
                    if let Err(error) = self.connect(Some(updates)) {
                        self.disconnect(error);
                    }
                }
            }
            DiscordAction::ToggleMute | DiscordAction::ToggleDeafen | DiscordAction::Leave => {
                let session = self
                    .session
                    .as_mut()
                    .context("Connect to Discord before using voice controls.")?;
                if matches!(action, DiscordAction::Leave) {
                    session.rpc.call(
                        "SELECT_VOICE_CHANNEL",
                        json!({ "channel_id": null }),
                        RPC_TIMEOUT,
                    )?;
                } else {
                    let (muted, deafened) = session.voice_settings()?;
                    let args = if matches!(action, DiscordAction::ToggleMute) {
                        json!({ "mute": !muted })
                    } else {
                        json!({ "deaf": !deafened })
                    };
                    session.rpc.call("SET_VOICE_SETTINGS", args, RPC_TIMEOUT)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct RpcError {
    code: Option<i64>,
    message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Discord RPC: {}", self.message)
    }
}

impl std::error::Error for RpcError {}

fn transport_failure(error: &anyhow::Error) -> bool {
    matches!(
        io_kind(error),
        Some(
            io::ErrorKind::TimedOut
                | io::ErrorKind::WouldBlock
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::UnexpectedEof
        )
    )
}

fn io_kind(error: &anyhow::Error) -> Option<io::ErrorKind> {
    error.downcast_ref::<io::Error>().map(io::Error::kind)
}

fn rpc_step(error: anyhow::Error, operation: &str) -> anyhow::Error {
    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        let message = match io_error.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                format!("Discord timed out during {operation}.")
            }
            io::ErrorKind::ConnectionAborted if io_error.get_ref().is_some() => {
                format!("Discord closed the RPC connection during {operation}: {io_error}")
            }
            io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof => {
                format!("Discord closed the RPC connection during {operation}.")
            }
            _ => format!("Discord IPC failed during {operation}: {io_error}."),
        };
        return error.context(message);
    }
    error
}

struct Rpc {
    socket: UnixStream,
    nonce: u64,
    incoming: Vec<u8>,
    events: Vec<Value>,
}

impl Rpc {
    fn new(socket: UnixStream) -> Self {
        Self {
            socket,
            nonce: 0,
            incoming: Vec::new(),
            events: Vec::new(),
        }
    }

    fn socket_paths() -> Vec<PathBuf> {
        let mut directories: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
            .into_iter()
            .filter_map(env::var_os)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
            for client in [
                "com.discordapp.Discord",
                "com.discordapp.DiscordCanary",
                "com.discordapp.DiscordPTB",
                "dev.vencord.Vesktop",
            ] {
                directories.push(PathBuf::from(&runtime).join("app").join(client));
            }
            directories.push(PathBuf::from(runtime).join("snap.discord"));
        }
        directories.push(PathBuf::from("/tmp"));

        let mut seen = HashSet::new();
        directories
            .into_iter()
            .filter(|dir| seen.insert(dir.clone()))
            .flat_map(|path| (0..10).map(move |n| path.join(format!("discord-ipc-{n}"))))
            .collect()
    }

    fn connect_paths(
        client_id: &str,
        paths: &[PathBuf],
        timeout: Duration,
    ) -> Result<Option<Self>> {
        let mut failure = None;
        let mut seen = HashSet::new();
        for path in paths {
            if let Ok(resolved) = path.canonicalize()
                && !seen.insert(resolved)
            {
                continue;
            }
            let socket = match UnixStream::connect(path) {
                Ok(socket) => socket,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    continue;
                }
                Err(error) => {
                    let error = anyhow::anyhow!(
                        "Could not open Discord IPC at {}: {error}",
                        path.display()
                    );
                    failure.get_or_insert(error);
                    continue;
                }
            };
            match Self::handshake(socket, client_id, timeout) {
                Ok(rpc) => return Ok(Some(rpc)),
                Err(error) => {
                    log::debug!("Discord handshake failed at {}: {error:#}", path.display());
                    failure.get_or_insert_with(|| rpc_step(error, "the READY handshake"));
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }

    fn handshake(socket: UnixStream, client_id: &str, timeout: Duration) -> Result<Self> {
        socket.set_write_timeout(Some(RPC_TIMEOUT))?;
        let mut rpc = Self::new(socket);
        rpc.send(
            0,
            &serde_json::to_vec(&json!({ "v": 1, "client_id": client_id }))?,
        )?;
        let ready = rpc.receive(Instant::now() + timeout)?;
        if ready["evt"] == "ERROR"
            && let Some(message) = ready["data"]["message"].as_str()
        {
            bail!("Discord rejected the RPC handshake: {message}");
        }
        ensure!(
            ready["evt"] == "READY",
            "Discord did not accept the RPC handshake."
        );
        Ok(rpc)
    }

    fn call(&mut self, command: &str, args: Value, timeout: Duration) -> Result<Value> {
        self.request(command, args, None, timeout)
    }

    fn subscription(&mut self, command: &str, event: &str, args: &Value) -> Result<()> {
        self.request(command, args.clone(), Some(event), SUBSCRIPTION_TIMEOUT)
            .map(|_| ())
    }

    fn request(
        &mut self,
        command: &str,
        args: Value,
        event: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let mut payload = json!({ "cmd": command, "args": args, "nonce": nonce });
        if let Some(event) = event {
            payload["evt"] = json!(event);
        }
        let operation =
            event.map_or_else(|| command.to_owned(), |event| format!("{command} {event}"));
        self.send(1, &serde_json::to_vec(&payload)?)
            .map_err(|error| rpc_step(error, &operation))?;
        let deadline = Instant::now() + timeout;
        loop {
            let response = self
                .receive(deadline)
                .map_err(|error| rpc_step(error, &operation))?;
            let command_error = response["evt"] == "ERROR"
                && response["cmd"] == command
                && response["nonce"].is_null();
            if response["nonce"].as_str() != Some(&nonce) && !command_error {
                self.queue_event(response);
                continue;
            }
            if response["evt"] == "ERROR" {
                let message = response["data"]["message"]
                    .as_str()
                    .unwrap_or("The command failed.");
                return Err(RpcError {
                    code: response["data"]["code"].as_i64(),
                    message: message.to_owned(),
                }
                .into());
            }
            ensure!(
                response["cmd"] == command,
                "Discord returned an unexpected RPC reply."
            );
            return Ok(response["data"].clone());
        }
    }

    fn queue_event(&mut self, message: Value) {
        if message["cmd"] == "DISPATCH"
            && message["evt"]
                .as_str()
                .is_some_and(|event| CHANNEL_EVENTS.contains(&event))
        {
            self.events.push(message);
        }
    }

    fn drain_events(&mut self) -> Result<()> {
        for _ in 0..64 {
            let Some(message) = self
                .receive_frame(None)
                .map_err(|error| rpc_step(error, "voice event delivery"))?
            else {
                break;
            };
            self.queue_event(message);
        }
        Ok(())
    }

    fn send(&mut self, opcode: u32, body: &[u8]) -> Result<()> {
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(&opcode.to_le_bytes());
        header[4..].copy_from_slice(&u32::try_from(body.len())?.to_le_bytes());
        self.socket
            .write_all(&header)
            .context("Could not write to Discord desktop.")?;
        self.socket
            .write_all(body)
            .context("Could not write to Discord desktop.")?;
        Ok(())
    }

    fn receive(&mut self, deadline: Instant) -> Result<Value> {
        self.receive_frame(Some(deadline))?
            .context("Missing Discord RPC frame.")
    }

    fn receive_frame(&mut self, deadline: Option<Instant>) -> Result<Option<Value>> {
        while let Some((opcode, payload)) = self.read_packet(deadline)? {
            match opcode {
                1 => {
                    return serde_json::from_slice(&payload)
                        .map(Some)
                        .context("Discord sent invalid RPC data.");
                }
                2 => {
                    let message = serde_json::from_slice::<Value>(&payload)
                        .ok()
                        .and_then(|value| value["message"].as_str().map(str::to_owned));
                    let error = message.map_or_else(
                        || io::Error::from(io::ErrorKind::ConnectionAborted),
                        |message| io::Error::new(io::ErrorKind::ConnectionAborted, message),
                    );
                    return Err(error.into());
                }
                3 => self.send(4, &payload)?,
                4 => {}
                _ => bail!("Discord sent an unknown RPC frame."),
            }
        }
        Ok(None)
    }

    fn read_packet(&mut self, deadline: Option<Instant>) -> Result<Option<(u32, Vec<u8>)>> {
        loop {
            let timeout = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::Error::from(io::ErrorKind::TimedOut).into());
                    }
                    Timespec {
                        tv_sec: remaining.as_secs() as _,
                        tv_nsec: remaining.subsec_nanos() as _,
                    }
                }
                None => Timespec::default(),
            };
            if let Some(header) = self.incoming.get(..8) {
                let size = u32::from_le_bytes(header[4..].try_into()?) as usize;
                ensure!(
                    size <= MAX_FRAME_BYTES,
                    "Discord sent an oversized RPC frame."
                );
                if let Some(payload) = self.incoming.get(8..8 + size) {
                    let opcode = u32::from_le_bytes(header[..4].try_into()?);
                    let payload = payload.to_vec();
                    self.incoming.drain(..8 + size);
                    return Ok(Some((opcode, payload)));
                }
            }

            let mut fd = [PollFd::new(&self.socket, PollFlags::IN)];
            match poll(&mut fd, Some(&timeout)) {
                Ok(0) if deadline.is_some() => {
                    return Err(io::Error::from(io::ErrorKind::TimedOut).into());
                }
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io::Error::from(error).into()),
            }
            // Only one worker reads this socket. Keep fragments between events and command deadlines.
            let mut bytes = [0u8; 8192];
            match self.socket.read(&mut bytes) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                Ok(count) => self.incoming.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::{BufRead, BufReader},
        net::TcpListener,
        os::unix::net::UnixListener,
        sync::mpsc,
        thread,
    };

    fn assert_rpc(rpc: &mut Rpc, command: &str, args: Value) -> Value {
        let request = rpc.receive(Instant::now() + RPC_TIMEOUT).unwrap();
        assert_eq!(request["cmd"], command);
        assert_eq!(request["args"], args);
        request
    }

    fn send(rpc: &mut Rpc, value: Value) {
        rpc.send(1, &serde_json::to_vec(&value).unwrap()).unwrap();
    }

    #[test]
    fn a_late_fragmented_reply_does_not_corrupt_the_next_rpc_call() {
        let (client, server) = UnixStream::pair().unwrap();
        let (continue_tx, continue_rx) = mpsc::channel();
        let ipc = thread::spawn(move || {
            let mut rpc = Rpc::new(server);
            let first = assert_rpc(&mut rpc, "GET_VOICE_SETTINGS", json!({}));
            let late = serde_json::to_vec(&json!({
                "cmd": first["cmd"], "nonce": first["nonce"], "data": { "mute": false }
            }))
            .unwrap();
            let header = [1u32.to_le_bytes(), (late.len() as u32).to_le_bytes()].concat();
            rpc.socket.write_all(&header[..5]).unwrap();
            continue_rx.recv_timeout(RPC_TIMEOUT).unwrap();
            rpc.socket.write_all(&header[5..]).unwrap();
            rpc.socket.write_all(&late).unwrap();

            let second = assert_rpc(&mut rpc, "GET_VOICE_SETTINGS", json!({}));
            send(
                &mut rpc,
                json!({
                    "cmd": second["cmd"], "nonce": second["nonce"], "data": { "mute": true }
                }),
            );
        });
        let mut rpc = Rpc::new(client);
        let error = rpc
            .call("GET_VOICE_SETTINGS", json!({}), Duration::from_millis(250))
            .unwrap_err();
        assert_eq!(io_kind(&error), Some(io::ErrorKind::TimedOut));
        assert!(error.to_string().contains("GET_VOICE_SETTINGS"));
        assert_eq!(rpc.incoming.len(), 5);
        rpc.drain_events().unwrap();
        assert_eq!(rpc.incoming.len(), 5);

        continue_tx.send(()).unwrap();
        let data = rpc
            .call("GET_VOICE_SETTINGS", json!({}), RPC_TIMEOUT)
            .unwrap();
        assert_eq!(data["mute"], true);
        ipc.join().unwrap();
        let error = rpc
            .call("GET_SELECTED_VOICE_CHANNEL", json!({}), RPC_TIMEOUT)
            .unwrap_err();
        assert!(transport_failure(&error));
        assert!(
            error
                .to_string()
                .contains("closed the RPC connection during GET_SELECTED_VOICE_CHANNEL")
        );
    }

    #[test]
    fn a_silent_socket_does_not_mask_a_desktop_that_sends_ready() {
        let root = std::env::temp_dir().join(format!("secondscreen-rpc-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let paths = [root.join("discord-ipc-0"), root.join("discord-ipc-1")];
        let silent = UnixListener::bind(&paths[0]).unwrap();
        let ready = UnixListener::bind(&paths[1]).unwrap();
        let ipc = thread::spawn(move || {
            for (listener, responding) in [(silent, false), (ready, true)] {
                let mut rpc = Rpc::new(listener.accept().unwrap().0);
                let (opcode, data) = rpc
                    .read_packet(Some(Instant::now() + RPC_TIMEOUT))
                    .unwrap()
                    .unwrap();
                assert_eq!(opcode, 0);
                assert_eq!(
                    serde_json::from_slice::<Value>(&data).unwrap()["client_id"],
                    "123456"
                );
                if responding {
                    send(&mut rpc, json!({ "cmd": "DISPATCH", "evt": "READY" }));
                } else {
                    assert_eq!(rpc.socket.read(&mut [0u8]).unwrap(), 0);
                }
            }
        });
        assert!(
            Rpc::connect_paths("123456", &paths, Duration::from_millis(250))
                .unwrap()
                .is_some()
        );
        ipc.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tracks_voice_activity_across_rpc_replies_and_channel_changes() {
        let (client, server) = UnixStream::pair().unwrap();
        let mut dispatch = Rpc::new(server.try_clone().unwrap());
        let ipc = thread::spawn(move || {
            let mut rpc = Rpc::new(server);
            let reply = |rpc: &mut Rpc, request: Value, data: Value| {
                send(
                    rpc,
                    json!({ "cmd": request["cmd"], "nonce": request["nonce"], "data": data }),
                );
            };
            for (index, channel_id) in ["40", "40", "50"].into_iter().enumerate() {
                let request = assert_rpc(&mut rpc, "GET_VOICE_SETTINGS", json!({}));
                reply(&mut rpc, request, json!({ "mute": false, "deaf": false }));
                let request = assert_rpc(&mut rpc, "GET_SELECTED_VOICE_CHANNEL", json!({}));
                if index == 1 {
                    send(
                        &mut rpc,
                        json!({
                            "cmd": "DISPATCH", "evt": "SPEAKING_STOP", "data": { "user_id": "1" }
                        }),
                    );
                    send(
                        &mut rpc,
                        json!({
                            "cmd": "DISPATCH", "evt": "VOICE_STATE_UPDATE",
                            "data": { "user": { "id": "2" }, "voice_state": { "self_stream": true } }
                        }),
                    );
                }
                reply(
                    &mut rpc,
                    request,
                    json!({
                        "id": channel_id,
                        "voice_states": [
                            { "nick": "Alex", "user": { "id": "1" }, "voice_state": { "self_stream": index == 0 } },
                            { "nick": "Alex", "user": { "id": "2" }, "voice_state": { "self_video": true } },
                        ]
                    }),
                );
                if index == 2 {
                    for &event in CHANNEL_EVENTS {
                        let request =
                            assert_rpc(&mut rpc, "UNSUBSCRIBE", json!({ "channel_id": "40" }));
                        assert_eq!(request["evt"], event);
                        reply(&mut rpc, request, json!({ "evt": event }));
                    }
                }
                if index != 1 {
                    for &event in CHANNEL_EVENTS {
                        let request =
                            assert_rpc(&mut rpc, "SUBSCRIBE", json!({ "channel_id": channel_id }));
                        assert_eq!(request["evt"], event);
                        if index == 0 && event == "SPEAKING_STOP" {
                            send(
                                &mut rpc,
                                json!({
                                    "cmd": "DISPATCH", "evt": "SPEAKING_START", "data": { "user_id": "1" }
                                }),
                            );
                        }
                        if index == 2 && event == "VOICE_STATE_UPDATE" {
                            send(
                                &mut rpc,
                                json!({
                                    "cmd": "SUBSCRIBE", "evt": "ERROR", "nonce": request["nonce"],
                                    "data": { "message": "Event unavailable" }
                                }),
                            );
                        } else {
                            reply(&mut rpc, request, json!({ "evt": event }));
                        }
                    }
                }
            }
        });
        let mut session = Session::new(Rpc::new(client), "Local user".into());
        let members = session.poll().unwrap().channel.unwrap().members;
        assert!(members[0].speaking && members[0].streaming);
        assert!(!members[1].speaking && !members[1].streaming);
        assert!(session.poll_events().unwrap().is_none());

        let members = session.poll().unwrap().channel.unwrap().members;
        assert!(!members[0].speaking && !members[0].streaming);
        assert!(members[1].streaming && !members[1].speaking);
        for event in [
            json!({ "evt": "SPEAKING_START", "data": { "channel_id": "404", "user_id": "1" } }),
            json!({ "evt": "SPEAKING_START", "data": { "user_id": "2" } }),
            json!({ "evt": "VOICE_STATE_UPDATE", "data": { "user_id": "2", "voice_state": { "self_video": true } } }),
        ] {
            let mut event = event;
            event["cmd"] = json!("DISPATCH");
            send(&mut dispatch, event);
        }
        let members = session
            .poll_events()
            .unwrap()
            .unwrap()
            .channel
            .unwrap()
            .members;
        assert!(!members[0].speaking && !members[0].streaming);
        assert!(members[1].speaking && !members[1].streaming);
        let channel = session.poll().unwrap().channel.unwrap();
        assert_eq!(channel.id, "50");
        assert!(
            channel
                .members
                .iter()
                .all(|member| !member.speaking && !member.streaming)
        );
        assert!(!session.subscriptions.contains(&"VOICE_STATE_UPDATE"));
        ipc.join().unwrap();
        drop(dispatch);
        assert!(session.poll_events().is_err());
    }

    #[test]
    fn discord_avatar_urls_use_profile_hash_or_default_index() {
        let hash = "a_0123456789abcdef0123456789abcdef";
        assert_eq!(
            avatar_url(&json!({ "id": "8388608", "avatar": hash })),
            Some(format!("{CDN_URL}/avatars/8388608/{hash}.png?size=64")),
        );
        assert_eq!(
            avatar_url(&json!({ "id": "8388608", "avatar": null, "discriminator": "0" })),
            Some(format!("{CDN_URL}/embed/avatars/2.png")),
        );
        assert_eq!(
            avatar_url(&json!({ "id": "8388608", "discriminator": "0048" })),
            Some(format!("{CDN_URL}/embed/avatars/3.png")),
        );
        assert_eq!(avatar_url(&json!({ "username": "Test account" })), None);
    }

    #[test]
    fn rejected_token_reauthorizes_and_caches_the_new_token() {
        let (client, server) = UnixStream::pair().unwrap();
        let ipc = thread::spawn(move || {
            let mut rpc = Rpc::new(server);
            let request = assert_rpc(
                &mut rpc,
                "AUTHENTICATE",
                json!({ "access_token": "expired" }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHENTICATE", "evt": "ERROR", "nonce": request["nonce"],
                    "data": { "code": 4006, "message": "Token expired" }
                }),
            );
            let request = assert_rpc(
                &mut rpc,
                "AUTHORIZE",
                json!({
                    "client_id": "123456",
                    "scopes": ["identify", "rpc", "rpc.voice.read", "rpc.voice.write"],
                }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHORIZE", "nonce": request["nonce"], "data": { "code": "test+code" }
                }),
            );
            let request = assert_rpc(
                &mut rpc,
                "AUTHENTICATE",
                json!({ "access_token": "new-token" }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHENTICATE", "nonce": request["nonce"],
                    "data": { "user": { "global_name": "Test account" } }
                }),
            );
        });

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/api/oauth2/token", listener.local_addr().unwrap());
        let http = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(RPC_TIMEOUT)).unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim_end(), "POST /api/oauth2/token HTTP/1.1");
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert_eq!(headers["content-type"], "application/x-www-form-urlencoded");
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            reader.read_exact(&mut body).unwrap();
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&body).into_owned().collect();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["client_id"], "123456");
            assert_eq!(form["client_secret"], "app+&secret");
            assert_eq!(form["code"], "test+code");
            assert_eq!(form["redirect_uri"], "http://127.0.0.1");
            let body = r#"{"access_token":"new-token"}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });

        let credentials = Credentials {
            client_id: "123456".into(),
            secret: Some("app+&secret".into()),
            access_token: Some("expired".into()),
        };
        let (updates, _) = async_channel::unbounded();
        let mut authorization = auth::Authorization::new(None);
        let mut login = authorization.login(&credentials, true).unwrap();
        let session = login
            .authenticate(Rpc::new(client), &endpoint, Some(&updates))
            .unwrap();
        assert_eq!(session.voice.account, "Test account");
        http.join().unwrap();
        ipc.join().unwrap();

        let (client, server) = UnixStream::pair().unwrap();
        let ipc = thread::spawn(move || {
            let mut rpc = Rpc::new(server);
            let request = assert_rpc(
                &mut rpc,
                "AUTHENTICATE",
                json!({ "access_token": "new-token" }),
            );
            send(
                &mut rpc,
                json!({
                    "cmd": "AUTHENTICATE", "nonce": request["nonce"], "data": { "user": { "global_name": "Test account" } }
                }),
            );
        });
        login
            .authenticate(Rpc::new(client), &endpoint, None)
            .unwrap();
        ipc.join().unwrap();
    }
}
