# Second Screen

Second Screen is a Rust and GPUI dashboard for ultrawide secondary monitors. It runs as a native Wayland window on Linux. Windows integration is deferred.

The app opens to a single resizable widget grid. Use **Edit layout** in the right-hand rail to choose the widgets on your display. **Settings** opens over the grid. Click outside it or press **Esc** to return to the dashboard.

- **System monitor** brings hardware sensors, RAM, storage, and network traffic together with AIDA64-style gauges, segmented usage bars, and live charts. It shows drive capacity and attributed filesystem usage, plus download and upload rates across active network interfaces with a one-minute history. Find it on the default dashboard or under **Edit layout → Add widget → System monitor**.
- **Now playing** shows track details, album art, and playback controls. Tap a player tab to switch apps, and swipe across the tabs if more apps are offscreen. Tap or drag the seek bar to jump within a track. Make the widget wider or taller if the seek bar is hidden.
- **Audio** adjusts device volume and mute and lets you set the default output or microphone. Add **App mixer** to adjust playback and recording streams using app icons, mute buttons, and volume sliders. Hover over an icon to see the app and stream names.
- **Quick controls** puts playback, audio, Discord voice, and dashboard shortcuts in a customizable grid. The icons reflow as you resize the tile. Scroll inside it when space is tight.
- **Discord** shows a green ring around each speaker’s avatar and a **LIVE** badge when the desktop client reports a Go Live stream. The widget also provides controls to connect, mute, deafen, and leave a call.
- **Home Assistant** shows the state of a selected entity and lets you control it from the dashboard.
- **Viewer** displays web pages, iframes, YouTube, Twitch, and local media inside the grid. Discord voice channel links open in Discord.

## Install on Linux

Open the **Linux packages** run for your commit under **Actions** on GitHub. Every push builds x86-64 packages with the default `gpui-pre` backend. Download and unzip the artifact for your distribution, then install the package inside it.

| Package | Build environment | Install |
| --- | --- | --- |
| DEB | Ubuntu 24.04 | `sudo apt install ./*.deb` |
| RPM | Fedora 44 | `sudo dnf install ./*.rpm` |
| Flatpak | GNOME 51 runtime from Flathub | `flatpak install --user ./*.flatpak` |

Launch **Second Screen** from your app menu in a Wayland session. Flatpak keeps its settings separately under `~/.var/app/io.github.secondscreen`. Host notification monitoring and compositor-specific display placement require a native package. System monitor reports usage for filesystems visible inside the sandbox, including drives mounted under `/media`, `/mnt`, and `/run/media`.

### Build packages locally

With Rust 1.95 or newer, run the script for your format from the repository root.

| Package | Command | Default output directory |
| --- | --- | --- |
| DEB | `./packaging/build-deb.sh` | `target/debian/` |
| RPM | `./packaging/build-rpm.sh` | `target/generate-rpm/` |
| Flatpak | `./packaging/build-flatpak.sh` | `target/packaging/` |

- **DEB** uses the Debian or Ubuntu build dependencies below plus `dpkg-dev`. The script installs `cargo-deb` if it is missing.
- **RPM** uses the Fedora packages `gcc`, `gcc-c++`, `make`, `pkgconf-pkg-config`, `fontconfig-devel`, `freetype-devel`, `wayland-devel`, `libxkbcommon-devel`, `'pkgconfig(xkbcommon-x11)'`, `webkit2gtk4.1-devel`, and `evolution-data-server-devel`. The script installs `cargo-generate-rpm` if it is missing.
- **Flatpak** requires `flatpak` and `flatpak-builder`. The script installs the runtime and SDK from Flathub in your user installation. If they are already installed, pass `--skip-install-deps`.

## Run on Linux

Build with Rust 1.95 or newer and keep the provided Cargo lockfile. On Debian or Ubuntu, install the build and audio dependencies:

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1 pulseaudio-utils wl-clipboard \
  libwebkit2gtk-4.1-dev libecal2.0-dev gstreamer1.0-plugins-good gstreamer1.0-libav
cargo run --release --locked
```

The Wayland session needs a working Vulkan driver. The viewer builds against GTK 3 and WebKitGTK 4.1 (`gtk3-devel` and `webkit2gtk4.1-devel` on Fedora). It loads those libraries when a web view opens and uses the system GStreamer codecs for playback. The audio mixer uses `pactl` with a PipeWire PulseAudio server or PulseAudio 15+. Media discovery uses the session D-Bus and works with players that advertise MPRIS. The Steam launch options **Copy** button uses `wl-clipboard` for mouse clicks in the default build.

Drag the grip on the right-hand rail to move the window to your bar monitor. Press **F11** or click the fullscreen button near the bottom of the rail. **Esc** dismisses an open overlay or the active editing controls before leaving fullscreen. **Tab** moves between controls, **Enter** activates buttons, and arrow keys adjust a focused slider. Enable **Start fullscreen** in **Settings → General** or pass `--fullscreen` to open fullscreen.

In **Settings → General**, enable **Start at login** to open Second Screen when you sign in. The Flatpak uses your desktop’s Background portal. Native packages write an XDG autostart entry. If you move a native executable, toggle **Start at login** off and back on to update its path.

Use **Launch display** in **Settings → General** to choose a connected output on Sway or KDE Plasma 6 on Wayland. The dashboard moves to that display and returns to it when it reconnects or the system wakes from sleep, preserving the selected fullscreen or windowed mode. If the display is disconnected at launch, the desktop chooses an output until the selected display returns. On GNOME, the desktop chooses the display.

In **Settings → General → Background image**, select **Choose folder…** to cycle through its PNG, JPEG, WebP, and GIF images in filename order. Rotation defaults to 6 hours (360 minutes). Enter a positive whole number of minutes and select **Save** to change it. **Choose image…** switches to a single image, and **Reset** restores the default background.

The same app can also be built with GPUI Fast:

```sh
cargo run --release --locked --no-default-features --features ui-gpui-fast
```

## Customize Home

Choose **Edit layout** to add widgets from the catalog and show widget names and controls at the top. Use **×** to remove a widget or the arrows to change its order. Drag a widget’s bottom-right corner to resize. Use the width and height controls for single steps, or focus the corner and press the arrow keys. Choose **Done** to return to Home.

Use the left rail to switch profiles. **+** creates an empty profile and selects it, up to five total. The existing Home layout becomes profile 1. Each profile saves its own layout and widget options.

Select the pencil on the left rail to open **Profiles**. Select any profile to rename it and preview its icon. Use the first character of the profile name, choose from 24 built-in SVGs, or import an SVG or PNG. Unnamed profiles use their number as the text badge. Select **Save changes** to update the profile. **Set active** switches the dashboard to the selected profile. **Delete profile** removes the selected profile and its layout after confirmation. At least one profile stays on the dashboard.

Home fits four grid rows and up to five columns in the visible area. Widgets can span two to four visible rows. When the grid extends to the right, swipe across it on a touchscreen, scroll horizontally with a trackpad, use **Shift** + mouse wheel, or use the arrows in the right-hand rail. The app keeps each widget’s requested size when space is limited and saves the layout in `${XDG_CONFIG_HOME:-~/.config}/secondscreen/home.json`.

On the **Quick controls** tile, choose **Setup** in **Edit layout**. Use **+** to add an action, the up and down arrows to set its position, or **×** to remove it. Buttons fill each row from left to right. Each profile saves its own selection and order.

## Configure System monitor

The starter dashboard shows live CPU and memory usage. Select **Add cards** directly on an empty System monitor tile, or open **Edit layout → Setup** and select **Add card**. Pick **Bar**, **Chart**, or **Gauge**, then choose the metric to display. Each card shows its display type above its metrics. Use **Edit** to change a card’s type or metrics, or use the arrows to reorder cards. The **Top bar** toggle controls the widget title, CPU model, and host uptime.

In the chart editor, use **Add metric** to select up to four readings. **Separate plots** puts them side by side when space allows. **Shared graph** overlays them with color-coded values. Readings with the same units share a scale, and mixed units use independent scales. Hover over a reading for its full label and scale.

A single card fills the widget body. The grid places cards in Setup order and uses open gaps, so compact bars and gauges can stack beside taller charts. Scroll inside the widget to reach additional readings. Charts show the last 60 samples.

The picker includes CPU and per-core load, memory, GPU load and VRAM where available, Linux hardware-monitor channels, download and upload speeds, and individual drives. **All visible drives** follows your Storage visibility settings and fills in drives that have no individual entry on that tile. Pick an individual drive to include it in a multi-metric chart. Each System monitor saves its own readings, display styles, order, and top-bar setting. Older layouts carry their active sensor selections into the new setup.

## Control Home Assistant devices

In your Home Assistant profile, open **Security** and create a **Long-lived access token**. In Second Screen, open **Settings → Home Assistant**, enter the server URL (for example, `http://homeassistant.local:8123`) and token, then choose **Connect**. Second Screen checks the connection before saving the credentials in its user-only `settings.json` file.

Open **Edit layout → Add widget → Home Assistant**. Choose **Entity** on the tile, then search by name or entity ID and select it. Add another tile for each entity you want to control. Selections are saved per tile and profile.

The controls depend on the entity. For example, you can switch devices on or off, dim lights, change fan speed, adjust a thermostat, operate covers and locks, and activate scenes or scripts. Tiles refresh when entities change in Home Assistant.

## Watch video and streams

Add **Viewer** from **Edit layout → Add widget**. Select a source and choose **Show**. Web pages, iframe embeds, YouTube video, live and playlist links play inside the tile. **Twitch Live** and **Twitch Chat** accept a channel name or channel URL. Add another Viewer to keep a stream and its chat side by side. In **Edit layout**, use **Source** to change the tile or open a restricted embed in your browser.

**Image / video** accepts a local media file. **Slideshow** accepts a folder and displays supported files in filename order. The delay controls set how long each image stays on screen. Videos advance at the end of playback. Use the arrows on the slideshow to move between items. The dashboard restores each saved source when it opens.

Paste a Discord voice channel link into the Viewer. The dashboard leaves fullscreen to show the Discord desktop client or its web client. Choose **Watch Stream** on the live participant in Discord.

## Discord setup

For client-secret login, add exactly `http://127.0.0.1` under **OAuth2 → Redirects** for your application in the Discord Developer Portal. Discord desktop uses the registered redirect and returns the authorization code over RPC. Second Screen sends the URI to Discord's token endpoint when exchanging the code.

Discord voice RPC requires an approved Discord application or access as an authorized tester of the application. Open Discord desktop with the authorized account. In **Settings → Discord**, enter your application ID and its client secret, then save and click **Connect** in the Discord widget to authorize access. Add it from **Edit layout** if needed. You can supply a Discord OAuth access token with the `identify`, `rpc`, `rpc.voice.read`, and `rpc.voice.write` scopes in place of the client secret. Second Screen saves OAuth authorization in the desktop keyring through Secret Service and reconnects when Discord opens. It refreshes expired tokens using the saved grant. On desktops without a working keyring, OAuth credentials obtained during authorization last until the app exits. Discord can restore prior voice settings when the RPC connection closes. [Discord RPC documentation](https://docs.discord.com/developers/topics/rpc)

When using Second Screen’s Flatpak, open Discord first so Flatpak can mount its IPC socket. If you restart a native Discord client, reopen Second Screen to pick up the new socket.

Use the **Media apps**, **Audio devices**, and **Storage** sections of Settings to choose which sources appear in the widgets. Settings saves credentials and visibility preferences to the user-only file `${XDG_CONFIG_HOME:-~/.config}/secondscreen/settings.json`. Existing deployments can still use `SECONDSCREEN_DISCORD_CLIENT_ID` with `SECONDSCREEN_DISCORD_CLIENT_SECRET` or `SECONDSCREEN_DISCORD_ACCESS_TOKEN`. Saved Discord credentials take precedence.

Storage lists whole drives exposed by Linux, including unmounted drives. It shows usage when a mounted filesystem can be attributed to one drive. Storage visibility is now saved per drive, so select your drives in Settings after upgrading from mount-level visibility.

## Framework comparison

See [BENCHMARK.md](BENCHMARK.md) for the performance results and reproduction steps.

## Clock, calendar, weather, notifications, and FPS

Add these tiles from **Edit layout → Add widget**. Open **Settings → Widgets** to change time zones, weather location, or the weather units and time format.

- **World clock** shows local time and your chosen IANA time zones. Enter names such as `America/New_York, Asia/Kolkata`. Each clock follows its zone’s daylight-saving rules.
- **Calendar** marks dates with events from the enabled calendars in Evolution Data Server, which GNOME Calendar and Evolution share. Select a date to see its events in local time. Use the arrows to browse months, or click the month name to return to today. The widget refreshes every 30 seconds. Install `evolution-data-server` on the host if your desktop does not already provide it, including when using Flatpak.
- **Weather / AQI** shows current conditions, today’s hourly temperatures in the location’s time zone, and a ten-day forecast with color-coded low-to-high ranges. Hover over the chart for the temperature at a specific hour. The tile also shows wind, sunrise and sunset, and US AQI. Enter a city name or exact latitude and longitude. City search uses the first matching location. The widget refreshes every 15 minutes and retries after five minutes when data is unavailable. [Open-Meteo weather](https://open-meteo.com/) · [Air quality data from CAMS via Open-Meteo](https://open-meteo.com/en/docs/air-quality-api)
- **Notifications** shows the latest 30 incoming Freedesktop desktop notifications while the tile is on Home. Use **Settings → Notifications** to hide individual apps or message bodies. Apps appear in Settings as they send notifications. Filters persist across restarts, and the history stays in memory. To clear it, choose **Clear** on the tile in **Edit layout**. The desktop session bus must allow notification monitoring.
- **FPS** shows current FPS, 1% low, 0.1% low, and the latest frame time in milliseconds. The 60-second graphs show FPS and peak frame time for each sample. Install MangoHud and add the tile. Copy the MangoHud launch options from **Settings → Widgets** into Steam. For a standalone game, replace `%command%` with its executable. Start Secondscreen once per login before launching the game so it can create `$XDG_RUNTIME_DIR/secondscreen-mangohud` for MangoHud's per-frame CSV files. Current FPS and peak frame time use the latest second of frames. Each low is the frame rate of the slowest 1% or 0.1% of frames in the latest 60 seconds, rounded up to at least one frame.

```sh
MANGOHUD_CONFIG="autostart_log=1,output_folder=$XDG_RUNTIME_DIR/secondscreen-mangohud" mangohud %command%
```

In **Edit layout**, choose **Setup** on the FPS tile to toggle each readout and graph independently. Changes save immediately. Select **Done** to return to the live tile.

To use an existing folder for MangoHud logs, enter its absolute path under **Settings → Widgets → MangoHud log folder** and select **Save folder**. Copy the updated launch options into Steam. Save a blank field to restore the default.

Gamescope reports an average every 300 frames, so the lows and frame time appear as dashes with that feed. Add the FPS tile or open **Settings → Integration status** to start the reader. For native packages, use:

```sh
gamescope --stats-path "$XDG_RUNTIME_DIR/secondscreen-fps" -- %command%
```

For Flatpak, use the shared runtime directory:

```sh
gamescope --stats-path "$XDG_RUNTIME_DIR/secondscreen-gamescope/secondscreen-fps" -- %command%
```

Open **Settings → Integration status** to check Calendar, FPS, and Notifications even if the tiles are absent from the active profile. The panel reports connection and feed status with setup steps for missing dependencies and desktop restrictions.
