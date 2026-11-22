use std::{
    collections::HashMap,
    fs::{self, File},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use async_channel::Sender;
use futures_lite::future::block_on;
use oo7::{
    Secret,
    dbus::{Collection, Service},
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{Credentials, RPC_TIMEOUT, Rpc, RpcError, Session};
use crate::{
    config::DISCORD_REDIRECT_URI,
    model::{DiscordState, Update},
};

pub(super) const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const AUTH_TIMEOUT: Duration = Duration::from_secs(90);
const SCOPES: &[&str] = &["identify", "rpc", "rpc.voice.read", "rpc.voice.write"];

pub(super) struct Authorization {
    lock_path: Option<PathBuf>,
    memory: Option<Tokens>,
}

impl Authorization {
    pub fn new(lock_path: Option<PathBuf>) -> Self {
        Self {
            lock_path,
            memory: None,
        }
    }

    pub fn login<'a>(
        &'a mut self,
        credentials: &'a Credentials,
        interactive: bool,
    ) -> Result<Login<'a>> {
        let keyring = match (credentials.secret.as_ref(), self.lock_path.as_deref()) {
            (Some(_), Some(path)) => match Keyring::open(path, credentials, interactive) {
                Ok(keyring) => Some(keyring),
                Err(error) if error.is::<LockedKeyring>() => return Err(error),
                Err(error) => {
                    log::debug!("Discord keyring unavailable: {error:#}");
                    None
                }
            },
            _ => None,
        };
        let cached = match keyring.as_ref().map(|keyring| keyring.read(interactive)) {
            Some(Ok(tokens)) => tokens,
            Some(Err(error)) if error.is::<LockedKeyring>() => return Err(error),
            Some(Err(error)) => {
                log::warn!("Could not read Discord authorization from the keyring: {error:#}");
                None
            }
            None => None,
        };
        Ok(Login {
            credentials,
            cached,
            keyring,
            memory: &mut self.memory,
        })
    }

    pub fn clear(&mut self, credentials: &Credentials) {
        if let Some(path) = self.lock_path.as_deref()
            && credentials.secret.is_some()
        {
            let result = Keyring::open(path, credentials, true).and_then(|keyring| keyring.clear());
            if let Err(error) = result {
                log::warn!("Could not clear Discord authorization from the keyring: {error:#}");
            }
        }
        self.memory = None;
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Tokens {
    access: String,
    refresh: Option<String>,
    expires_at: Option<u64>,
}

#[derive(Deserialize, Serialize)]
struct SavedAuthorization {
    credentials: String,
    tokens: Tokens,
}

#[derive(Debug)]
struct LockedKeyring;

impl std::fmt::Display for LockedKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Unlock your desktop keyring to resume Discord authorization.")
    }
}

impl std::error::Error for LockedKeyring {}

struct Keyring {
    collection: Collection<'static>,
    attributes: HashMap<&'static str, String>,
    credentials: String,
    _lock: File,
}

impl Keyring {
    fn open(path: &Path, credentials: &Credentials, interactive: bool) -> Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        // Serialize refresh and storage across instances because Discord rotates refresh tokens.
        lock.lock()?;
        let collection = block_on(async {
            let service = Service::encrypted().await?;
            let collection = service
                .with_alias(Service::DEFAULT_COLLECTION)
                .await?
                .context("No default desktop keyring is available.")?;
            if collection.is_locked().await? {
                if !interactive {
                    bail!(LockedKeyring);
                }
                collection.unlock(None).await.map_err(|_| LockedKeyring)?;
                ensure!(!collection.is_locked().await?, LockedKeyring);
            }
            Ok::<_, anyhow::Error>(collection)
        })?;
        Ok(Self {
            collection,
            attributes: [
                ("application", "io.github.secondscreen".into()),
                ("purpose", "discord-authorization".into()),
                ("client-id", credentials.client_id.clone()),
                (
                    "profile",
                    format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes())),
                ),
            ]
            .into(),
            // Secret Service attributes are public metadata. Keep this fingerprint in the secret.
            credentials: format!("{:x}", Sha256::digest(serde_json::to_vec(credentials)?)),
            _lock: lock,
        })
    }

    fn read(&self, interactive: bool) -> Result<Option<Tokens>> {
        block_on(async {
            for item in self.collection.search_items(&self.attributes).await? {
                if item.is_locked().await? {
                    ensure!(interactive, LockedKeyring);
                    item.unlock(None).await.map_err(|_| LockedKeyring)?;
                    ensure!(!item.is_locked().await?, LockedKeyring);
                }
                let secret = item.secret().await?;
                let saved: SavedAuthorization = serde_json::from_slice(secret.as_bytes())
                    .context("Invalid Discord authorization in the keyring.")?;
                if saved.credentials == self.credentials {
                    return Ok(Some(saved.tokens));
                }
            }
            Ok(None)
        })
    }

    fn save(&self, tokens: &Tokens) -> Result<()> {
        let secret = serde_json::to_vec(&SavedAuthorization {
            credentials: self.credentials.clone(),
            tokens: tokens.clone(),
        })?;
        block_on(self.collection.create_item(
            "Second Screen Discord authorization",
            &self.attributes,
            Secret::from(secret),
            true,
            None,
        ))?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        block_on(async {
            for item in self.collection.search_items(&self.attributes).await? {
                let secret = item.secret().await?;
                let saved: SavedAuthorization = serde_json::from_slice(secret.as_bytes())?;
                if saved.credentials == self.credentials {
                    item.delete(None).await?;
                }
            }
            Ok(())
        })
    }
}

pub(super) struct Login<'a> {
    credentials: &'a Credentials,
    cached: Option<Tokens>,
    keyring: Option<Keyring>,
    memory: &'a mut Option<Tokens>,
}

impl Login<'_> {
    pub fn can_resume(&self) -> bool {
        self.tokens().is_some() || self.credentials.access_token.is_some()
    }

    fn tokens(&self) -> Option<&Tokens> {
        self.memory.as_ref().or(self.cached.as_ref())
    }

    fn save(&mut self, tokens: Tokens, updates: Option<&Sender<Update>>) {
        let result = self
            .keyring
            .as_ref()
            .context("Desktop keyring unavailable")
            .and_then(|keyring| keyring.save(&tokens));
        match result {
            Ok(()) => {
                self.cached = Some(tokens);
                *self.memory = None;
            }
            Err(error) => {
                log::warn!("Could not save Discord authorization to the keyring: {error:#}");
                if let Some(updates) = updates {
                    let _ = updates.send_blocking(Update::ActionError(
                        "Discord authorization could not be saved. Unlock or set up your desktop keyring.".into(),
                    ));
                }
                *self.memory = Some(tokens);
            }
        }
    }

    pub fn authenticate(
        &mut self,
        mut rpc: Rpc,
        endpoint: &str,
        interactive: Option<&Sender<Update>>,
    ) -> Result<Session> {
        let access = match self.tokens() {
            Some(tokens)
                if tokens
                    .expires_at
                    .is_none_or(|expires| expires > unix_time()) =>
            {
                Some(tokens.access.as_str())
            }
            Some(_) => None,
            None => self.credentials.access_token.as_deref(),
        };
        if let Some(access) = access {
            match authenticate(&mut rpc, access) {
                Ok(account) => return Ok(Session::new(rpc, account)),
                Err(error) if self.credentials.secret.is_some() && token_rejected(&error) => {}
                Err(error) => return Err(error),
            }
        }

        if let Some(refresh) = self.tokens().and_then(|tokens| tokens.refresh.clone())
            && let Some(tokens) =
                request_token(self.credentials, endpoint, Grant::Refresh(&refresh))?
        {
            // Save the rotated grant even if the desktop connection closes during AUTHENTICATE.
            let access = tokens.access.clone();
            self.save(tokens, interactive);
            let account = authenticate(&mut rpc, &access)?;
            return Ok(Session::new(rpc, account));
        }

        let updates = interactive.context(
            "Discord authorization expired or was revoked. Choose Connect to authorize again.",
        )?;
        let _ = updates.send_blocking(Update::Discord(DiscordState::Authorizing));
        // Discord uses the application's registered redirect for desktop RPC.
        let authorized = rpc.call(
            "AUTHORIZE",
            json!({ "client_id": self.credentials.client_id, "scopes": SCOPES }),
            AUTH_TIMEOUT,
        ).map_err(|error| {
            if error.to_string().to_ascii_lowercase().contains("missing \"redirect_uri\"") {
                anyhow::anyhow!(
                    "{error} Register {DISCORD_REDIRECT_URI} under OAuth2 → Redirects for this application in the Discord Developer Portal."
                )
            } else {
                error
            }
        })?;
        let code = authorized["code"]
            .as_str()
            .context("Discord did not return an authorization code.")?;
        let tokens = request_token(self.credentials, endpoint, Grant::Code(code))?
            .context("Discord rejected the authorization code.")?;
        let access = tokens.access.clone();
        self.save(tokens, Some(updates));
        let account = authenticate(&mut rpc, &access)?;
        Ok(Session::new(rpc, account))
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn token_rejected(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<RpcError>()
        .is_some_and(|error| matches!(error.code, Some(4006 | 4009)))
}

#[derive(Clone, Copy)]
enum Grant<'a> {
    Code(&'a str),
    Refresh(&'a str),
}

fn request_token(
    credentials: &Credentials,
    endpoint: &str,
    grant: Grant<'_>,
) -> Result<Option<Tokens>> {
    #[derive(Deserialize)]
    struct Response {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    }

    #[derive(Deserialize)]
    struct Failure {
        error: String,
    }

    let secret = credentials
        .secret
        .as_deref()
        .context("A Discord application secret is required to exchange tokens.")?;
    let mut form = vec![
        ("client_id", credentials.client_id.as_str()),
        ("client_secret", secret),
    ];
    match grant {
        Grant::Code(code) => form.extend([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", DISCORD_REDIRECT_URI),
        ]),
        Grant::Refresh(token) => {
            form.extend([("grant_type", "refresh_token"), ("refresh_token", token)])
        }
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .post(endpoint)
        .form(&form)
        .send()
        .context("Could not reach Discord's OAuth2 endpoint.")?;
    let status = response.status();
    if !status.is_success() {
        if matches!(grant, Grant::Refresh(_))
            && status == reqwest::StatusCode::BAD_REQUEST
            && response
                .json::<Failure>()
                .is_ok_and(|error| error.error == "invalid_grant")
        {
            return Ok(None);
        }
        bail!("Discord rejected the OAuth2 token request ({status}).");
    }
    let response = response
        .json::<Response>()
        .context("Discord returned an invalid OAuth2 response.")?;
    ensure!(
        !response.access_token.is_empty(),
        "Discord did not return an access token."
    );
    Ok(Some(Tokens {
        access: response.access_token,
        refresh: response.refresh_token.or_else(|| match grant {
            Grant::Refresh(token) => Some(token.to_owned()),
            Grant::Code(_) => None,
        }),
        expires_at: response
            .expires_in
            .and_then(|seconds| unix_time().checked_add(seconds)),
    }))
}

fn authenticate(rpc: &mut Rpc, token: &str) -> Result<String> {
    let result = rpc.call(
        "AUTHENTICATE",
        json!({ "access_token": token }),
        RPC_TIMEOUT,
    )?;
    Ok(result["user"]["global_name"]
        .as_str()
        .or_else(|| result["user"]["username"].as_str())
        .unwrap_or("Discord account")
        .to_owned())
}
