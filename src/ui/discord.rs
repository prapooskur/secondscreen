use gpui::{Context, FontWeight, IntoElement, Styled, div, prelude::*};

use super::{
    Dashboard,
    icons::{Icon, icon},
    theme::*,
};
use crate::model::{DiscordAction, DiscordState, VoiceState};

impl Dashboard {
    pub(super) fn discord_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.discord {
            DiscordState::Connected(voice) => self.connected(voice, cx).into_any_element(),
            state => self.disconnected(state, cx).into_any_element(),
        };
        col()
            .flex_1()
            .gap(r(12.))
            .child(section_header("Discord voice", None))
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
            .justify_center()
            .gap(r(8.))
            .flex_none()
            .child(bind(
                button("open-discord", None).child("Open Discord"),
                |_, cx| cx.open_url("discord://-/channels/@me"),
            ))
            .when(matches!(state, DiscordState::Unconfigured), |it| {
                it.child(bind(
                    button("rpc-setup", Some(VIOLET)).child("RPC documentation"),
                    |_, cx| {
                        cx.open_url("https://docs.discord.com/developers/topics/rpc");
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

        panel()
            .flex_1()
            .justify_center()
            .items_center()
            .p(r(14.))
            .gap(r(4.))
            .overflow_hidden()
            .child(
                empty(Icon::Chat, VIOLET, headline, description)
                    .flex_none()
                    .flex_basis(gpui::auto()),
            )
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
        let account = panel()
            .flex_1()
            .p(r(16.))
            .gap(r(11.))
            .overflow_hidden()
            .child(
                row()
                    .gap(r(9.))
                    .child(dot(TEAL))
                    .child(label(voice.account.clone()).truncate()),
            )
            .child(
                col()
                    .flex_1()
                    .justify_center()
                    .gap(r(9.))
                    .child(
                        title(
                            channel
                                .map(|channel| channel.name.clone())
                                .unwrap_or_else(|| "No active voice channel".into()),
                        )
                        .truncate(),
                    )
                    .child(
                        quiet(channel.map_or_else(
                            || "Join a call in Discord to see its participants here.".to_owned(),
                            |channel| format!("{} in voice", channel.members.len()),
                        ))
                        .truncate(),
                    )
                    .child(
                        row()
                            .gap(r(7.))
                            .child(mute)
                            .child(deafen)
                            .when(channel.is_some(), |it| it.child(leave)),
                    ),
            )
            .child(row().justify_start().child(disconnect));

        let participants = panel()
            .w(gpui::relative(0.43))
            .overflow_hidden()
            .child(
                row()
                    .h(r(50.))
                    .px(r(12.))
                    .flex_none()
                    .gap(r(8.))
                    .border_b_1()
                    .border_color(c(STROKE))
                    .child(icon_tile(Icon::Chat, VIOLET, 28.))
                    .child(label("Participants").text_color(c(INK))),
            )
            .child(
                col()
                    .id("voice-members")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(r(12.))
                    .gap(r(6.))
                    .when(
                        channel.is_none_or(|channel| channel.members.is_empty()),
                        |it| it.child(quiet("No participants to show")),
                    )
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
                    ),
            );

        split()
            .flex_1()
            .min_h_0()
            .gap(r(12.))
            .child(account)
            .child(participants)
    }
}

pub(super) fn discord_button(
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
            .child(icon(symbol, c(color.unwrap_or(TEXT)), 15.))
            .child(name),
        move |_, cx| {
            view.read(cx).send_discord(action());
        },
    )
}
