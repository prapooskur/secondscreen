use anyhow::Result;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use jiff::Zoned;
use std::cell::RefCell;
use std::collections::HashMap;
use zbus::{Connection, Message, MessageStream, zvariant::Value};

pub(super) struct Notice {
    pub app: String,
    pub title: String,
    pub body: String,
    pub time: String,
}

pub(super) async fn listen() -> Result<MessageStream> {
    let connection = Connection::session().await?;
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
    let (app, _, _, title, body, _, _, _) = payload.deserialize::<Payload>().ok()?;
    Some(Notice {
        app: app.chars().take(120).collect(),
        title: title.chars().take(300).collect(),
        body: body_text(body),
        time: Zoned::now().strftime("%H:%M").to_string(),
    })
}
