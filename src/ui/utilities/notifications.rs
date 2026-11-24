use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use jiff::Zoned;
use std::{cell::RefCell, collections::HashMap, time::Duration};
use zbus::{Error, Message, MessageStream, connection::Builder, zvariant::Value};

pub(super) struct Notice {
    pub key: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub time: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Status {
    #[default]
    Connecting,
    Listening,
    Blocked,
    Unavailable,
    Disconnected,
}

impl Status {
    pub fn from_error(error: &Error) -> Self {
        use zbus::fdo::Error::*;

        let blocked = match error {
            Error::MethodError(name, _, _) => matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.AccessDenied"
                    | "org.freedesktop.DBus.Error.NotSupported"
                    | "org.freedesktop.DBus.Error.UnknownMethod"
                    | "org.freedesktop.DBus.Error.UnknownInterface"
            ),
            Error::FDO(error) => matches!(
                error.as_ref(),
                AccessDenied(_) | NotSupported(_) | UnknownMethod(_) | UnknownInterface(_)
            ),
            _ => false,
        };
        if blocked {
            Self::Blocked
        } else {
            Self::Unavailable
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting to desktop notifications…",
            Self::Listening => "Listening for desktop notifications.",
            Self::Blocked => "The session bus does not allow notification monitoring.",
            Self::Unavailable => "Desktop notification feed unavailable.",
            Self::Disconnected => "Notification bus disconnected. Retrying…",
        }
    }
}

pub(super) async fn listen() -> zbus::Result<MessageStream> {
    let connection = Builder::session()?
        .method_timeout(Duration::from_secs(5))
        .build()
        .await?;
    let stream = MessageStream::from(&connection);
    let rules = [
        "type='method_call',path='/org/freedesktop/Notifications',interface='org.freedesktop.Notifications',member='Notify'",
    ];
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(rules.as_slice(), 0u32),
        )
        .await?;
    Ok(stream)
}

// Freedesktop bodies may contain inline markup and XML entities.
#[derive(Default)]
struct BodyText(RefCell<String>);

impl TokenSink for BodyText {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        match token {
            Token::CharacterTokens(text) => self.0.borrow_mut().push_str(&text),
            Token::TagToken(tag) if tag.kind == TagKind::StartTag && &*tag.name == "br" => {
                self.0.borrow_mut().push('\n')
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn body_text(raw: &str) -> String {
    let tokenizer = Tokenizer::new(BodyText::default(), Default::default());
    let input = BufferQueue::default();
    input.push_back(raw.chars().take(4096).collect::<String>().into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().chars().take(600).collect()
}

pub(super) fn notice(msg: &Message) -> Option<Notice> {
    let header = msg.header();
    if header.member()?.as_str() != "Notify"
        || header.interface()?.as_str() != "org.freedesktop.Notifications"
    {
        return None;
    }
    type Payload<'a> = (
        &'a str,
        u32,
        &'a str,
        &'a str,
        &'a str,
        Vec<&'a str>,
        HashMap<&'a str, Value<'a>>,
        i32,
    );
    let payload = msg.body();
    let (app, _, _, title, body, _, hints, _) = payload.deserialize::<Payload>().ok()?;
    let desktop = hints
        .get("desktop-entry")
        .and_then(|value| <&str>::try_from(value).ok());
    let (key, app) = identity(app, desktop);
    Some(Notice {
        key,
        app,
        title: title.chars().take(300).collect(),
        body: body_text(body),
        time: Zoned::now().strftime("%H:%M").to_string(),
    })
}

fn identity(app: &str, desktop: Option<&str>) -> (String, String) {
    let app = app.trim();
    let desktop = desktop
        .map(str::trim)
        .map(|name| name.strip_suffix(".desktop").unwrap_or(name))
        .filter(|name| !name.is_empty());
    let (kind, key) = match desktop {
        Some(id) => ("desktop", id),
        None => ("name", app),
    };
    let label = if app.is_empty() {
        desktop.unwrap_or("Unknown app")
    } else {
        app
    };
    (
        format!(
            "{kind}:{}",
            key.chars().take(256).collect::<String>().to_lowercase()
        ),
        label.chars().take(120).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::identity;

    #[test]
    fn notification_filter_prefers_desktop_id_and_falls_back_to_the_app_name() {
        let (key, _) = identity("Example", Some("org.example.App.desktop"));
        assert_eq!(key, identity("Localized name", Some("org.example.App")).0);
        assert_ne!(key, identity("Example", Some("org.other.App")).0);
        assert_eq!(
            identity(" Example ", None).0,
            identity("example", Some(" ")).0
        );
    }
}
