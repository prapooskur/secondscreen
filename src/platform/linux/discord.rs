mod auth;

use anyhow::{Context, Result, bail, ensure};
use async_channel::Sender;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::{Value, json};
use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::Integration;
use crate::{
    config::{self, DiscordCredentials as Credentials},
    model::{DiscordAction, DiscordState, Update, VoiceChannel, VoiceMember, VoiceState},
};

const RPC_TIMEOUT: Duration = Duration::from_secs(2);
const EVENT_INTERVAL: Duration = Duration::from_millis(50);
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
    resume: bool,
    session: Option<Session>,
    error: Option<String>,
}

struct Session {
    rpc: Rpc,
    voice: VoiceState,
    subscriptions: Vec<&'static str>,
}

impl Discord {
    pub fn new(credentials: Option<Credentials>) -> Self {
        Self {
            credentials: effective_credentials(credentials),
            auth: auth::Authorization::new(config::path("discord-session.lock")),
            resume: true,
            session: None,
            error: None,
        }
    }

    fn connect(&mut self, interactive: Option<&Sender<Update>>) -> Result<()> {
        let credentials = self
            .credentials
            .as_ref()
            .context("Configure Discord RPC before connecting.")?;
        let Some(rpc) = Rpc::connect(&credentials.client_id)? else {
            if interactive.is_some() {
                bail!(
                    "Discord desktop is not listening for local RPC. Open Discord and try again."
                );
            }
            return Ok(());
        };

        self.resume = false;
        let mut login = self.auth.login(credentials, interactive.is_some())?;
        if interactive.is_some() || login.can_resume() {
            self.session = Some(login.authenticate(rpc, auth::TOKEN_URL, interactive)?);
        }
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
            self.resume = true;
        }
    }

    fn disconnect(&mut self, error: anyhow::Error) -> DiscordState {
        self.session = None;
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
        if self.resume
            && self.session.is_none()
            && self.credentials.is_some()
            && let Err(error) = self.connect(None)
        {
            self.resume = false;
            self.error = Some(error.to_string());
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

    fn event_interval(&self) -> Option<Duration> {
        self.session
            .as_ref()
            .is_some_and(|session| !session.subscriptions.is_empty())
            .then_some(EVENT_INTERVAL)
    }

    fn poll_events(&mut self) -> Option<Update> {
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
                    self.error = self
                        .connect(Some(updates))
                        .err()
                        .map(|error| error.to_string());
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

struct Rpc {
    socket: UnixStream,
    nonce: u64,
    events: Vec<Value>,
}

impl Rpc {
    fn new(socket: UnixStream) -> Self {
        Self {
            socket,
            nonce: 0,
            events: Vec::new(),
        }
    }

    fn connect(client_id: &str) -> Result<Option<Self>> {
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

        let Some(socket) = directories
            .iter()
            .flat_map(|path| (0..10).map(move |n| path.join(format!("discord-ipc-{n}"))))
            .find_map(|path| UnixStream::connect(path).ok())
        else {
            return Ok(None);
        };
        socket.set_write_timeout(Some(RPC_TIMEOUT))?;
        let mut rpc = Self::new(socket);
        rpc.send(
            0,
            &serde_json::to_vec(&json!({ "v": 1, "client_id": client_id }))?,
        )?;
        let ready = rpc.receive(Instant::now() + RPC_TIMEOUT)?;
        ensure!(
            ready["evt"] == "READY",
            "Discord did not accept the RPC handshake."
        );
        Ok(Some(rpc))
    }

    fn call(&mut self, command: &str, args: Value, timeout: Duration) -> Result<Value> {
        self.request(command, args, None, timeout)
    }

    fn subscription(&mut self, command: &str, event: &str, args: &Value) -> Result<()> {
        self.request(command, args.clone(), Some(event), RPC_TIMEOUT)
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
        self.send(1, &serde_json::to_vec(&payload)?)?;
        let deadline = Instant::now() + timeout;
        loop {
            let response = self.receive(deadline)?;
            if response["nonce"].as_str() != Some(&nonce) {
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
        let deadline = Instant::now() + RPC_TIMEOUT;
        for _ in 0..64 {
            let mut fd = [PollFd::new(&self.socket, PollFlags::IN)];
            match poll(&mut fd, Some(&Timespec::default())) {
                Ok(0) | Err(rustix::io::Errno::INTR) => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(error).context("Could not check for Discord voice events.");
                }
            }
            if let Some(message) = self.receive_frame(deadline)? {
                self.queue_event(message);
            }
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
        loop {
            if let Some(message) = self.receive_frame(deadline)? {
                return Ok(message);
            }
        }
    }

    fn receive_frame(&mut self, deadline: Instant) -> Result<Option<Value>> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Discord RPC timed out.");
        self.socket.set_read_timeout(Some(remaining))?;
        let mut header = [0u8; 8];
        self.socket
            .read_exact(&mut header)
            .context("Discord desktop did not send a reply.")?;
        let opcode = u32::from_le_bytes(header[..4].try_into()?);
        let size = u32::from_le_bytes(header[4..].try_into()?) as usize;
        ensure!(
            size <= 8 * 1024 * 1024,
            "Discord sent an oversized RPC frame."
        );
        let mut payload = vec![0; size];
        self.socket
            .read_exact(&mut payload)
            .context("Discord desktop closed an RPC frame early.")?;
        match opcode {
            1 => {
                return serde_json::from_slice(&payload)
                    .map(Some)
                    .context("Discord sent invalid RPC data.");
            }
            2 => {
                let message = serde_json::from_slice::<Value>(&payload)
                    .ok()
                    .and_then(|value| value["message"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Discord closed its RPC connection.".into());
                bail!("{message}");
            }
            3 => self.send(4, &payload)?,
            4 => {}
            _ => bail!("Discord sent an unknown RPC frame."),
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::{BufRead, BufReader},
        net::TcpListener,
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
