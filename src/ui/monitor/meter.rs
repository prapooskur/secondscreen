use gpui::{Bounds, IntoElement, PathBuilder, Styled, canvas, fill, point, prelude::*, px, size};
use std::f32::consts::PI;

use crate::ui::theme::*;

pub(in crate::ui) fn monitoring_bar(theme: Theme, load: f32, accent: Color) -> impl IntoElement {
    const SEGMENTS: u32 = 20;

    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let step = bounds.size.width / SEGMENTS as f32;
            let width = (step - bounds.size.height * 0.36).max(px(0.));
            let filled = load.clamp(0., 100.) * SEGMENTS as f32 / 100.;
            for index in 0..SEGMENTS {
                let origin = bounds.origin + point(step * index as f32, px(0.));
                let segment = Bounds::new(origin, size(width, bounds.size.height));
                window.paint_quad(fill(segment, theme.color(STROKE)));
                let fraction = (filled - index as f32).clamp(0., 1.);
                if fraction > 0. {
                    let active = Bounds::new(origin, size(width * fraction, bounds.size.height));
                    window.paint_quad(fill(active, theme.color(accent)));
                }
            }
        },
    )
    .w_full()
    .h(r(5.))
    .flex_none()
}

pub(in crate::ui) fn gauge(
    theme: Theme,
    value: Option<f32>,
    accent: Color,
    diameter: f32,
) -> gpui::Div {
    const TICKS: usize = 25;
    let reading = value.map_or_else(|| "—".into(), |value| format!("{value:.0}%"));

    col()
        .relative()
        .size(r(diameter))
        .flex_none()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let center = bounds.center();
                    let radius = bounds.size.width * 0.46;
                    let level = value.unwrap_or_default().clamp(0., 100.) / 100.;
                    let lit = (level * TICKS as f32).round() as usize;
                    let thickness = radius * 0.078;
                    let mut track = PathBuilder::stroke(thickness);
                    let mut active = PathBuilder::stroke(thickness);
                    for index in 0..TICKS {
                        let angle = PI * (0.75 + 1.5 * index as f32 / (TICKS - 1) as f32);
                        let direction = point(radius * angle.cos(), radius * angle.sin());
                        let ticks = if index < lit { &mut active } else { &mut track };
                        ticks.move_to(center + direction * 0.76);
                        ticks.line_to(center + direction);
                    }
                    if let Ok(path) = track.build() {
                        window.paint_path(path, theme.color(STROKE));
                    }
                    if let Ok(path) = active.build() {
                        window.paint_path(path, theme.color(accent));
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(
            row().absolute().inset_0().justify_center().child(
                mono(reading)
                    .text_size(r(diameter * 0.23))
                    .text_color(theme.color(if value.is_some() { accent } else { MUTED })),
            ),
        )
}
