use std::{
    ffi::{CString, c_char, c_int, c_uint, c_void},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use super::Source;
use anyhow::{Context, Result};
use gpui::RenderImage;

unsafe extern "C" {
    fn ss_web_new(
        uri: *const c_char,
        html: *const c_char,
        width: c_int,
        height: c_int,
        standalone: c_int,
    ) -> *mut c_void;
    fn ss_web_free(tile: *mut c_void);
    fn ss_web_run(tile: *mut c_void);
    fn ss_web_has_discord() -> c_int;
    fn ss_web_pump();
    fn ss_web_resize(tile: *mut c_void, width: c_int, height: c_int);
    fn ss_web_frame(tile: *mut c_void, bgra: *mut u8, width: c_int, height: c_int) -> c_int;
    fn ss_web_pointer(
        tile: *mut c_void,
        action: c_int,
        button: c_int,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        state: c_uint,
    );
    fn ss_web_key(
        tile: *mut c_void,
        name: *const c_char,
        unicode: u32,
        state: c_uint,
        press: c_int,
    );
    fn ss_web_focus(tile: *mut c_void, focus: c_int);
}

pub(super) fn has_discord() -> bool {
    unsafe { ss_web_has_discord() != 0 }
}

fn create(
    source: &Source,
    (width, height): (i32, i32),
    standalone: bool,
) -> Result<NonNull<c_void>> {
    let (uri, html) = source.document()?;
    let uri = CString::new(uri)?;
    let html = html.map(CString::new).transpose()?;
    NonNull::new(unsafe {
        ss_web_new(
            uri.as_ptr(),
            html.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            width,
            height,
            standalone.into(),
        )
    })
    .context("Could not initialize WebKit on this display.")
}

pub(super) fn standalone(source: &Source) -> Result<()> {
    let window = create(source, (1000, 560), true)?;
    unsafe { ss_web_run(window.as_ptr()) };
    Ok(())
}

// GTK objects and their GLib context belong to the GPUI main thread.
pub(crate) struct WebTile {
    view: NonNull<c_void>,
    size: (i32, i32),
    pixels: Vec<u8>,
    _main_thread: PhantomData<Rc<()>>,
}

impl WebTile {
    pub fn new(source: &Source, size: (i32, i32)) -> Result<Self> {
        Ok(Self {
            view: create(source, size, false)?,
            size,
            pixels: Vec::new(),
            _main_thread: PhantomData,
        })
    }

    pub fn update(
        &mut self,
        size: (i32, i32),
        previous: Option<&RenderImage>,
        visible: bool,
    ) -> Option<Arc<RenderImage>> {
        unsafe { ss_web_pump() };
        if !visible {
            return None;
        }
        let (w, h) = size;
        let resized = self.size != size;
        if resized {
            unsafe { ss_web_resize(self.view.as_ptr(), w, h) };
            self.size = size;
        }
        // GTK suppresses damage notifications for an unmapped host, including
        // changes from a cross-origin frame. Sample the actual web surface.
        self.pixels.resize(w as usize * h as usize * 4, 0);
        if unsafe { ss_web_frame(self.view.as_ptr(), self.pixels.as_mut_ptr(), w, h) } == 0 {
            return None;
        }
        #[cfg(target_endian = "big")]
        for p in self.pixels.chunks_exact_mut(4) {
            p.reverse();
        }
        if !resized && previous.and_then(|frame| frame.as_bytes(0)) == Some(self.pixels.as_slice())
        {
            return None;
        }
        let frame = image::RgbaImage::from_raw(w as u32, h as u32, self.pixels.clone())?;
        Some(Arc::new(RenderImage::new([image::Frame::new(frame)])))
    }

    pub fn pointer(
        &self,
        action: i32,
        button: i32,
        (x, y): (f64, f64),
        delta: (f64, f64),
        state: u32,
    ) {
        unsafe {
            ss_web_pointer(
                self.view.as_ptr(),
                action,
                button,
                x,
                y,
                delta.0,
                delta.1,
                state,
            )
        };
    }
    pub fn key(&self, name: &str, unicode: u32, state: u32, press: bool) {
        if let Ok(name) = CString::new(name) {
            unsafe {
                ss_web_key(
                    self.view.as_ptr(),
                    name.as_ptr(),
                    unicode,
                    state,
                    press.into(),
                )
            }
        }
    }
    pub fn focus(&self, focused: bool) {
        unsafe { ss_web_focus(self.view.as_ptr(), focused.into()) }
    }
}

impl Drop for WebTile {
    fn drop(&mut self) {
        unsafe { ss_web_free(self.view.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::{ViewerMode, ViewerOptions};
    use std::{
        fs,
        time::{Duration, Instant},
    };

    #[test]
    #[ignore = "requires a graphical session with WebKitGTK"]
    fn inline_slideshow_repaints_after_resize_and_accepts_clicks() {
        let dir =
            std::env::temp_dir().join(format!("secondscreen-inline-slides-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for (name, color) in [("1.png", [255, 0, 0, 255]), ("2.png", [0, 0, 255, 255])] {
            image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
                .save(dir.join(name))
                .unwrap();
        }
        let source = ViewerOptions {
            mode: ViewerMode::Slideshow,
            source: dir.to_string_lossy().into_owned(),
            interval: 3600,
        }
        .resolve()
        .unwrap();
        let mut tile = WebTile::new(&source, (320, 180)).unwrap();
        let mut previous = None;
        let red = [0, 0, 255, 255];
        let blue = [255, 0, 0, 255];
        for (size, expected, advance) in [
            ((320, 180), red, true),
            ((320, 180), blue, false),
            ((280, 220), blue, true),
            ((280, 220), red, false),
        ] {
            let deadline = Instant::now() + Duration::from_secs(12);
            loop {
                if let Some(frame) = tile.update(size, previous.as_deref(), true) {
                    let center = ((size.1 / 2 * size.0 + size.0 / 2) * 4) as usize;
                    let found = frame.as_bytes(0).unwrap()[center..center + 4] == expected;
                    previous = Some(frame);
                    if found {
                        break;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "inline view did not display {expected:?} at {size:?}"
                );
                std::thread::sleep(Duration::from_millis(30));
            }
            if advance {
                let next = (f64::from(size.0) - 15., 15.);
                tile.focus(true);
                tile.pointer(0, 0, next, (0., 0.), 0);
                tile.pointer(1, 1, next, (0., 0.), 0);
                tile.pointer(2, 1, next, (0., 0.), 1 << 8);
            }
        }
        drop(tile);
        fs::remove_dir_all(dir).unwrap();
    }
}
