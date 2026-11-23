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

## Default dashboard after reducing redraws

The standard GPUI release builds ran fullscreen at 2560 × 720 on a 12-vCPU AMD EPYC VM with headless Sway, Pixman, and Mesa lavapipe. Each app ran alone with a fresh default dashboard, without media playback, an audio server, or a Discord session. CPU is relative to one logical core and includes child processes reaped during the sample. Memory is median proportional set size (PSS), sampled twice per second. The early figures are medians from two runs per build. The late figures come from one run per build.

| Default dashboard | Before (`7a579c6`) | Updated |
| --- | ---: | ---: |
| CPU at 10–24 seconds | 65.9% | 20.6% |
| CPU at 76–90 seconds | 69.1% | 21.8% |
| PSS at 10–24 seconds | 207.4 MiB | 180.2 MiB |
| PSS at 76–90 seconds | 210.1 MiB | 179.8 MiB |
| Buffer submissions per second | 3.3 | 1.0 |

The updated release submitted zero buffers during a 14-second idle sample on an empty dashboard.

On this software renderer, the updated release used 8.2% CPU and 158.4 MiB PSS in a separate 10–24 second sample with the following options. One sample per path reduces antialiasing on chart paths. The cached icon sprites retain their own antialiasing.

```sh
ZED_PATH_SAMPLE_COUNT=1 LP_NUM_THREADS=4 ./target/release/secondscreen --fullscreen
```
