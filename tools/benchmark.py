#!/usr/bin/env python3
"""Compare the two release builds in the same Sway Wayland session."""

import argparse
import bisect
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
KEY_PRESS = re.compile(r"wl_keyboard[#@]\d+\.key\(\d+, \d+, \d+, 1\)")
XDG_SURFACE = re.compile(r"\.get_xdg_surface\(new id xdg_surface[#@]\d+, wl_surface[#@](\d+)\)")
TIMESTAMP = re.compile(r"^\[\s*([\d.]+)\]")
TICKS = os.sysconf("SC_CLK_TCK")
SWITCHES = "23410" * 16


def sway(*args):
    return subprocess.check_output(["swaymsg", "-r", *args], text=True)


def find_window(node):
    if node.get("app_id") == APP_ID:
        return node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        result = find_window(child)
        if result:
            return result


def window():
    return find_window(json.loads(sway("-t", "get_tree")))


@contextlib.contextmanager
def run_app(binary, output, trace):
    rect = output["rect"]
    seat = json.loads(sway("-t", "get_seats"))[0]["name"]
    sway(f"seat {seat} cursor set {rect['x'] + 1} {rect['y'] + 1}")
    sway(f"focus output {output['name']}")
    env = os.environ.copy()
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DEBUG", None)
    if trace is not None:
        env["WAYLAND_DEBUG"] = "client"
    with tempfile.TemporaryFile() as log:
        start = time.monotonic()
        app = subprocess.Popen([binary, "--fullscreen"], env=env, stdout=log, stderr=trace or log)
        try:
            while time.monotonic() - start < 15:
                node = window()
                if node and node.get("visible") and node.get("fullscreen_mode") == 1 and node["rect"] == rect:
                    break
                if app.poll() is not None:
                    break
                time.sleep(0.01)
            if not node or node.get("shell") != "xdg_shell" or node["rect"] != output["rect"]:
                log.seek(0)
                location = node.get("rect") if node else None
                raise RuntimeError(f"{binary} did not map fullscreen on {output['name']} (window: {location}). "
                                   f"{log.read().decode(errors='replace')}")
            startup = (time.monotonic() - start) * 1_000
            sway(f"{APP} focus")
            yield app, startup
        finally:
            if app.poll() is None:
                sway(f"{APP} kill")
                try:
                    app.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    app.terminate()
                    app.wait(timeout=5)


def cpu_seconds(app):
    stat = Path(f"/proc/{app.pid}/stat").read_text().rsplit(")", 1)[1].split()
    return (int(stat[11]) + int(stat[12])) / TICKS


def rss_mib(app):
    status = Path(f"/proc/{app.pid}/status").read_text()
    return int(re.search(r"^VmRSS:\s+(\d+)", status, re.MULTILINE)[1]) / 1024


def keyboard():
    return subprocess.Popen(["wtype", "-s", "350", "-d", "180", SWITCHES, "-s", "150"])


def resources(app, seconds=None):
    start_cpu, start = cpu_seconds(app), time.monotonic()
    memory = []
    keys = keyboard() if seconds is None else None
    while (time.monotonic() - start < seconds) if seconds is not None else (keys.poll() is None):
        memory.append(rss_mib(app))
        time.sleep(0.20)
    if keys is not None and keys.wait() != 0:
        raise RuntimeError("wtype failed")
    return 100 * (cpu_seconds(app) - start_cpu) / (time.monotonic() - start), statistics.median(memory)


def latency(binary, output):
    with tempfile.TemporaryFile() as trace:
        with run_app(binary, output, trace):
            time.sleep(1.5)
            if keyboard().wait() != 0:
                raise RuntimeError("wtype failed")
            time.sleep(0.4)
        trace.seek(0)
        keys, attachments = [], []
        surface = None
        for line in trace.read().decode(errors="replace").splitlines():
            if surface is None and (match := XDG_SURFACE.search(line)):
                surface = re.compile(rf"wl_surface[#@]{match[1]}\.attach\(wl_buffer")
            if not (timestamp := TIMESTAMP.match(line)):
                continue
            if KEY_PRESS.search(line):
                keys.append(float(timestamp[1]))
            if surface and surface.search(line):
                attachments.append(float(timestamp[1]))
        if len(keys) != len(SWITCHES):
            raise RuntimeError(f"Expected {len(SWITCHES)} Wayland key presses, observed {len(keys)}")
        delays = []
        for index, key in enumerate(keys):
            following = keys[index + 1] if index + 1 < len(keys) else key + 180
            pos = bisect.bisect_left(attachments, key)
            if pos < len(attachments) and attachments[pos] < following:
                delays.append(attachments[pos] - key)
        if not delays:
            raise RuntimeError("No Wayland buffer attachments matched keyboard input")
        p95 = statistics.quantiles(delays, n=100, method="inclusive")[94]
        return {"matched": len(delays), "keys": len(keys), "p50_ms": statistics.median(delays), "p95_ms": p95}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gpui", type=lambda name: Path(name).resolve())
    parser.add_argument("gpui_fast", type=lambda name: Path(name).resolve())
    parser.add_argument("--output", required=True, help="Name of the Sway output")
    parser.add_argument("--rounds", type=int, default=3)
    args = parser.parse_args()
    if args.rounds < 1:
        parser.error("--rounds must be positive")
    if window():
        parser.error("Close existing Second Screen windows before benchmarking")
    outputs = {output["name"]: output for output in json.loads(sway("-t", "get_outputs")) if output["active"]}
    if args.output not in outputs:
        parser.error("Choose an active Sway output")
    output = outputs[args.output]
    print(json.dumps({"output": output["rect"], "mode": output["current_mode"]}), flush=True)

    binaries = {"gpui": args.gpui, "gpui-fast": args.gpui_fast}
    for label, binary in binaries.items():
        with run_app(binary, output, None):
            time.sleep(2)
        print(json.dumps({"warmed_up": label}), flush=True)

    for index in range(args.rounds):
        entries = list(binaries.items())
        if index % 2:
            entries.reverse()
        for name, binary in entries:
            with run_app(binary, output, None) as (app, startup):
                time.sleep(2)
                idle_cpu, idle_rss = resources(app, seconds=8)
                switch_cpu, switch_rss = resources(app)
            print(json.dumps({
                "round": index + 1, "build": name, "startup_ms": startup,
                "idle_cpu": idle_cpu, "idle_rss_mib": idle_rss,
                "switch_cpu": switch_cpu, "switch_rss_mib": switch_rss,
                "binary_mib": binary.stat().st_size / (1024 * 1024),
            }), flush=True)

    for name, binary in binaries.items():
        print(json.dumps({"build": name, "wayland_latency": latency(binary, output)}), flush=True)


if __name__ == "__main__":
    main()
