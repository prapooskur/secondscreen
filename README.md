# Second Screen

Second Screen is a Rust and GPUI dashboard for ultrawide secondary monitors. It runs as a native Wayland window on Linux. Windows integration is deferred.

The app opens to a single resizable widget grid. Use **Edit layout** in the right-hand rail to choose the widgets on your display. **Settings** opens over the grid. Click outside it or press **Esc** to return to the dashboard.

- **Performance** shows selected sensors as a chart, a pair, or a list. **Memory** and **Network** show live system readings and usage history. **Storage** shows each drive's capacity and the space used by mounted filesystems that can be attributed to that drive.
- **Now playing** shows track details, album art, and playback controls. When multiple players are available, choose **Edit layout** and click the player name at the top of the widget to switch. Make the widget at least three rows tall to see the seek bar.
- **Audio** adjusts device volume and mute and lets you set the default output or microphone. Add **App mixer** to adjust playback and recording streams using app icons, mute buttons, and volume sliders. Hover over an icon to see the app and stream names.
- **Discord** shows a green ring around each speaker’s avatar and a **LIVE** badge when the desktop client reports a Go Live stream. The widget also provides controls to connect, mute, deafen, and leave a call.
- **Viewer** displays web pages, iframes, YouTube, Twitch, and local media inside the grid. Discord voice channel links open in Discord.

## Install on Linux

Open the **Linux packages** run for your commit under **Actions** on GitHub. Every push builds x86-64 packages with the default GPUI backend. Download and unzip the artifact for your distribution, then install the package inside it.

| Package | Build environment | Install |
| --- | --- | --- |
| DEB | Ubuntu 24.04 | `sudo apt install ./*.deb` |
| RPM | Fedora 44 | `sudo dnf install ./*.rpm` |
| Flatpak | GNOME 51 runtime from Flathub | `flatpak install --user ./*.flatpak` |

Launch **Second Screen** from your app menu in a Wayland session. Flatpak keeps its settings separately under `~/.var/app/io.github.secondscreen`. The sandbox limits host integrations such as login launchers, compositor control, desktop notification monitoring, and FPS feeds from host games.

### Build packages locally

For a DEB, install the Debian or Ubuntu build dependencies below and `dpkg-dev`. For an RPM on Fedora, install `gcc`, `gcc-c++`, `make`, `pkgconf-pkg-config`, `fontconfig-devel`, `freetype-devel`, `wayland-devel`, `libxkbcommon-devel`, `webkit2gtk4.1-devel`, and `evolution-data-server-devel`. Run the commands for your format from the repository root.

```sh
# DEB writes to target/debian/
cargo install --locked cargo-deb --version 3.8.0
cargo build --release --locked
cargo deb --locked --no-build

# RPM writes to target/generate-rpm/
cargo install --locked cargo-generate-rpm --version 0.21.1
CARGO_PROFILE_RELEASE_STRIP=symbols cargo build --release --locked
cargo generate-rpm

# Flatpak requires flatpak, flatpak-builder, and Cargo, and writes to target/packaging/
bash packaging/build-flatpak.sh
```

## Run on Linux

Build with Rust 1.95 or newer and keep the provided Cargo lockfile. On Debian or Ubuntu, install the build and audio dependencies:

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1 pulseaudio-utils wl-clipboard \
  libwebkit2gtk-4.1-dev libecal2.0-dev gstreamer1.0-plugins-good gstreamer1.0-libav
cargo run --release --locked
```

The Wayland session needs a working Vulkan driver. The viewer builds against GTK 3 and WebKitGTK 4.1 (`gtk3-devel` and `webkit2gtk4.1-devel` on Fedora). It loads those libraries when a web view opens and uses the system GStreamer codecs for playback. The audio mixer uses `pactl` with a PipeWire PulseAudio server or PulseAudio 15+. Media discovery uses the session D-Bus and works with players that advertise MPRIS. The Steam launch options **Copy** button uses `wl-clipboard` for mouse clicks in the default build.

Drag the grip on the right-hand rail to move the window to your bar monitor. Press **F11** or click the fullscreen button near the bottom of the rail. **Esc** dismisses Settings or the active editing controls before leaving fullscreen. **Tab** moves between controls, **Enter** activates buttons, and arrow keys adjust a focused slider. Enable **Start fullscreen** in **Settings → General** or pass `--fullscreen` to open fullscreen.

In **Settings → General**, enable **Start at login** to register the current executable with your desktop’s XDG autostart service. Your session must process XDG autostart entries. If you move the executable, toggle **Start at login** off and back on to update its path.

Use **Launch display** in **Settings → General** to choose a connected output on Sway or KDE Plasma 6 on Wayland. The selection applies to both windowed and fullscreen launches. If the display is disconnected, Second Screen opens on the desktop’s default output and keeps your selection. On GNOME, the desktop chooses the launch display.

The same app can also be built with GPUI Fast:

```sh
cargo run --release --locked --no-default-features --features ui-gpui-fast
```

## Customize Home

Choose **Edit layout** to add widgets from the catalog and show widget names and controls at the top. Use **×** to remove a widget or the arrows to change its order. Drag a widget’s bottom-right corner to resize. Use the width and height controls for single steps, or focus the corner and press the arrow keys. Choose **Done** to return to Home.

Use the left rail to switch profiles. **+** creates an empty profile and selects it, up to five total. The existing Home layout becomes profile 1. Each profile saves its own layout and widget options. Select the pencil on the left rail to give the active profile a single letter, a number, or an SVG or PNG image. Secondscreen imports a copy of the image and remembers which profile was selected.

Home fits four grid rows and up to five columns in the visible area. Widgets can span two to four visible rows. When the grid extends to the right, scroll horizontally with a trackpad, use **Shift** + mouse wheel, or use the arrows in the right-hand rail. The app keeps each widget’s requested size when space is limited and saves the layout in `${XDG_CONFIG_HOME:-~/.config}/secondscreen/home.json`.

## Configure Performance

In **Edit layout**, choose **Sensors** on a Performance tile. **Chart** plots one sensor over the last 60 samples. **2 sensors** shows two readings, each with its own units and graph scale. Select a slot, then choose its sensor. **List** lets you toggle individual readings. Choose **Done** on the tile to return to the live view.

The picker includes CPU and per-core load, memory, GPU load and VRAM where the graphics driver reports them, and Linux hardware-monitor channels such as temperatures, fan speeds, voltages, and power. Add Performance again to keep a chart and a sensor list visible together. Each tile saves its selections and size independently.

## Watch video and streams

Add **Viewer** from **Edit layout → Add widget**. Select a source and choose **Show**. Web pages, iframe embeds, YouTube video, live and playlist links play inside the tile. **Twitch Live** and **Twitch Chat** accept a channel name or channel URL. Add another Viewer to keep a stream and its chat side by side. In **Edit layout**, use **Source** to change the tile or open a restricted embed in your browser.

**Image / video** accepts a local media file. **Slideshow** accepts a folder and displays supported files in filename order. The delay controls set how long each image stays on screen. Videos advance at the end of playback. Use the arrows on the slideshow to move between items. The dashboard restores each saved source when it opens.

Paste a Discord voice channel link into the Viewer. The dashboard leaves fullscreen to show the Discord desktop client or its web client. Choose **Watch Stream** on the live participant in Discord.

## Discord setup

For client-secret login, add exactly `http://127.0.0.1` under **OAuth2 → Redirects** for your application in the Discord Developer Portal. Discord desktop uses the registered redirect and returns the authorization code over RPC. Second Screen sends the URI to Discord's token endpoint when exchanging the code.

Discord voice RPC requires an approved Discord application or access as an authorized tester of the application. Open Discord desktop with the authorized account. In **Settings → Discord**, enter your application ID and its client secret, then save and click **Connect** in the Discord widget to authorize access. Add it from **Edit layout** if needed. You can supply a Discord OAuth access token with the `identify`, `rpc`, `rpc.voice.read`, and `rpc.voice.write` scopes in place of the client secret. Second Screen saves OAuth authorization in the desktop keyring through Secret Service and reconnects when Discord opens. It refreshes expired tokens using the saved grant. On desktops without a working keyring, OAuth credentials obtained during authorization last until the app exits. Discord can restore prior voice settings when the RPC connection closes. [Discord RPC documentation](https://docs.discord.com/developers/topics/rpc)

Use the **Media apps**, **Audio devices**, and **Storage** sections of Settings to choose which sources appear in the widgets. Settings saves credentials and visibility preferences to the user-only file `${XDG_CONFIG_HOME:-~/.config}/secondscreen/settings.json`. Existing deployments can still use `SECONDSCREEN_DISCORD_CLIENT_ID` with `SECONDSCREEN_DISCORD_CLIENT_SECRET` or `SECONDSCREEN_DISCORD_ACCESS_TOKEN`. Saved Discord credentials take precedence.

Storage lists whole drives exposed by Linux, including unmounted drives. It shows usage when a mounted filesystem can be attributed to one drive. Storage visibility is now saved per drive, so select your drives in Settings after upgrading from mount-level visibility.

## Framework comparison

See [BENCHMARK.md](BENCHMARK.md) for the performance results and reproduction steps.

## Clock, calendar, weather, notifications, and FPS

Add these tiles from **Edit layout → Add widget**. Open **Settings → Widgets** to change time zones, weather location, or the weather units and time format.

- **World clock** shows local time and your chosen IANA time zones. Enter names such as `America/New_York, Asia/Kolkata`. Each clock follows its zone’s daylight-saving rules.
- **Calendar** marks dates with events from the enabled calendars in Evolution Data Server, which GNOME Calendar and Evolution share. Select a date to see its events in local time. Use the arrows to browse months, or click the month name to return to today. The widget refreshes every 30 seconds. Install `evolution-data-server` on the host if your desktop does not already provide it, including when using Flatpak.
- **Weather / AQI** shows current conditions, today’s hourly temperatures in the location’s time zone, and a ten-day forecast with color-coded low-to-high ranges. Hover over the chart for the temperature at a specific hour. The tile also shows wind, sunrise and sunset, and US AQI. Enter a city name or exact latitude and longitude. City search uses the first matching location. The widget refreshes every 15 minutes and retries after five minutes when data is unavailable. [Open-Meteo weather](https://open-meteo.com/) · [Air quality data from CAMS via Open-Meteo](https://open-meteo.com/en/docs/air-quality-api)
- **Notifications** shows the latest 30 incoming Freedesktop desktop notifications while the tile is on Home. The history stays in memory. To clear it, choose **Clear** on the tile in **Edit layout**. The desktop session bus must allow notification monitoring.
- **FPS** shows current FPS, 1% low, 0.1% low, and a 60-second graph. Install MangoHud and add the tile. In **Edit layout**, choose **Setup** on the FPS tile and copy the launch options into Steam. For a standalone game, replace `%command%` with its executable. Start Secondscreen once per login before launching the game so it can create `$XDG_RUNTIME_DIR/secondscreen-mangohud` for MangoHud's per-frame CSV files. Current FPS uses the latest second of frame times. Each low is the frame rate of the slowest 1% or 0.1% of frames in the latest 60 seconds, rounded up to at least one frame.

```sh
MANGOHUD_CONFIG="autostart_log=1,output_folder=$XDG_RUNTIME_DIR/secondscreen-mangohud" mangohud %command%
```

The existing Gamescope launch options still provide current FPS. Gamescope reports an average every 300 frames, so the lows appear as dashes with that feed.

```sh
gamescope --stats-path "$XDG_RUNTIME_DIR/secondscreen-fps" -- %command%
```
