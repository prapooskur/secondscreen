use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::PathBuf,
    ptr,
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::app_icons;
use crate::platform::in_flatpak;

pub const UNAVAILABLE: &str = "App shortcuts require a native package.";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
}

unsafe extern "C" {
    fn secondscreen_desktop_apps(
        callback: extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char),
        data: *mut c_void,
    );
    fn secondscreen_launch_desktop_app(
        id: *const c_char,
        callback: extern "C" fn(*mut c_void, *const c_char),
        data: *mut c_void,
    );
}

pub fn installed() -> Vec<DesktopApp> {
    let theme = freedesktop_icons::default_theme_gtk().unwrap_or_else(|| "hicolor".into());
    let mut catalog = (theme, Vec::<DesktopApp>::new());
    // The bridge borrows the catalog synchronously and lends each entry's strings to collect.
    unsafe { secondscreen_desktop_apps(collect, ptr::from_mut(&mut catalog).cast()) };
    let (_, mut apps) = catalog;
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
    apps
}

extern "C" fn collect(
    data: *mut c_void,
    id: *const c_char,
    name: *const c_char,
    icon: *const c_char,
) {
    if id.is_null() || name.is_null() {
        return;
    }
    let (Ok(id), Ok(name)) = (
        unsafe { CStr::from_ptr(id) }.to_str(),
        unsafe { CStr::from_ptr(name) }.to_str(),
    ) else {
        return;
    };
    let (theme, apps) = unsafe { &mut *data.cast::<(String, Vec<DesktopApp>)>() };
    let icon = (!icon.is_null())
        .then(|| unsafe { CStr::from_ptr(icon) }.to_str().ok())
        .flatten()
        .filter(|icon| !icon.is_empty())
        .and_then(|icon| app_icons::lookup(icon, theme));
    apps.push(DesktopApp {
        id: id.into(),
        name: name.into(),
        icon,
    });
}

pub fn launch(app: &DesktopApp) -> Result<()> {
    ensure!(!in_flatpak(), "{UNAVAILABLE}");
    let id = CString::new(app.id.as_str()).context("Invalid desktop app ID.")?;
    let mut error = None::<String>;
    // The bridge returns only after launch has completed and copies any error through the callback.
    unsafe {
        secondscreen_launch_desktop_app(
            id.as_ptr(),
            collect_error,
            ptr::from_mut(&mut error).cast(),
        );
    }
    if let Some(error) = error {
        bail!("Could not launch {}. {error}", app.name);
    }
    Ok(())
}

extern "C" fn collect_error(data: *mut c_void, error: *const c_char) {
    let message = unsafe { &mut *data.cast::<Option<String>>() };
    *message = Some(
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned(),
    );
}
