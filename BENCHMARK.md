# GPUI and GPUI Fast on Wayland

## Earlier page-switch benchmark

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

## Benchmark the current dashboard

Use a Sway session with a pointer device and an output sized to your target monitor. Install `grim` and close any running Second Screen windows before starting.

```sh
bench_dir=$(mktemp -d)
cargo build --release --locked
cp target/release/secondscreen "$bench_dir/gpui"
cargo build --release --locked --no-default-features --features ui-gpui-fast
cp target/release/secondscreen "$bench_dir/gpui-fast"
python3 tools/benchmark.py --output HEADLESS-2 "$bench_dir/gpui" "$bench_dir/gpui-fast"
```

Replace `HEADLESS-2` with an active output from `swaymsg -t get_outputs`. The script uses temporary configs with two profiles and a fixed dark theme. Each cycle opens Settings, closes Settings, and switches to each profile. Cropped screenshots verify the backdrop and active profile badge. Latency includes Sway input delivery and screenshot capture, so compare it within this benchmark. The script reports CPU, RSS, and visible latency by action in one JSON record per run. Keep the same active applications, audio streams, and renderer for both builds.

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

## Offscreen widgets and idle FPS

Compared with `f650b03`, the GPUI release build eliminated periodic frame submissions for a dashboard whose telemetry widgets were all horizontally clipped and for a standalone FPS widget waiting for a game. Both releases ran fullscreen at 2560 × 720 on headless Sway with Mesa lavapipe. Each row uses one 10-second sample after a 3-second settle. CPU is a percentage of one logical core.

| Dashboard | CPU before | CPU after | Buffers/s before | Buffers/s after |
| --- | ---: | ---: | ---: | ---: |
| Default layout | 12.2% | 12.0% | 1.0 | 1.0 |
| Telemetry offscreen | 13.9% | 0.6% | 1.0 | 0.0 |
| FPS waiting for a game | 7.9% | 0.6% | 1.0 | 0.0 |

In a separate Viewer test, application PSS before scrolling fell from 176 MiB to 130 MiB. The updated build loaded WebKit and rendered the test page on first reveal. The app continued to sample telemetry for chart history. Storage scans started when Storage settings opened and stopped after they closed.

A CPU profile of the default dashboard attributed about 97% of sampled userspace cycles to the software Vulkan driver and its generated code. GPU-accelerated performance remains unmeasured.

## Current GPUI, GPUI Fast, and Xenon

The current Second Screen release builds and Xenon 5.2.0 (`77f849c9`) ran alone at 2560 × 720 on the same 12-vCPU AMD EPYC VM with headless Sway 1.11, Pixman, and Mesa 26.0.8 software rendering. All used the same PipeWire null sink. Xenon ran its native Tauri shell with WebKitGTK 2.52.6 and its Node 22.23.4 backend. Xenon used its standard visible layout and animation settings, with background updates and usage reporting disabled.

| Default dashboard | GPUI 0.2.2 | GPUI Fast 0.1.17 | Xenon 5.2.0 |
| --- | ---: | ---: | ---: |
| Launch to visible dashboard | 0.39 s | 0.71 s | 6.17 s |
| CPU at 6–18 s after the dashboard appeared | 12.7% | 15.8% | 360.4% |
| PSS at 6–18 s | 155 MiB | 244 MiB | 953 MiB |
| CPU at 72–84 s with no input | 13.7% | 13.8% | 22.4% |
| PSS at 72–84 s | 163 MiB | 233 MiB | 969 MiB |

Launch and early results are medians of two runs per app. Late results are from one run per app. Startup ends when a screenshot shows the dashboard controls and includes starting Node for Xenon. CPU is relative to one logical core. PSS is proportional set size, sampled twice per second and summed across each app's processes, including the Xenon backend and WebKit subprocesses. The compositor and audio server are outside the app totals.

Xenon pauses decorative animation after 60 seconds without input. In the run with both windows measured, its CPU use dropped from 350.3% to 22.4%. The Node backend used 1.7% of one core in the early window. The default layouts and visual effects differ, and these measurements use software rendering.

A telemetry-focused run used Performance, Memory, and Storage in Second Screen and only Xenon's System widget, with its Network & Gaming cards hidden. Xenon's theme and animations were still enabled.

| Telemetry-focused dashboard at 6–18 s | GPUI | GPUI Fast | Xenon |
| --- | ---: | ---: | ---: |
| CPU | 13.5% | 16.6% | 340.8% |
| PSS | 157 MiB | 220 MiB | 1083 MiB |

The headless session delivered the synthetic Settings click to Xenon's Wayland window, but the panel did not appear in the screenshots. The comparison therefore omits Settings latency.
