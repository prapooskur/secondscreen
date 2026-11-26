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
use unicode_segmentation::UnicodeSegmentation;

use crate::config;

use super::{
    super::{
        Dashboard, Tooltip,
        icons::{Icon, ProfileIcon},
        overlay::Overlay,
        theme::*,
    },
    Home, Layout, ProfilePreset, RAIL_BUTTON_SIZE, RAIL_WIDTH, rail_control,
};

pub(in crate::ui) const MAX_PROFILES: usize = 5;
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
    #[serde(flatten)]
    pub details: ProfileDetails,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(in crate::ui) struct ProfileDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<Badge>,
    #[serde(skip)]
    pub image: Option<Arc<RenderImage>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ui) enum Badge {
    #[serde(rename = "label", skip_serializing)]
    LegacyLabel(char),
    Preset(ProfileIcon),
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
            let details = &mut profile.details;
            match &details.badge {
                Some(Badge::LegacyLabel(label)) => {
                    if label.is_alphanumeric()
                        && details
                            .name
                            .as_deref()
                            .is_none_or(|name| name.trim().is_empty())
                    {
                        details.name = Some(label.to_string());
                    }
                    details.badge = None;
                }
                Some(Badge::Png(encoded)) => match decode_profile_image(encoded) {
                    Ok(image) => details.image = Some(image),
                    Err(error) => log::warn!("Could not load a profile image: {error:#}"),
                },
                _ => {}
            }
        }
        Ok(profiles)
    }

    fn add(&mut self, preset: ProfilePreset) -> Option<usize> {
        let index = self.items.len();
        if index >= MAX_PROFILES {
            return None;
        }
        self.items.push(preset.build());
        Some(index)
    }

    fn remove(&mut self, index: usize) -> Option<Profile> {
        if self.items.len() <= 1 || index >= self.items.len() {
            return None;
        }
        let profile = self.items.remove(index);
        self.active = if index < self.active {
            self.active - 1
        } else {
            self.active.min(self.items.len() - 1)
        };
        Some(profile)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        config::write_json(path, self).context("Could not save the Home profiles")
    }
}

impl Profile {
    fn new(layout: Layout) -> Self {
        Self {
            layout,
            details: ProfileDetails::default(),
        }
    }

    pub(in crate::ui) fn name(&self, index: usize) -> String {
        self.details
            .name
            .clone()
            .unwrap_or_else(|| format!("Profile {}", index + 1))
    }

    pub(in crate::ui) fn button_content(
        &self,
        index: usize,
        theme: Theme,
        size: f32,
    ) -> impl IntoElement {
        Badge::content(
            self.details.badge.as_ref(),
            self.details.image.as_ref(),
            self.details.name.as_deref(),
            index,
            theme,
            size,
        )
    }
}

impl Badge {
    pub(in crate::ui) fn content(
        badge: Option<&Self>,
        image: Option<&Arc<RenderImage>>,
        name: Option<&str>,
        index: usize,
        theme: Theme,
        size: f32,
    ) -> gpui::AnyElement {
        match (badge, image) {
            (Some(Self::Preset(icon)), _) => icon.element(theme, size).into_any_element(),
            (Some(Self::Png(_)), Some(image)) => {
                img(image.clone()).size(r(size)).into_any_element()
            }
            _ => mono(profile_label(name, index))
                .text_size(r(size * 0.67))
                .into_any_element(),
        }
    }
}

fn profile_label(name: Option<&str>, index: usize) -> String {
    name.and_then(|name| name.trim().graphemes(true).next())
        .map_or_else(|| (index + 1).to_string(), str::to_owned)
}

fn decode_profile_image(encoded: &str) -> Result<Arc<RenderImage>> {
    ensure!(
        encoded.len() <= 128 * 1024,
        "Profile icon data is too large."
    );
    let bytes = STANDARD.decode(encoded)?;
    let mut image = read_png(&bytes, ICON_SIZE)?.into_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(image)])))
}

pub(in crate::ui) fn import_profile_image(path: &Path) -> Result<(Badge, Arc<RenderImage>)> {
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
    let encoded = STANDARD.encode(png);
    let image = decode_profile_image(&encoded)?;
    Ok((Badge::Png(encoded), image))
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

impl Home {
    pub(in crate::ui) fn profiles(&self) -> &[Profile] {
        &self.profiles.items
    }
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
                let name = profile.name(index);
                let description = if active {
                    format!("{name} (active)")
                } else {
                    format!("Switch to {name}")
                };
                theme.bind(
                    theme
                        .button(("switch-profile", index), active.then_some(TEAL))
                        .size(r(RAIL_BUTTON_SIZE))
                        .px_0()
                        .child(profile.button_content(index, theme, 24.))
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
                            this.show_profile_presets(window, cx);
                        })
                    },
                ))
            })
            .child(col().flex_1())
            .child(rail_control(
                theme,
                "edit-profile",
                "Edit profile",
                Icon::Edit,
                false,
                move |window, cx| {
                    edit.update(cx, |this, cx| {
                        this.show_overlay(Overlay::Profile, window, cx)
                    })
                },
            ))
    }

    pub(in crate::ui) fn switch_profile(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.home.profiles.active == index || index >= self.home.profiles.items.len() {
            return;
        }
        let previous = std::mem::replace(&mut self.home.profiles.active, index);
        if self.save_home(cx) {
            self.reset_active_profile(window, cx);
        } else {
            self.home.profiles.active = previous;
        }
    }

    pub(in crate::ui) fn add_profile(
        &mut self,
        preset: ProfilePreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let index = self.home.profiles.add(preset)?;
        self.error = None;
        self.switch_profile(index, window, cx);
        if self.home.profiles.active == index {
            Some(index)
        } else {
            self.home.profiles.items.pop();
            None
        }
    }

    pub(in crate::ui) fn delete_profile(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = self.home.profiles.active;
        let Some(profile) = self.home.profiles.remove(index) else {
            return false;
        };
        if !self.save_home(cx) {
            self.home.profiles.items.insert(index, profile);
            self.home.profiles.active = previous;
            return false;
        }
        if let Some(image) = profile.details.image {
            cx.drop_image(image, Some(window));
        }
        if index == previous {
            self.reset_active_profile(window, cx);
        }
        true
    }

    fn reset_active_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.home.profile_revision += 1;
        self.home.leave();
        self.home.scroll = ScrollHandle::new();
        self.home_assistant.dismiss_picker();
        self.viewers.clear();
        super::super::monitor::discard_charts(None, window, cx);
        self.gesture.borrow_mut().take();
        if self.overlay.open.is_some() {
            self.focus_overlay(window, cx);
        } else {
            self.focus_dashboard(window, cx);
        }
    }

    pub(in crate::ui) fn set_profile_details(
        &mut self,
        index: usize,
        details: ProfileDetails,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(profile) = self.home.profiles.items.get_mut(index) else {
            return false;
        };
        let previous = std::mem::replace(&mut profile.details, details);
        self.error = None;
        if !self.save_home(cx) {
            self.home.profiles.items[index].details = previous;
            return false;
        }
        let new_image = self.home.profiles.items[index].details.image.as_ref();
        if let Some(old) = previous.image
            && new_image.is_none_or(|new| !Arc::ptr_eq(&old, new))
        {
            cx.defer(move |cx| cx.drop_image(old, None));
        }
        true
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
        fs::write(&path, r#"{"widgets":[{"kind":"performance","id":9,"columns":2,"rows":5}],"sensors":{"9":{"mode":"chart","chart":"memory"}}}"#).unwrap();
        let mut profiles = Profiles::read(&path).unwrap();
        assert_eq!(profiles.active, 0);
        assert_eq!(profiles.items.len(), 1);
        for index in 1..MAX_PROFILES {
            assert_eq!(profiles.add(ProfilePreset::BLANK), Some(index));
            assert!(profiles.items[index].layout.widgets.is_empty());
        }
        assert_eq!(profiles.add(ProfilePreset::BLANK), None);
        profiles.items[1].layout.add(Kind::Weather);
        profiles.items[1].details.name = Some("Weather".into());
        profiles.items[2].details.name = Some("Studio".into());
        profiles.items[2].details.badge = Some(Badge::Preset(ProfileIcon::Music));
        profiles.active = 1;
        profiles.write(&path).unwrap();

        let saved = Profiles::read(&path).unwrap();
        assert_eq!(saved.active, 1);
        assert_eq!(saved.items[0].layout, profiles.items[0].layout);
        assert_eq!(
            saved.items[0].layout.monitors[&9].readings[0].source,
            crate::ui::home::options::MonitorSource::Sensor("memory".into())
        );
        assert_eq!(saved.items[1].layout.widgets[0].kind, Kind::Weather);
        assert_eq!(saved.items[1].name(1), "Weather");
        assert_eq!(saved.items[1].details.badge, None);
        assert_eq!(saved.items[2].name(2), "Studio");
        assert_eq!(
            saved.items[2].details.badge,
            Some(Badge::Preset(ProfileIcon::Music))
        );
        assert_eq!(saved.items[4].name(4), "Profile 5");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn presets_keep_their_layout_and_widget_options_when_reopened() {
        use crate::ui::home::layout::{MAX_COLUMNS, place};
        use crate::viewer::ViewerMode;

        let file = tempfile::NamedTempFile::new().unwrap();
        for (position, preset) in ProfilePreset::ALL.into_iter().enumerate() {
            let mut profiles = Profiles::default();
            profiles.active = profiles.add(preset).unwrap();
            profiles.write(file.path()).unwrap();

            let restored = Profiles::read(file.path()).unwrap();
            assert_eq!(restored.active, 1);
            assert_eq!(restored.items[0].layout, profiles.items[0].layout);
            let profile = &restored.items[1];
            assert_eq!(profile.layout, profiles.items[1].layout);
            assert_eq!(profile.layout.widgets.is_empty(), position == 0);
            assert_eq!(profile.details.badge, preset.icon.map(Badge::Preset));
            assert_eq!(
                profile.name(1),
                if position == 0 {
                    "Profile 2"
                } else {
                    preset.name
                }
            );

            let placements = place(profile.layout.widgets.iter().copied(), MAX_COLUMNS);
            assert!(
                placements
                    .iter()
                    .all(|tile| tile.x + usize::from(tile.columns) <= usize::from(MAX_COLUMNS))
            );
            for tile in &profile.layout.widgets {
                if tile.kind == Kind::SystemMonitor {
                    assert!(!profile.layout.monitors[&tile.id].readings.is_empty());
                }
            }
            if preset.name == "Stream Companion" {
                let viewers = &profile.layout.viewers;
                assert_eq!(viewers.len(), 2);
                for (tile, mode) in profile
                    .layout
                    .widgets
                    .iter()
                    .zip([ViewerMode::TwitchLive, ViewerMode::TwitchChat])
                {
                    assert_eq!(tile.kind, Kind::Viewer);
                    assert_eq!(viewers[&tile.id].mode, mode);
                    assert!(viewers[&tile.id].source.is_empty());
                }
            }
        }
    }

    #[test]
    fn migrates_legacy_labels_into_unnamed_profiles() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            file.path(),
            r#"{"profiles":[
                {"layout":{"widgets":[]},"badge":{"label":"W"}},
                {"layout":{"widgets":[]},"name":"Studio","badge":{"label":"M"}}
            ]}"#,
        )
        .unwrap();
        let profiles = Profiles::read(file.path()).unwrap();
        assert_eq!(profiles.items[0].name(0), "W");
        assert_eq!(profiles.items[1].name(1), "Studio");

        profiles.write(file.path()).unwrap();
        let document: serde_json::Value = config::read_json(file.path()).unwrap();
        for profile in document["profiles"].as_array().unwrap() {
            assert!(profile.get("badge").is_none());
        }
    }

    #[test]
    fn derives_the_text_badge_from_the_first_character() {
        assert_eq!(profile_label(Some("Studio"), 0), "S");
        assert_eq!(profile_label(Some("  winter"), 0), "w");
        assert_eq!(profile_label(Some("e\u{301}criture"), 0), "e\u{301}");
        assert_eq!(profile_label(Some("👩‍💻 Work"), 0), "👩‍💻");
        assert_eq!(profile_label(Some(" "), 1), "2");
        assert_eq!(profile_label(None, 4), "5");
    }

    #[test]
    fn removes_profiles_and_preserves_the_active_selection() {
        let mut profiles = Profiles::default();
        for (index, name) in ["Studio", "Games", "Desk"].into_iter().enumerate() {
            assert_eq!(profiles.add(ProfilePreset::BLANK), Some(index + 1));
            profiles.items[index + 1].details.name = Some(name.into());
        }
        profiles.active = 2;
        assert!(profiles.remove(10).is_none());

        // Deleting before the active profile shifts its index but keeps it active.
        assert_eq!(profiles.remove(0).unwrap().name(0), "Profile 1");
        assert_eq!(profiles.active, 1);
        assert_eq!(profiles.items[profiles.active].name(1), "Games");

        // Deleting the active profile selects the next one, or the previous one at the end.
        assert_eq!(profiles.remove(1).unwrap().name(1), "Games");
        assert_eq!(profiles.active, 1);
        assert_eq!(profiles.items[profiles.active].name(1), "Desk");
        assert_eq!(profiles.remove(1).unwrap().name(1), "Desk");
        assert_eq!(profiles.active, 0);
        assert_eq!(profiles.items[profiles.active].name(0), "Studio");
        assert!(profiles.remove(0).is_none());

        let file = tempfile::NamedTempFile::new().unwrap();
        profiles.write(file.path()).unwrap();
        let restored = Profiles::read(file.path()).unwrap();
        assert_eq!(restored.active, 0);
        assert_eq!(restored.items.len(), 1);
        assert_eq!(restored.items[0].name(0), "Studio");
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
            let (badge, _) = import_profile_image(path).unwrap();
            let saved: Badge =
                serde_json::from_str(&serde_json::to_string(&badge).unwrap()).unwrap();
            fs::remove_file(path).unwrap();
            let Badge::Png(encoded) = saved else {
                panic!("Imported image was not saved as PNG")
            };
            let rendered = decode_profile_image(&encoded).unwrap();
            let pixels = rendered.as_bytes(0).unwrap();
            let midpoint = (pixels.len() / 8) * 4;
            assert_eq!(&pixels[midpoint..midpoint + 4], &[10, 40, 255, 255]);
        }
        fs::remove_dir(dir).unwrap();
    }
}
