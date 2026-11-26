#!/usr/bin/env bash
set -euo pipefail

if (( $# != 0 )); then
    printf 'Usage: %s\n' "${0##*/}" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

if ! fedora_release=$(rpm --eval '%{?fedora}' 2>/dev/null) || [[ ! $fedora_release =~ ^[0-9]+$ ]]; then
    printf 'Build this RPM on the target Fedora release or in a matching Fedora container. On Ubuntu, use ./packaging/build-deb.sh.\n' >&2
    exit 1
fi

rpm_target_dir="target/packaging/rpm/fedora-$fedora_release"

if ! cargo generate-rpm --version > /dev/null 2>&1; then
    cargo install --locked cargo-generate-rpm --version 0.21.1
fi

CARGO_PROFILE_RELEASE_STRIP=symbols cargo build --release --locked --target-dir "$rpm_target_dir"
mkdir -p target/generate-rpm
cargo generate-rpm --target-dir "$rpm_target_dir" --output target/generate-rpm \
    --set-metadata "release = '1.fc$fedora_release'"
