use gpui::{Context, FontWeight, IntoElement, Styled, div, prelude::*};

use super::{
    Dashboard,
    icons::{Icon, icon},
    settings::Section,
    theme::*,
};
use crate::model::{DiscordAction, DiscordState, VoiceState};

impl Dashboard {
    pub(super) fn discord_widget(&self, cx: &Context<Self>) -> impl IntoElement {
        let content = match &self.discord {
            DiscordState::Connected(voice) => self.connected(voice, cx).into_any_element(),
            state => self.disconnected(state, cx).into_any_element(),
        };
        col()
            .id("voice-content")
            .flex_1()
            .overflow_y_scroll()
            .child(content)
    }

    fn disconnected(&self, state: &DiscordState, cx: &Context<Self>) -> impl IntoElement {
        let (headline, description) = match state {
            DiscordState::Unconfigured => ("Set up Discord RPC", "Configure a Discord application with voice RPC access to enable channel and microphone controls.".to_owned()),
            DiscordState::Disconnected => ("Connect Discord", "Open the desktop client and authorize Second Screen to access your voice settings.".to_owned()),
            DiscordState::Authorizing => ("Waiting for Discord", "Finish the authorization in the Discord desktop client.".to_owned()),
            DiscordState::Error(message) => ("Discord connection failed", message.clone()),
            DiscordState::Connected(_) => unreachable!(),
        };
        let controls = row()
            .flex_wrap()
            .gap(r(6.))
            .flex_none()
            .child(bind(
                button("open-discord", None).child("Open Discord"),
                |_, cx| cx.open_url("discord://-/channels/@me"),
            ))
            .when(matches!(state, DiscordState::Unconfigured), |it| {
                let view = cx.entity();
                it.child(bind(
                    button("rpc-setup", Some(VIOLET)).child("Discord settings"),
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
                    it.child(bind(
                        button("connect-discord", Some(VIOLET)).child("Connect"),
                        move |_, cx| {
                            view.read(cx).send_discord(DiscordAction::Connect);
                        },
                    ))
                },
            );

        col()
            .flex_none()
            .gap(r(8.))
            .child(label(headline).text_color(c(INK)))
            .child(quiet(description))
            .child(controls)
    }

    fn connected(&self, voice: &VoiceState, cx: &Context<Self>) -> impl IntoElement {
        let mute = discord_button(
            "discord-mute",
            if voice.muted { Icon::MicOff } else { Icon::Mic },
            if voice.muted { "Unmute" } else { "Mute" },
            voice.muted.then_some(AMBER),
            || DiscordAction::ToggleMute,
            cx,
        );
        let deafen = discord_button(
            "discord-deafen",
            Icon::Headphones,
            if voice.deafened { "Undeafen" } else { "Deafen" },
            voice.deafened.then_some(AMBER),
            || DiscordAction::ToggleDeafen,
            cx,
        );
        let leave = discord_button(
            "discord-leave",
            Icon::Phone,
            "Leave call",
            Some(RED),
            || DiscordAction::Leave,
            cx,
        );
        let disconnect = {
            let view = cx.entity();
            bind(
                button("disconnect-rpc", None).child("Disconnect RPC"),
                move |_, cx| view.read(cx).send_discord(DiscordAction::Disconnect),
            )
        };
        let channel = voice.channel.as_ref();
        col()
            .gap(r(7.))
            .flex_none()
            .child(
                row()
                    .gap(r(6.))
                    .child(dot(TEAL))
                    .child(quiet(voice.account.clone()).truncate()),
            )
            .child(
                label(
                    channel
                        .map(|channel| channel.name.clone())
                        .unwrap_or_else(|| "No active voice channel".into()),
                )
                .text_color(c(INK))
                .truncate(),
            )
            .child(
                row()
                    .gap(r(5.))
                    .flex_wrap()
                    .child(mute)
                    .child(deafen)
                    .when(channel.is_some(), |it| it.child(leave)),
            )
            .child(row().child(disconnect))
            .when_some(channel, |it, channel| {
                it.child(quiet(format!("{} in voice", channel.members.len())))
            })
            .children(
                channel
                    .into_iter()
                    .flat_map(|channel| &channel.members)
                    .map(|member| {
                        let initial = member
                            .name
                            .chars()
                            .next()
                            .map(|letter| letter.to_uppercase().to_string())
                            .unwrap_or_else(|| "?".into());
                        row()
                            .h(r(38.))
                            .px(r(7.))
                            .gap(r(8.))
                            .flex_none()
                            .rounded(r(6.))
                            .bg(c(BACK).opacity(0.45))
                            .child(
                                row()
                                    .size(r(25.))
                                    .flex_none()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(c(VIOLET).opacity(0.13))
                                    .text_color(c(VIOLET))
                                    .text_size(r(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(initial),
                            )
                            .child(div().flex_1().truncate().child(member.name.clone()))
                            .when(member.muted, |it| {
                                it.child(icon(Icon::MicOff, c(AMBER), 15.))
                            })
                            .when(member.deafened, |it| {
                                it.child(icon(Icon::Headphones, c(AMBER), 15.))
                            })
                    }),
            )
    }
}

fn discord_button(
    id: &'static str,
    symbol: Icon,
    name: &'static str,
    color: Option<u32>,
    action: impl Fn() -> DiscordAction + 'static,
    cx: &Context<Dashboard>,
) -> impl IntoElement {
    let view = cx.entity();
    bind(
        button(id, color)
            .h(r(29.))
            .px(r(6.))
            .gap(r(4.))
            .child(icon(symbol, c(color.unwrap_or(TEXT)), 15.))
            .child(name),
        move |_, cx| {
            view.read(cx).send_discord(action());
        },
    )
}
