# GPUI and GPUI Fast on Wayland

Both release builds ran Second Screen fullscreen at **2560 × 720** on Sway 1.11 with the default Home widgets and the left navigation rail. The test machine had a 12-vCPU AMD EPYC and used Mesa 26.0.8 lavapipe software Vulkan with a headless Pixman compositor. PipeWire 1.6.2 supplied virtual audio devices and an active playback stream. Two local MPRIS players supplied media metadata.

| Measurement | GPUI 0.2.2 | GPUI Fast 0.1.17 |
| --- | ---: | ---: |
| Warm launch to mapped fullscreen window | **206 ms** | 519 ms |
| Idle CPU | **86.2%** | 171.8% |
| Idle RSS | **183.4 MiB** | 248.7 MiB |
| CPU while switching pages | 761.9% | **299.1%** |
| RSS while switching pages | **185.0 MiB** | 250.9 MiB |
| Key press to next Wayland buffer, median | **29.7 ms** | 40.6 ms |
| Key press to next Wayland buffer, p95 | **48.4 ms** | 80.6 ms |
| Release executable | **36.4 MiB** | 48.8 MiB |

CPU percentages are relative to one logical core. The CPU and RSS figures count the app process and its threads. Startup, CPU, and RSS are medians from three runs per build, with the launch order alternated. Each run settled for two seconds, idled for eight seconds on Home, then switched among Audio, Media, Discord, System, and Home 80 times at 180 ms intervals.

A separate run for each build used the Wayland protocol trace to match all 80 key presses to the next buffer attachment on the window surface. This is an input-to-buffer measurement and includes scheduling and software rendering. Display scanout timing requires a physical display. A periodic dashboard refresh can also contribute the next buffer in this measurement.

GPUI Fast used **61% less CPU during page switching**. GPUI used **50% less CPU while idling on Home**, used **26% less idle memory**, and attached buffers sooner after input. GPUI remains the default for an always-visible dashboard. GPU-accelerated performance still needs measurement.

## Reproduce

Use a Sway session with an output sized to your target monitor and install `wtype`. Close all Second Screen windows before starting. Both builds accept the same app arguments.

```sh
bench_dir=$(mktemp -d)
cargo build --release --locked
cp target/release/secondscreen "$bench_dir/gpui"
cargo build --release --locked --no-default-features --features ui-gpui-fast
cp target/release/secondscreen "$bench_dir/gpui-fast"
XDG_CONFIG_HOME="$bench_dir/config" \
  python3 tools/benchmark.py --output HEADLESS-2 "$bench_dir/gpui" "$bench_dir/gpui-fast"
```

Replace `HEADLESS-2` with an active output from `swaymsg -t get_outputs`. The temporary configuration path loads the default Home layout for both builds. The script prints one JSON record per measured run and per latency trace. Keep the same active applications, audio streams, and renderer for both builds.
