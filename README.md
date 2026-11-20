# Second Screen

Second Screen is a Rust and GPUI dashboard for ultrawide secondary monitors. It runs as a native Wayland window on Linux. Windows integration is deferred.

- **Home** brings live system readings, playback, audio devices, and Discord voice controls into one widget layout.
- **System** shows CPU, GPU, memory and network history, usage for mounted volumes, uptime, and CPU temperature or GPU VRAM when the driver exposes those readings.
- **Audio** adjusts volume and mute for outputs, microphones, playback streams, and recording streams. Volume adjustments preserve the balance of channels with existing levels. You can change the default devices and route individual streams.
- **Media** controls MPRIS players on the desktop session, with track details, album art, player selection, play/pause, previous/next, and seeking when supported by the player.
- **Discord** shows the current voice channel and participants, with mute, deafen, and leave-call controls through the Discord desktop client.

## Run on Linux

Build with Rust 1.95 or newer and keep the provided Cargo lockfile. On Debian or Ubuntu, install the build and audio dependencies:

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan1 pulseaudio-utils
cargo run --release --locked
```

The Wayland session needs a working Vulkan driver. The audio mixer uses `pactl` with a PipeWire PulseAudio server or PulseAudio 15+. Media discovery uses the session D-Bus and works with players that advertise MPRIS.

Use the icons in the left rail or keys **0–4** to switch pages. Drag an empty area of the rail to move the window to your bar monitor. Press **F11** or use the fullscreen button at the bottom of the rail. **Esc** closes Home editing first and can then restore the window. **Tab** moves between controls, **Enter** activates buttons, and arrow keys adjust a focused slider. `--fullscreen` opens the window fullscreen on the compositor’s chosen output. The window’s Wayland app ID is `io.github.secondscreen`, which you can use in compositor window rules.

The same app can also be built with GPUI Fast:

```sh
cargo run --release --locked --no-default-features --features ui-gpui-fast
```

## Customize Home

Choose **Edit layout** to add widgets from the catalog, remove them with **×**, or change their order with the arrows in each card header. Drag a card’s bottom-right corner to resize. Use the width and height controls for single steps, or focus the corner and press the arrow keys. Choose **Done** to return to Home.

Home wraps widgets into fewer columns on narrower windows. Scroll down when the layout grows past the visible area. The app keeps each widget’s requested size when space is limited and saves the layout in `${XDG_CONFIG_HOME:-~/.config}/secondscreen/home.json`.

## Discord setup

Discord voice RPC requires an approved Discord application or access as an authorized tester of the application. Open Discord desktop with the authorized account and launch Second Screen with `SECONDSCREEN_DISCORD_CLIENT_ID` and `SECONDSCREEN_DISCORD_CLIENT_SECRET` in its environment. Select **Discord → Connect** and approve access in Discord. Second Screen keeps the exchanged token in memory. Discord can restore prior voice settings when the RPC connection closes. If your deployment already supplies a Discord OAuth access token with the `identify`, `rpc`, `rpc.voice.read`, and `rpc.voice.write` scopes, set `SECONDSCREEN_DISCORD_ACCESS_TOKEN` in place of the client secret. [Discord RPC documentation](https://docs.discord.com/developers/topics/rpc)

## Framework comparison

See [BENCHMARK.md](BENCHMARK.md) for the performance results and reproduction steps.
