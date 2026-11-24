#!/usr/bin/env bash
set -euo pipefail

if (( $# != 0 )); then
    printf 'Usage: %s\n' "${0##*/}" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

if ! cargo deb --version > /dev/null 2>&1; then
    cargo install --locked cargo-deb --version 3.8.0
fi

cargo build --release --locked
cargo deb --locked --no-build
