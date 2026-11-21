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
    fn ss_web_dirty(tile: *mut c_void) -> c_int;
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
    _main_thread: PhantomData<Rc<()>>,
}

impl WebTile {
    pub fn new(source: &Source, size: (i32, i32)) -> Result<Self> {
        Ok(Self {
            view: create(source, size, false)?,
            size,
            _main_thread: PhantomData,
        })
    }

    pub fn update(&mut self, size: (i32, i32)) -> Option<Arc<RenderImage>> {
        let (w, h) = size;
        if self.size != size {
            unsafe { ss_web_resize(self.view.as_ptr(), w, h) };
            self.size = size;
        }
        unsafe { ss_web_pump() };
        if unsafe { ss_web_dirty(self.view.as_ptr()) } == 0 {
            return None;
        }
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        if unsafe { ss_web_frame(self.view.as_ptr(), pixels.as_mut_ptr(), w, h) } == 0 {
            return None;
        }
        #[cfg(target_endian = "big")]
        for p in pixels.chunks_exact_mut(4) {
            p.reverse();
        }
        let frame = image::RgbaImage::from_raw(w as u32, h as u32, pixels)?;
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
