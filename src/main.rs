#[cfg(all(
    target_os = "linux",
    feature = "ui-gpui-fast",
    not(feature = "ui-gpui")
))]
extern crate gpui_fast_backend as gpui;

#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod config;
#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod model;
#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod monitor;
#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod platform;
#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod ui;
#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
mod viewer;

#[cfg(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
))]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context as _, bail, ensure};
    use gpui::{App, AppContext, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size};
    use std::{borrow::Cow, ffi::OsStr};

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some_and(|arg| arg == "--viewer") {
        args.next();
        let source = args
            .next()
            .context("Provide a URL or iframe to --viewer.")?;
        ensure!(args.next().is_none(), "Pass the viewer one quoted source.");
        return viewer::run(viewer::Source::parse(&source)?);
    }

    let mut fullscreen = false;
    for arg in args {
        match arg.as_str() {
            "--fullscreen" => fullscreen = true,
            "--help" | "-h" => {
                println!(
                    "Second Screen\n\nUsage: secondscreen [--fullscreen]\n\nF11 toggles fullscreen. Escape closes the current overlay or restores the window."
                );
                return Ok(());
            }
            arg => bail!("Unknown option {arg}. Use --help for usage."),
        }
    }
    ensure!(
        ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .into_iter()
            .any(|name| std::env::var_os(name)
                .as_deref()
                .is_some_and(|value| value != OsStr::new(""))),
        "Second Screen needs a running Wayland session."
    );
    ensure!(
        gpui::guess_compositor().eq_ignore_ascii_case("wayland"),
        "Second Screen could not select the Wayland backend."
    );

    let preferences = config::Preferences::load();
    fullscreen |= preferences.start_fullscreen;
    let (updates, receiver) = async_channel::unbounded();
    let controls = platform::start(updates, preferences.discord.clone())
        .context("Could not start desktop services")?;
    #[cfg(feature = "ui-gpui")]
    let app = gpui::Application::new();
    #[cfg(not(feature = "ui-gpui"))]
    let app = gpui_platform::application();
    app.with_assets(ui::IconAssets).run(move |cx: &mut App| {
        if let Err(error) = cx.text_system().add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
        ]) {
            log::warn!("Could not register bundled fonts: {error}");
        }

        let bounds = Bounds::centered(None, size(px(1440.), px(432.)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(if fullscreen {
                    WindowBounds::Fullscreen(bounds)
                } else {
                    WindowBounds::Windowed(bounds)
                }),
                titlebar: None,
                app_id: Some(platform::APP_ID.into()),
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(740.), px(300.))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Second Screen");
                cx.new(|cx| {
                    ui::Dashboard::new(controls, receiver, preferences, fullscreen, window, cx)
                })
            },
        );
        match result {
            Ok(_) => {
                let on_close = |cx: &mut App| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                };
                #[cfg(feature = "ui-gpui")]
                cx.on_window_closed(on_close).detach();
                #[cfg(not(feature = "ui-gpui"))]
                cx.on_window_closed(move |cx, _| on_close(cx)).detach();
                cx.activate(true);
            }
            Err(error) => {
                eprintln!("Could not open the Wayland dashboard: {error:#}");
                std::process::exit(1);
            }
        }
    });
    Ok(())
}

#[cfg(not(all(
    target_os = "linux",
    any(feature = "ui-gpui", feature = "ui-gpui-fast")
)))]
fn main() {
    eprintln!(
        "Second Screen requires Linux Wayland and one of the ui-gpui or ui-gpui-fast features."
    );
    std::process::exit(1);
}
