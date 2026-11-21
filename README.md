# Second Screen

Second Screen is a Rust and GPUI dashboard for ultrawide secondary monitors. It runs as a native Wayland window on Linux. Windows integration is deferred.

The app opens to a single resizable widget grid. **Edit layout** lets you choose the widgets on your display. **Settings** opens over the grid. Click outside it or press **Esc** to return to the dashboard.

- **Performance**, **Memory**, and **Network** show live system readings and usage history. **Storage** shows each drive's capacity and the space used by mounted filesystems that can be attributed to that drive.
- **Now playing** shows track details, album art, and playback controls. When multiple players are available, click the player name in the card header to switch. Make the widget at least three rows tall to see the seek bar.
- **Audio** adjusts device volume and mute and lets you set the default output or microphone. Add **App mixer** for playback and recording streams. Click a stream’s current device to change its route.
- **Discord** shows voice participants and provides controls to connect, mute, deafen, and leave a call.

## Run on Linux

Build with Rust 1.95 or newer and keep the provided Cargo lockfile. On Debian or Ubuntu, install the build and audio dependencies:

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1 pulseaudio-utils
cargo run --release --locked
```

The Wayland session needs a working Vulkan driver. The audio mixer uses `pactl` with a PipeWire PulseAudio server or PulseAudio 15+. Media discovery uses the session D-Bus and works with players that advertise MPRIS.

Drag the title area to move the window to your bar monitor. Press **F11** or click the fullscreen button beside **Settings**. **Esc** dismisses Settings or the active editing controls before leaving fullscreen. **Tab** moves between controls, **Enter** activates buttons, and arrow keys adjust a focused slider. Enable **Start fullscreen** in Settings or pass `--fullscreen` to open fullscreen on the compositor’s chosen output. The window’s Wayland app ID is `io.github.secondscreen`, which you can use in compositor window rules.

The same app can also be built with GPUI Fast:

```sh
cargo run --release --locked --no-default-features --features ui-gpui-fast
```

## Customize Home

Choose **Edit layout** to add widgets from the catalog, remove them with **×**, or change their order with the arrows in each card header. Drag a card’s bottom-right corner to resize. Use the width and height controls for single steps, or focus the corner and press the arrow keys. Choose **Done** to return to Home.

Home fits four grid rows and up to five columns in the visible area. Widgets can span two to five rows. Scroll down when the layout grows past the visible area. The app keeps each widget’s requested size when space is limited and saves the layout in `${XDG_CONFIG_HOME:-~/.config}/secondscreen/home.json`.

## Discord setup

Discord voice RPC requires an approved Discord application or access as an authorized tester of the application. Open Discord desktop with the authorized account. In **Settings → Discord**, enter your application ID and its client secret, then save and click **Connect** in the Discord widget to authorize access. Add it from **Edit layout** if needed. You can supply a Discord OAuth access token with the `identify`, `rpc`, `rpc.voice.read`, and `rpc.voice.write` scopes in place of the client secret. Second Screen keeps tokens obtained during authorization in memory. Discord can restore prior voice settings when the RPC connection closes. [Discord RPC documentation](https://docs.discord.com/developers/topics/rpc)

Use the **Media apps**, **Audio devices**, and **Storage** sections of Settings to choose which sources appear in the widgets. Settings saves credentials and visibility preferences to the user-only file `${XDG_CONFIG_HOME:-~/.config}/secondscreen/settings.json`. Existing deployments can still use `SECONDSCREEN_DISCORD_CLIENT_ID` with `SECONDSCREEN_DISCORD_CLIENT_SECRET` or `SECONDSCREEN_DISCORD_ACCESS_TOKEN`. Saved Discord credentials take precedence.

Storage lists whole drives exposed by Linux, including unmounted drives. It shows usage when a mounted filesystem can be attributed to one drive. Storage visibility is now saved per drive, so select your drives in Settings after upgrading from mount-level visibility.

## Framework comparison

See [BENCHMARK.md](BENCHMARK.md) for the performance results and reproduction steps.
