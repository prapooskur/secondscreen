use gpui::{Context, Div, FontWeight, IntoElement, ObjectFit, Stateful, Styled, div, prelude::*};

use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    media::remote_image,
    settings::Section,
    theme::*,
};
use crate::model::{DiscordAction, DiscordState, VoiceMember, VoiceState};
use crate::viewer::DISCORD_HOME;

impl Dashboard {
    pub(super) fn discord_widget(&self, cx: &Context<Self>) -> impl IntoElement {
        let (content, controls) = match &self.discord {
            DiscordState::Connected(voice) => self.connected(voice, cx),
            state => self.disconnected(state, cx),
        };
        col()
            .flex_1()
            .gap(r(7.))
            .child(
                col()
                    .id("voice-content")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(content),
            )
            .child(controls)
    }

    fn disconnected(&self, state: &DiscordState, cx: &Context<Self>) -> (Div, Div) {
        let theme = Theme::current(cx);
        let (headline, description) = match state {
            DiscordState::Unconfigured => ("Set up Discord RPC", "Configure a Discord application with voice RPC access to enable channel and microphone controls.".to_owned()),
            DiscordState::Disconnected => ("Connect Discord", "Open the desktop client and authorize Second Screen to access your voice settings.".to_owned()),
            DiscordState::Authorizing => ("Waiting for Discord", "Finish the authorization in the Discord desktop client.".to_owned()),
            DiscordState::Error(message) => ("Discord connection failed", message.clone()),
            DiscordState::Connected(_) => unreachable!(),
        };
        let view = cx.entity();
        let controls = row()
            .flex_wrap()
            .gap(r(6.))
            .flex_none()
            .child(theme.bind(
                control(theme, "open-discord", Icon::Discord, "Open Discord", None),
                move |window, cx| {
                    view.update(cx, |this, cx| this.open_discord(DISCORD_HOME, window, cx))
                },
            ))
            .when(matches!(state, DiscordState::Unconfigured), |it| {
                let view = cx.entity();
                it.child(theme.bind(
                    control(
                        theme,
                        "rpc-setup",
                        Icon::Settings,
                        "Discord settings",
                        Some(VIOLET),
                    ),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            this.open_settings(Section::Discord, window, cx)
                        })
                    },
                ))
            })
            .when(
                matches!(state, DiscordState::Disconnected | DiscordState::Error(_)),
                |it| {
                    let view = cx.entity();
                    it.child(theme.bind(
                        control(
                            theme,
                            "connect-discord",
                            Icon::Link,
                            "Connect",
                            Some(VIOLET),
                        ),
                        move |_, cx| {
                            view.read(cx).send_discord(DiscordAction::Connect);
                        },
                    ))
                },
            );

        let content = col()
            .flex_none()
            .gap(r(8.))
            .child(theme.label(headline).text_color(theme.color(INK)))
            .child(theme.quiet(description));
        (content, controls)
    }

    fn connected(&self, voice: &VoiceState, cx: &Context<Self>) -> (Div, Div) {
        let theme = Theme::current(cx);
        let mute = discord_button(
            theme,
            "discord-mute",
            if voice.muted { Icon::MicOff } else { Icon::Mic },
            if voice.muted { "Unmute" } else { "Mute" },
            voice.muted.then_some(AMBER),
            || DiscordAction::ToggleMute,
            cx,
        );
        let deafen = discord_button(
            theme,
            "discord-deafen",
            Icon::Headphones,
            if voice.deafened { "Undeafen" } else { "Deafen" },
            voice.deafened.then_some(AMBER),
            || DiscordAction::ToggleDeafen,
            cx,
        );
        let leave = discord_button(
            theme,
            "discord-leave",
            Icon::Phone,
            "Leave call",
            Some(RED),
            || DiscordAction::Leave,
            cx,
        );
        let channel = voice.channel.as_ref();
        let voice_controls = row()
            .gap(r(6.))
            .flex_none()
            .child(mute)
            .child(deafen)
            .when(channel.is_some(), |it| it.child(leave));
        let controls = row()
            .flex_none()
            .justify_between()
            .gap(r(8.))
            .child(voice_controls)
            .child(
                row()
                    .gap(r(6.))
                    .child(theme.dot(TEAL))
                    .child(theme.quiet(voice.account.clone()).truncate()),
            );
        let content = col()
            .gap(r(7.))
            .flex_none()
            .child(
                row()
                    .justify_between()
                    .gap(r(8.))
                    .child(
                        theme
                            .label(
                                channel
                                    .map(|channel| channel.name.clone())
                                    .unwrap_or_else(|| "No active voice channel".into()),
                            )
                            .text_color(theme.color(INK))
                            .truncate(),
                    )
                    .when_some(channel, |it, channel| {
                        it.child(
                            theme
                                .quiet(format!("{} in voice", channel.members.len()))
                                .flex_none(),
                        )
                    }),
            )
            .when_some(channel, |it, channel| {
                it.children(
                    channel
                        .members
                        .iter()
                        .enumerate()
                        .map(|(index, member)| member_row(theme, index, member)),
                )
            });
        (content, controls)
    }
}

fn member_row(theme: Theme, index: usize, member: &VoiceMember) -> Div {
    let initial = member
        .name
        .chars()
        .next()
        .map(|letter| letter.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    let avatar = row()
        .id(("discord-avatar", index))
        .size(r(25.))
        .flex_none()
        .relative()
        .justify_center()
        .rounded_full()
        .bg(theme.color(VIOLET).opacity(0.13))
        .text_color(theme.color(VIOLET))
        .text_size(r(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
        .when_some(member.avatar_url.clone(), |it, url| {
            it.child(
                remote_image(url)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            )
        })
        .when(member.speaking, |it| {
            it.child(
                div()
                    .absolute()
                    .inset(r(-2.))
                    .rounded_full()
                    .border_2()
                    .border_color(theme.color(SPEAKING_GREEN)),
            )
            .tooltip(|_, cx| cx.new(|_| Tooltip("Speaking".into())).into())
        });
    row()
        .h(r(38.))
        .px(r(7.))
        .gap(r(8.))
        .flex_none()
        .rounded(r(6.))
        .bg(theme.color(BACK).opacity(0.45))
        .child(avatar)
        .child(div().flex_1().truncate().child(member.name.clone()))
        .when(member.streaming, |it| {
            it.child(theme.chip("LIVE", RED).h(r(21.)).px(r(5.)))
        })
        .when(member.muted, |it| {
            it.child(icon(Icon::MicOff, theme.color(AMBER), 15.))
        })
        .when(member.deafened, |it| {
            it.child(icon(Icon::Headphones, theme.color(AMBER), 15.))
        })
}

fn control(
    theme: Theme,
    id: &'static str,
    symbol: Icon,
    name: &'static str,
    color: Option<Color>,
) -> Stateful<Div> {
    theme
        .button(id, color)
        .size(r(32.))
        .px_0()
        .child(icon(symbol, theme.color(color.unwrap_or(TEXT)), 16.))
        .tooltip(move |_, cx| cx.new(|_| Tooltip(name.into())).into())
}

fn discord_button(
    theme: Theme,
    id: &'static str,
    symbol: Icon,
    name: &'static str,
    color: Option<Color>,
    action: impl Fn() -> DiscordAction + 'static,
    cx: &Context<Dashboard>,
) -> impl IntoElement {
    let view = cx.entity();
    theme.bind(control(theme, id, symbol, name, color), move |_, cx| {
        view.read(cx).send_discord(action());
    })
}
