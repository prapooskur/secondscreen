use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use gpui::{App, Global, Hsla, IntoElement, RenderImage, Styled, Window, canvas};
use image::{Frame, RgbaImage};
use resvg::tiny_skia::{
    Color as PaintColor, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    Rect, SpreadMode, Stroke, Transform,
};

use crate::ui::{HISTORY, theme::*};

#[derive(PartialEq)]
struct Graph {
    primary: VecDeque<f32>,
    secondary: Option<VecDeque<f32>>,
    maximum: f32,
    grid: Hsla,
    accent: Hsla,
    blue: Hsla,
}

#[derive(Default)]
struct Charts(HashMap<(u32, usize), ChartTexture>);
impl Global for Charts {}

struct ChartTexture {
    graph: Graph,
    size: (u32, u32),
    scale: f32,
    image: Arc<RenderImage>,
}

pub(in crate::ui) fn discard_charts(widget: Option<u32>, window: &mut Window, cx: &mut App) {
    if cx.try_global::<Charts>().is_some() {
        cx.global_mut::<Charts>().0.retain(|(id, _), texture| {
            if widget.is_some_and(|widget| widget != *id) {
                return true;
            }
            let _ = window.drop_image(texture.image.clone());
            false
        });
    }
}

pub(in crate::ui) fn graph(
    id: (u32, usize),
    theme: Theme,
    primary: VecDeque<f32>,
    secondary: Option<VecDeque<f32>>,
    maximum: f32,
    accent: Color,
) -> impl IntoElement {
    let graph = Graph {
        primary,
        secondary,
        maximum,
        grid: theme.color(STROKE),
        accent: theme.color(accent),
        blue: theme.color(BLUE),
    };
    canvas(
        |_, _, _| (),
        move |area, _, window, cx| {
            let clip = area.intersect(&window.content_mask().bounds);
            if clip.size.width <= gpui::px(0.) || clip.size.height <= gpui::px(0.) {
                return;
            }
            let scale = window.scale_factor();
            let size = (
                (f32::from(area.size.width) * scale).ceil() as u32,
                (f32::from(area.size.height) * scale).ceil() as u32,
            );
            let charts = &mut cx.default_global::<Charts>().0;
            let image = if let Some(texture) = charts.get(&id)
                && texture.graph == graph
                && texture.size == size
                && texture.scale == scale
            {
                texture.image.clone()
            } else {
                let Some(image) = rasterize(&graph, size, scale) else {
                    return;
                };
                let texture = ChartTexture {
                    graph,
                    size,
                    scale,
                    image: image.clone(),
                };
                if let Some(old) = charts.insert(id, texture) {
                    let _ = window.drop_image(old.image);
                }
                image
            };
            #[cfg(feature = "ui-gpui")]
            let result = window.paint_image(area, Default::default(), image, 0, false);
            #[cfg(not(feature = "ui-gpui"))]
            let result = window.paint_image(area, area, Default::default(), image, 0, false);
            if let Err(error) = result {
                log::debug!("Could not paint chart: {error:#}");
            }
        },
    )
    .flex_1()
    .w_full()
    .min_h(r(14.))
}

fn rasterize(graph: &Graph, (width, height): (u32, u32), scale: f32) -> Option<Arc<RenderImage>> {
    let mut pixmap = Pixmap::new(width, height)?;
    let top = 3. * scale;
    let bottom = (height as f32 - 4. * scale).max(top);
    let width = (width as f32 - 4. * scale).max(0.);
    let height = bottom - top;
    let transform = Transform::identity();
    let grid = solid(graph.grid);
    for y in [top, top + height * 0.5, bottom] {
        if let Some(rect) = Rect::from_xywh(0., y, width, scale) {
            pixmap.fill_rect(rect, &grid, transform, None);
        }
    }
    let mut draw = |values: &VecDeque<f32>, color: Hsla, fill: bool| {
        let mut points = values.iter().enumerate().map(|(index, value)| {
            let offset = (values.len() - index - 1) as f32 / (HISTORY - 1) as f32;
            Point::from_xy(
                width * (1. - offset),
                bottom - height * (value / graph.maximum).clamp(0., 1.),
            )
        });
        let Some(first) = points.next() else {
            return;
        };
        let mut curve = PathBuilder::new();
        curve.move_to(first.x, first.y);
        let mut last = first;
        for point in points {
            curve.line_to(point.x, point.y);
            last = point;
        }
        if first != last {
            if fill && height > 0. {
                let mut area = curve.clone();
                area.line_to(last.x, bottom);
                area.line_to(first.x, bottom);
                area.close();
                let shader = LinearGradient::new(
                    Point::from_xy(0., top),
                    Point::from_xy(0., bottom),
                    vec![
                        GradientStop::new(0., paint_color(color.opacity(0.23))),
                        GradientStop::new(1., paint_color(color.opacity(0.025))),
                    ],
                    SpreadMode::Pad,
                    transform,
                );
                if let (Some(area), Some(shader)) = (area.finish(), shader) {
                    let paint = Paint {
                        shader,
                        ..Paint::default()
                    };
                    pixmap.fill_path(&area, &paint, FillRule::Winding, transform, None);
                }
            }
            if let Some(line) = curve.finish() {
                let stroke = Stroke {
                    width: 1.65 * scale,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&line, &solid(color), &stroke, transform, None);
            }
        }
        if let Some(dot) = PathBuilder::from_circle(last.x, last.y, 2.5 * scale) {
            pixmap.fill_path(&dot, &solid(color), FillRule::Winding, transform, None);
        }
    };
    if let Some(secondary) = &graph.secondary {
        draw(secondary, graph.blue, false);
    }
    draw(&graph.primary, graph.accent, true);

    let dimensions = (pixmap.width(), pixmap.height());
    let mut pixels = pixmap.take();
    // Tiny-skia stores premultiplied RGBA. GPUI sprites take straight BGRA.
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for component in &mut pixel[..3] {
                *component = ((u16::from(*component) * 255 + alpha / 2) / alpha) as u8;
            }
        }
        pixel.swap(0, 2);
    }
    let image = RgbaImage::from_raw(dimensions.0, dimensions.1, pixels)?;
    Some(Arc::new(RenderImage::new([Frame::new(image)])))
}

fn paint_color(color: Hsla) -> PaintColor {
    let color = gpui::Rgba::from(color);
    PaintColor::from_rgba(color.r, color.g, color.b, color.a).unwrap_or(PaintColor::TRANSPARENT)
}

fn solid(color: Hsla) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(paint_color(color));
    paint
}
