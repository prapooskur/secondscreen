#!/usr/bin/env python3
"""Compare release builds using verified Settings and profile actions on Sway."""

import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time

APP_ID = "io.github.secondscreen"
APP = f'[app_id="{APP_ID}"]'
TICKS = os.sysconf("SC_CLK_TCK")
ACTION_INTERVAL = 0.18


def sway(*args):
    return json.loads(subprocess.check_output(["swaymsg", "-r", *args], text=True))


def command(message):
    if not all(result["success"] for result in sway(message)):
        raise RuntimeError(f"Sway command failed: {message}")


def find_window(node):
    if node.get("app_id") == APP_ID:
        return node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        if result := find_window(child):
            return result


def window():
    return find_window(sway("-t", "get_tree"))


def fixture(directory):
    directory = directory / "secondscreen"
    directory.mkdir()
    kinds = ["performance", "media", "audio", "discord"]
    profiles = [
        {"layout": {"widgets": [
            {"id": i, "kind": kind, "columns": 1, "rows": 4}
            for i, kind in enumerate(order, 1)
        ]}}
        for order in (kinds, kinds[1:] + kinds[:1])
    ]
    (directory / "home.json").write_text(json.dumps({"profiles": profiles}))
    (directory / "settings.json").write_text(json.dumps({"theme": "dark"}))


@contextlib.contextmanager
def run_app(binary, output, seat):
    rect = output["rect"]
    command(f"seat {seat} cursor set {rect['x'] + 1} {rect['y'] + 1}")
    command(f"focus output {output['name']}")
    with (
        tempfile.TemporaryDirectory(prefix="secondscreen-bench-") as config,
        tempfile.TemporaryFile() as log,
    ):
        fixture(Path(config))
        env = dict(os.environ, XDG_CONFIG_HOME=config)
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DEBUG", None)
        start = time.monotonic()
        app = subprocess.Popen([binary, "--fullscreen"], env=env, stdout=log, stderr=log)
        try:
            while time.monotonic() - start < 15:
                node = window()
                if (
                    node and node.get("pid") == app.pid and node.get("visible")
                    and node.get("fullscreen_mode") == 1 and node.get("shell") == "xdg_shell"
                    and node["rect"] == rect
                ):
                    break
                if app.poll() is not None:
                    break
                time.sleep(0.01)
            else:
                node = None
            if node is None or app.poll() is not None:
                log.seek(0)
                raise RuntimeError(f"{binary} did not map fullscreen on {output['name']}. "
                                   f"{log.read().decode(errors='replace')}")
            startup = (time.monotonic() - start) * 1_000
            command(f"{APP} focus")
            yield app, startup
        finally:
            if app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait()


def cpu_seconds(app):
    stat = Path(f"/proc/{app.pid}/stat").read_text().rsplit(")", 1)[1].split()
    return (int(stat[11]) + int(stat[12])) / TICKS


def rss_mib(app):
    status = Path(f"/proc/{app.pid}/status").read_text()
    return int(re.search(r"^VmRSS:\s+(\d+)", status, re.MULTILINE)[1]) / 1024


def idle_resources(app):
    start_cpu, start = cpu_seconds(app), time.monotonic()
    memory = []
    while time.monotonic() - start < 8:
        memory.append(rss_mib(app))
        time.sleep(0.2)
    return 100 * (cpu_seconds(app) - start_cpu) / (time.monotonic() - start), statistics.median(memory)


class Dashboard:
    def __init__(self, rect, seat):
        self.seat = seat
        self.x, self.y = rect["x"], rect["y"]
        self.width = rect["width"]
        self.scale = max(0.79, min(1.72, self.width / 1400, rect["height"] / 430))

    def click(self, x, y):
        x, y = self.x + round(x), self.y + round(y)
        command(f"seat {self.seat} cursor set {x} {y}; "
                f"seat {self.seat} cursor press button1; "
                f"seat {self.seat} cursor release button1")

    def pixels(self, x, y, width, height):
        geometry = f"{self.x + round(x)},{self.y + round(y)} {round(width)}x{round(height)}"
        data = subprocess.check_output(["grim", "-s", "1", "-t", "ppm", "-g", geometry, "-"], timeout=3)
        magic, _, depth, pixels = data.split(b"\n", 3)
        if magic != b"P6" or depth != b"255":
            raise RuntimeError("Unexpected grim PPM format")
        channels = iter(pixels)
        return zip(channels, channels, channels, strict=True)

    def settings_visible(self):
        # Settings dims this empty strip of the left rail with its backdrop.
        s = self.scale
        pixels = self.pixels(24 * s, 190 * s, 6 * s, 10 * s)
        return statistics.median(g for _, g, _ in pixels) < 24

    def selected_profile(self):
        # Sample the badge backgrounds to avoid font antialiasing differences between renderers.
        s = self.scale
        pixels = list(self.pixels(27 * s, 35 * s, 5 * s, 47 * s))
        patch_size = round(8 * s) * round(5 * s)
        first = statistics.median(g for _, g, _ in pixels[:patch_size])
        second = statistics.median(g for _, g, _ in pixels[-patch_size:])
        if first >= 50 > second:
            return 1
        if second >= 50 > first:
            return 2

    def actions(self):
        s = self.scale
        return [
            ("open_settings", lambda: self.click(self.width - 40 * s, 78 * s),
             self.settings_visible),
            ("close_settings", lambda: self.click(self.width - 44 * s, 43 * s),
             lambda: not self.settings_visible()),
            ("profile_2", lambda: self.click(40 * s, 78 * s),
             lambda: self.selected_profile() == 2),
            ("profile_1", lambda: self.click(40 * s, 39 * s),
             lambda: self.selected_profile() == 1),
        ]


def wait_visible(predicate, name):
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if predicate():
            return
    raise RuntimeError(f"Did not see {name} complete on screen")


def interactions(app, dashboard, cycles):
    actions = dashboard.actions()
    wait_visible(lambda: dashboard.selected_profile() == 1 and not dashboard.settings_visible(), "initial Home")
    timings = {name: [] for name, _, _ in actions}
    memory = []
    start_cpu, start = cpu_seconds(app), time.monotonic()
    for _ in range(cycles):
        memory.append(rss_mib(app))
        for name, act, visible in actions:
            triggered = time.monotonic()
            act()
            wait_visible(visible, name)
            elapsed = time.monotonic() - triggered
            timings[name].append(1_000 * elapsed)
            time.sleep(max(0., ACTION_INTERVAL - elapsed))
    cpu = 100 * (cpu_seconds(app) - start_cpu) / (time.monotonic() - start)
    latency = {
        name: {
            "samples": len(samples),
            "p50_ms": statistics.median(samples),
            "p95_ms": (
                statistics.quantiles(samples, n=100, method="inclusive")[94]
                if len(samples) > 1 else samples[0]
            ),
        }
        for name, samples in timings.items()
    }
    return cpu, statistics.median(memory), latency


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gpui", type=lambda name: Path(name).resolve())
    parser.add_argument("gpui_fast", type=lambda name: Path(name).resolve())
    parser.add_argument("--output", required=True, help="Name of the Sway output")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--cycles", type=int, default=20, help="Settings and profile cycles per run")
    args = parser.parse_args()
    if args.rounds < 1 or args.cycles < 1:
        parser.error("--rounds and --cycles must be positive")
    if window():
        parser.error("Close existing Second Screen windows before benchmarking")
    outputs = {output["name"]: output for output in sway("-t", "get_outputs") if output["active"]}
    if args.output not in outputs:
        parser.error("Choose an active Sway output")
    output = outputs[args.output]
    seat = sway("-t", "get_seats")[0]["name"]
    dashboard = Dashboard(output["rect"], seat)
    print(json.dumps({"output": output["rect"], "mode": output["current_mode"]}), flush=True)

    binaries = [("gpui", args.gpui), ("gpui-fast", args.gpui_fast)]
    for name, binary in binaries:
        with run_app(binary, output, seat):
            time.sleep(2)
        print(json.dumps({"warmed_up": name}), flush=True)

    for index in range(args.rounds):
        for name, binary in (binaries if index % 2 == 0 else reversed(binaries)):
            with run_app(binary, output, seat) as (app, startup):
                time.sleep(2)
                idle_cpu, idle_rss = idle_resources(app)
                interaction_cpu, interaction_rss, latency = interactions(app, dashboard, args.cycles)
            print(json.dumps({
                "round": index + 1, "build": name, "startup_ms": startup,
                "idle_cpu": idle_cpu, "idle_rss_mib": idle_rss,
                "interaction_cpu": interaction_cpu, "interaction_rss_mib": interaction_rss,
                "visible_latency": latency,
                "binary_mib": binary.stat().st_size / (1024 * 1024),
            }), flush=True)


if __name__ == "__main__":
    main()
