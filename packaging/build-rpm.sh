#!/usr/bin/env bash
set -euo pipefail

if (( $# != 0 )); then
    printf 'Usage: %s\n' "${0##*/}" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

if ! cargo generate-rpm --version > /dev/null 2>&1; then
    cargo install --locked cargo-generate-rpm --version 0.21.1
fi

CARGO_PROFILE_RELEASE_STRIP=symbols cargo build --release --locked
cargo generate-rpm
