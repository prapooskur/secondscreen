use std::{
    fs::{self, File},
    io::{Cursor, ErrorKind, Read, Write},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result, ensure};
use gpui::{
    Context, Div, ObjectFit, PathPromptOptions, RenderImage, Task, Window, div, img, prelude::*,
};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::config::{self, BackgroundSlideshow};

use super::{Dashboard, theme::*};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 8192;
const MAX_RENDER_SIDE: u32 = 4096;
const IMAGE_FILE: &str = "background-image";
const DEFAULT_INTERVAL_MINUTES: NonZeroU32 = NonZeroU32::new(6 * 60).unwrap();

struct Wallpaper {
    path: PathBuf,
    image: Arc<RenderImage>,
}

#[derive(Default)]
pub(super) struct Background {
    wallpaper: Option<Wallpaper>,
    rotation: Option<Task<()>>,
    pub(super) pending: bool,
    pub(super) unavailable: bool,
}

impl Background {
    pub(super) fn load(
        slideshow: Option<&BackgroundSlideshow>,
        cx: &mut Context<Dashboard>,
    ) -> Self {
        let folder = slideshow.map(|slideshow| slideshow.folder.clone());
        cx.spawn(async move |this, cx| {
            let result = blocking::unblock(move || match folder {
                Some(folder) => next_wallpaper(&folder, None).map(Some),
                None => load_saved(),
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                this.background.pending = false;
                match result {
                    Ok(wallpaper) => this.set_background(wallpaper, None, cx),
                    Err(error) => {
                        log::warn!("Could not load the background: {error:#}");
                        this.background.unavailable = true;
                    }
                }
                if let Some(slideshow) = this.preferences.background_slideshow.clone() {
                    this.background.start_rotation(slideshow, cx);
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
        self.wallpaper.is_some()
    }

    pub(super) fn element(&self, theme: Theme) -> Option<Div> {
        let image = self.wallpaper.as_ref()?.image.clone();
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

    fn start_rotation(&mut self, slideshow: BackgroundSlideshow, cx: &mut Context<Dashboard>) {
        let mut previous = self
            .wallpaper
            .as_ref()
            .map(|wallpaper| wallpaper.path.clone());
        let interval = Duration::from_secs(u64::from(slideshow.interval_minutes.get()) * 60);
        self.rotation = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let folder = slideshow.folder.clone();
                let current = previous.clone();
                let next =
                    blocking::unblock(move || next_wallpaper(&folder, current.as_deref())).await;
                if this
                    .update(cx, |this, cx| match next {
                        Ok(wallpaper) => {
                            previous = Some(wallpaper.path.clone());
                            this.set_background(Some(wallpaper), None, cx);
                            cx.notify();
                        }
                        Err(error) if !this.background.unavailable => {
                            log::warn!("Could not rotate the background: {error:#}");
                            this.background.unavailable = true;
                            cx.notify();
                        }
                        Err(_) => {}
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

impl Dashboard {
    pub(super) fn choose_background(&mut self, directory: bool, cx: &mut Context<Self>) {
        if self.background.pending {
            return;
        }
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some("Use as dashboard background".into()),
        });
        self.background.pending = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = async {
                let paths = result.await??;
                let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else {
                    return Ok(None);
                };
                blocking::unblock(move || {
                    let wallpaper = if directory {
                        next_wallpaper(&path, None)?
                    } else {
                        import_image(&path)?
                    };
                    Ok::<_, anyhow::Error>(Some((path, wallpaper)))
                })
                .await
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.background.pending = false;
                match result {
                    Ok(Some((folder, wallpaper))) if directory => {
                        this.set_background_folder(folder, wallpaper, cx);
                    }
                    Ok(Some((_, wallpaper))) => {
                        this.set_static_background(Some(wallpaper), None, cx);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        this.set_error(format!("Could not set the background: {error:#}"), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_background_folder(
        &mut self,
        folder: PathBuf,
        wallpaper: Wallpaper,
        cx: &mut Context<Self>,
    ) {
        let interval_minutes = self
            .preferences
            .background_slideshow
            .as_ref()
            .map_or(DEFAULT_INTERVAL_MINUTES, |slideshow| {
                slideshow.interval_minutes
            });
        let slideshow = BackgroundSlideshow {
            folder,
            interval_minutes,
        };
        let mut preferences = self.preferences.clone();
        preferences.background_slideshow = Some(slideshow.clone());
        if self.save_preferences(preferences, cx) {
            self.set_background(Some(wallpaper), None, cx);
            self.background.start_rotation(slideshow, cx);
        }
    }

    pub(super) fn set_background_interval(
        &mut self,
        minutes: NonZeroU32,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.background.pending {
            return false;
        }
        let mut preferences = self.preferences.clone();
        let Some(slideshow) = &mut preferences.background_slideshow else {
            return false;
        };
        if slideshow.interval_minutes == minutes {
            return true;
        }
        slideshow.interval_minutes = minutes;
        let slideshow = slideshow.clone();
        if !self.save_preferences(preferences, cx) {
            return false;
        }
        self.background.start_rotation(slideshow, cx);
        true
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
            Ok(()) => self.set_static_background(None, Some(window), cx),
            Err(error) => self.set_error(
                format!("Could not reset the background image: {error:#}"),
                cx,
            ),
        }
        cx.notify();
    }

    fn set_static_background(
        &mut self,
        wallpaper: Option<Wallpaper>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.background_slideshow.is_some() {
            let mut preferences = self.preferences.clone();
            preferences.background_slideshow = None;
            if !self.save_preferences(preferences, cx) {
                return;
            }
            self.background.rotation = None;
            self.settings.background_interval_field = None;
        }
        self.set_background(wallpaper, window, cx);
        self.error = None;
    }

    fn set_background(
        &mut self,
        wallpaper: Option<Wallpaper>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(previous) = std::mem::replace(&mut self.background.wallpaper, wallpaper) {
            cx.drop_image(previous.image, window);
        }
        self.background.unavailable = false;
    }
}

fn background_path() -> Result<PathBuf> {
    config::path(IMAGE_FILE).context("No settings directory is available.")
}

fn load_saved() -> Result<Option<Wallpaper>> {
    let Some(path) = config::path(IMAGE_FILE) else {
        return Ok(None);
    };
    match File::open(&path) {
        Ok(file) => read_image(file).map(|(_, image)| Some(Wallpaper { path, image })),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn import_image(path: &Path) -> Result<Wallpaper> {
    let destination = background_path()?;
    ensure!(path.is_file(), "Choose an image file.");
    let file = File::open(path).context("Could not open the selected image")?;
    let (bytes, image) = read_image(file)?;
    config::write_file(&destination, |file| {
        file.write_all(&bytes).map_err(Into::into)
    })
    .context("Could not save the background image")?;
    Ok(Wallpaper {
        path: destination,
        image,
    })
}

fn next_wallpaper(folder: &Path, previous: Option<&Path>) -> Result<Wallpaper> {
    let mut files = fs::read_dir(folder)
        .with_context(|| format!("Could not open {}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| ImageFormat::from_path(path).is_ok_and(supported_format) && path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    let start = previous.map_or(0, |previous| {
        files.partition_point(|path| path.as_path() <= previous)
    });
    files[start..]
        .iter()
        .chain(&files[..start])
        .find_map(|path| {
            let file = File::open(path).ok()?;
            let (_, image) = read_image(file).ok()?;
            Some(Wallpaper {
                path: path.clone(),
                image,
            })
        })
        .context("No readable PNG, JPEG, WebP, or GIF images were found in this folder.")
}

fn supported_format(format: ImageFormat) -> bool {
    matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif
    )
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
        reader.format().is_some_and(supported_format),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_skips_unreadable_images_and_wraps_in_filename_order() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path();
        let first = folder.join("01.PNG");
        let last = folder.join("03.png");
        let image = DynamicImage::new_rgb8(1, 1);
        image.save_with_format(&last, ImageFormat::Png).unwrap();
        image.save_with_format(&first, ImageFormat::Png).unwrap();
        fs::write(folder.join("02.jpg"), b"not an image").unwrap();

        assert_eq!(next_wallpaper(folder, None).unwrap().path, first);
        assert_eq!(next_wallpaper(folder, Some(&first)).unwrap().path, last);
        assert_eq!(next_wallpaper(folder, Some(&last)).unwrap().path, first);

        fs::remove_file(&first).unwrap();
        assert_eq!(next_wallpaper(folder, Some(&first)).unwrap().path, last);
        fs::remove_file(&last).unwrap();
        assert!(next_wallpaper(folder, Some(&last)).is_err());
    }
}
