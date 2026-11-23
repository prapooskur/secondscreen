use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use anyhow::{Context as _, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use gpui::{Context, IntoElement, RenderImage, ScrollHandle, Styled, Window, img, prelude::*};
use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};

use crate::config;

use super::{
    super::{Dashboard, Tooltip, icons::Icon, settings::Section, theme::*},
    Layout, RAIL_BUTTON_SIZE, RAIL_WIDTH, rail_control,
};

const MAX_PROFILES: usize = 5;
const ICON_SIZE: u32 = 128;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Profiles {
    #[serde(default)]
    pub active: usize,
    #[serde(rename = "profiles")]
    pub items: Vec<Profile>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(in crate::ui) struct Profile {
    pub(in crate::ui::home) layout: Layout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    badge: Option<Badge>,
    #[serde(skip)]
    image: Option<Arc<RenderImage>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ui) enum Badge {
    Label(char),
    Png(String),
}

impl Default for Profiles {
    fn default() -> Self {
        Self::from_layout(Layout::default())
    }
}

impl Profiles {
    fn from_layout(layout: Layout) -> Self {
        Self {
            active: 0,
            items: vec![Profile::new(layout)],
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let document: serde_json::Value = config::read_json(path)?;
        let mut profiles = if document.is_null() {
            Self::default()
        } else if document.get("profiles").is_some() {
            serde_json::from_value(document).context("Invalid profile configuration")?
        } else {
            Self::from_layout(serde_json::from_value(document).context("Invalid Home layout")?)
        };
        if profiles.items.is_empty() {
            profiles = Self::default();
        }
        profiles.items.truncate(MAX_PROFILES);
        profiles.active = profiles.active.min(profiles.items.len() - 1);
        for profile in &mut profiles.items {
            profile.layout.normalize();
            match profile.badge.as_ref().map(Badge::image).transpose() {
                Ok(image) => profile.image = image.flatten(),
                Err(error) => log::warn!("Could not load a profile image: {error:#}"),
            }
        }
        Ok(profiles)
    }

    pub fn add(&mut self) -> Option<usize> {
        let index = self.items.len();
        if index >= MAX_PROFILES {
            return None;
        }
        self.items.push(Profile::new(Layout::empty()));
        Some(index)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        config::write_json(path, self).context("Could not save the Home profiles")
    }
}

impl Profile {
    fn new(layout: Layout) -> Self {
        Self {
            layout,
            badge: None,
            image: None,
        }
    }

    pub(in crate::ui) fn label(&self, index: usize) -> String {
        match self.badge {
            Some(Badge::Label(label)) if label.is_alphanumeric() => label.to_string(),
            _ => (index + 1).to_string(),
        }
    }

    pub(in crate::ui) fn button_content(&self, index: usize, size: f32) -> impl IntoElement {
        if let Some(image) = &self.image {
            img(image.clone()).size(r(size)).into_any_element()
        } else {
            mono(self.label(index)).text_size(r(16.)).into_any_element()
        }
    }
}

impl Badge {
    fn image(&self) -> Result<Option<Arc<RenderImage>>> {
        let Self::Png(encoded) = self else {
            return Ok(None);
        };
        ensure!(
            encoded.len() <= 128 * 1024,
            "Profile icon data is too large."
        );
        let bytes = STANDARD.decode(encoded)?;
        let mut image = read_png(&bytes, ICON_SIZE)?.into_rgba8();
        for pixel in image.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Some(Arc::new(RenderImage::new([image::Frame::new(image)]))))
    }
}

pub(in crate::ui) fn import_profile_image(path: &Path) -> Result<Badge> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let svg = extension.eq_ignore_ascii_case("svg");
    ensure!(
        svg || extension.eq_ignore_ascii_case("png"),
        "Choose an SVG or PNG file."
    );
    ensure!(path.is_file(), "Choose an SVG or PNG file.");
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_IMAGE_BYTES,
        "Choose an image smaller than 8 MiB."
    );
    let png = if svg {
        render_svg(&bytes)?
    } else {
        let mut png = Cursor::new(Vec::new());
        read_png(&bytes, 4096)?
            .thumbnail(ICON_SIZE, ICON_SIZE)
            .write_to(&mut png, image::ImageFormat::Png)?;
        png.into_inner()
    };
    Ok(Badge::Png(STANDARD.encode(png)))
}

fn read_png(bytes: &[u8], max_dimension: u32) -> Result<image::DynamicImage> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

fn render_svg(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(bytes, &options)?;
    let size = tree.size();
    let side = ICON_SIZE as f32;
    let scale = side / size.width().max(size.height());
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.,
        0.,
        scale,
        (side - size.width() * scale) / 2.,
        (side - size.height() * scale) / 2.,
    );
    let mut pixmap =
        tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).context("Could not render the SVG.")?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

impl Dashboard {
    pub(super) fn profile_rail(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let profiles = &self.home.profiles;
        let view = cx.entity();
        let edit = view.clone();
        theme
            .panel()
            .w(r(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .gap(r(5.))
            .py(r(5.))
            .children(profiles.items.iter().enumerate().map(|(index, profile)| {
                let active = index == profiles.active;
                let view = view.clone();
                let description = if active {
                    format!("Profile {} (active)", index + 1)
                } else {
                    format!("Switch to profile {}", index + 1)
                };
                theme.bind(
                    theme
                        .button(("switch-profile", index), active.then_some(TEAL))
                        .size(r(RAIL_BUTTON_SIZE))
                        .px_0()
                        .child(profile.button_content(index, 24.))
                        .tooltip(move |_, cx| {
                            cx.new(|_| Tooltip(description.clone().into())).into()
                        }),
                    move |window, cx| {
                        view.update(cx, |this, cx| this.switch_profile(index, window, cx))
                    },
                )
            }))
            .when(profiles.items.len() < MAX_PROFILES, |it| {
                let view = view.clone();
                it.child(rail_control(
                    theme,
                    "add-profile",
                    "New profile",
                    Icon::Plus,
                    false,
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(index) = this.home.profiles.add() {
                                this.switch_profile(index, window, cx);
                            }
                        })
                    },
                ))
            })
            .child(col().flex_1())
            .child(rail_control(
                theme,
                "edit-profile",
                "Customize profile button",
                Icon::Edit,
                false,
                move |window, cx| {
                    edit.update(cx, |this, cx| {
                        this.open_settings(Section::Profiles, window, cx)
                    })
                },
            ))
    }

    fn switch_profile(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.home.profiles.active == index || index >= self.home.profiles.items.len() {
            return;
        }
        self.home.profiles.active = index;
        self.home.leave();
        self.home.scroll = ScrollHandle::new();
        self.viewers.clear();
        super::super::monitor::discard_charts(None, window, cx);
        self.gesture.borrow_mut().take();
        self.focus_dashboard(window, cx);
        self.save_home(cx);
    }

    pub(in crate::ui) fn set_profile_badge(
        &mut self,
        index: usize,
        badge: Option<Badge>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let image = badge.as_ref().map(Badge::image).transpose()?.flatten();
        let profile = &mut self.home.profiles.items[index];
        if let Some(previous) = profile.image.take() {
            cx.drop_image(previous, None);
        }
        profile.badge = badge;
        profile.image = image;
        self.error = None;
        self.save_home(cx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::home::Kind;
    use std::fs;

    #[test]
    fn migrates_a_legacy_layout_and_saves_independent_profiles() {
        let path =
            std::env::temp_dir().join(format!("secondscreen-profiles-{}.json", std::process::id()));
        fs::write(&path, r#"{"widgets":[{"kind":"performance","id":9,"columns":2,"rows":5}],"sensors":{"9":{"chart":"memory"}}}"#).unwrap();
        let mut profiles = Profiles::read(&path).unwrap();
        assert_eq!(profiles.active, 0);
        assert_eq!(profiles.items.len(), 1);
        for index in 1..MAX_PROFILES {
            assert_eq!(profiles.add(), Some(index));
            assert!(profiles.items[index].layout.widgets.is_empty());
        }
        assert_eq!(profiles.add(), None);
        profiles.items[1].layout.add(Kind::Weather);
        profiles.items[1].badge = Some(Badge::Label('W'));
        profiles.active = 1;
        profiles.write(&path).unwrap();

        let saved = Profiles::read(&path).unwrap();
        assert_eq!(saved.active, 1);
        assert_eq!(saved.items[0].layout, profiles.items[0].layout);
        assert_eq!(saved.items[0].layout.sensors[&9].chart, "memory");
        assert_eq!(saved.items[1].layout.widgets[0].kind, Kind::Weather);
        assert_eq!(saved.items[1].label(1), "W");
        assert_eq!(saved.items[4].label(4), "5");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn imports_self_contained_svg_and_png_badges() {
        let dir = std::env::temp_dir().join(format!(
            "secondscreen-profile-badges-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("badge.svg");
        fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff280a"/></svg>"##).unwrap();
        let png = dir.join("badge.png");
        image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 40, 10, 255]))
            .save(&png)
            .unwrap();

        for path in [&svg, &png] {
            let badge = import_profile_image(path).unwrap();
            let saved: Badge =
                serde_json::from_str(&serde_json::to_string(&badge).unwrap()).unwrap();
            fs::remove_file(path).unwrap();
            let rendered = saved.image().unwrap().unwrap();
            let pixels = rendered.as_bytes(0).unwrap();
            let midpoint = (pixels.len() / 8) * 4;
            assert_eq!(&pixels[midpoint..midpoint + 4], &[10, 40, 255, 255]);
        }
        fs::remove_dir(dir).unwrap();
    }
}
