use std::{
    fs::{self, File},
    io::{Cursor, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, ensure};
use gpui::{Context, Div, ObjectFit, PathPromptOptions, RenderImage, Window, div, img, prelude::*};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::config;

use super::{Dashboard, theme::*};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 8192;
const MAX_RENDER_SIDE: u32 = 4096;
const IMAGE_FILE: &str = "background-image";

#[derive(Default)]
pub(super) struct Background {
    image: Option<Arc<RenderImage>>,
    pub(super) pending: bool,
    pub(super) unavailable: bool,
}

impl Background {
    pub(super) fn load(cx: &mut Context<Dashboard>) -> Self {
        let Some(path) = config::path(IMAGE_FILE) else {
            return Self::default();
        };
        cx.spawn(async move |this, cx| {
            let result = blocking::unblock(move || load_saved(&path)).await;
            let _ = this.update(cx, |this, cx| {
                this.background.pending = false;
                match result {
                    Ok(image) => this.background.image = image,
                    Err(error) => {
                        log::warn!("Could not load the background image: {error:#}");
                        this.background.unavailable = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            pending: true,
            ..Self::default()
        }
    }

    pub(super) fn has_image(&self) -> bool {
        self.image.is_some()
    }

    pub(super) fn element(&self, theme: Theme) -> Option<Div> {
        let image = self.image.clone()?;
        let tint = match theme {
            Theme::Dark => 0.26,
            Theme::Light => 0.30,
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    img(image)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(theme.color(BACK).opacity(tint)),
                ),
        )
    }
}

impl Dashboard {
    pub(super) fn choose_background(&mut self, cx: &mut Context<Self>) {
        if self.background.pending {
            return;
        }
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as dashboard background".into()),
        });
        self.background.pending = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match result.await {
                Ok(Ok(paths)) => match paths.and_then(|paths| paths.into_iter().next()) {
                    Some(path) => blocking::unblock(move || import_image(path))
                        .await
                        .map(Some),
                    None => Ok(None),
                },
                Ok(Err(error)) => Err(error),
                Err(error) => Err(error.into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.background.pending = false;
                match result {
                    Ok(Some(image)) => this.set_background(Some(image), None, cx),
                    Ok(None) => {}
                    Err(error) => {
                        this.set_error(format!("Could not set the background image: {error:#}"), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn reset_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.background.pending {
            return;
        }
        let result = background_path().and_then(|path| match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        });
        match result {
            Ok(()) => self.set_background(None, Some(window), cx),
            Err(error) => self.set_error(
                format!("Could not reset the background image: {error:#}"),
                cx,
            ),
        }
        cx.notify();
    }

    fn set_background(
        &mut self,
        image: Option<Arc<RenderImage>>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(previous) = std::mem::replace(&mut self.background.image, image) {
            cx.drop_image(previous, window);
        }
        self.background.unavailable = false;
        self.error = None;
    }
}

fn background_path() -> Result<PathBuf> {
    config::path(IMAGE_FILE).context("No settings directory is available.")
}

fn load_saved(path: &Path) -> Result<Option<Arc<RenderImage>>> {
    match File::open(path) {
        Ok(file) => read_image(file).map(|(_, image)| Some(image)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn import_image(path: PathBuf) -> Result<Arc<RenderImage>> {
    let destination = background_path()?;
    ensure!(path.is_file(), "Choose an image file.");
    let file = File::open(path).context("Could not open the selected image")?;
    let (bytes, image) = read_image(file)?;
    config::write_file(&destination, |file| {
        file.write_all(&bytes).map_err(Into::into)
    })
    .context("Could not save the background image")?;
    Ok(image)
}

fn read_image(file: File) -> Result<(Vec<u8>, Arc<RenderImage>)> {
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "Choose an image up to 64 MiB."
    );

    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    ensure!(
        matches!(
            reader.format(),
            Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif)
        ),
        "Choose a PNG, JPEG, WebP, or GIF image."
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits.clone());
    let mut decoder = reader
        .into_decoder()
        .context("Could not decode the image")?;
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder).context("Could not decode the image")?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_RENDER_SIDE {
        image = image.thumbnail(MAX_RENDER_SIDE, MAX_RENDER_SIDE);
    }
    let mut image = image.into_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok((
        bytes,
        Arc::new(RenderImage::new([image::Frame::new(image)])),
    ))
}
