#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

app_id=io.github.secondscreen
work_dir=target/packaging/flatpak
flathub=https://dl.flathub.org/repo/flathub.flatpakrepo
bundle="target/packaging/secondscreen-$(flatpak --default-arch).flatpak"

cargo vendor --locked "$work_dir/vendor" > /dev/null

deps_args=()
if [[ ${1:-} != --skip-install-deps ]]; then
    flatpak remote-add --user --if-not-exists flathub "$flathub"
    deps_args=(--install-deps-from=flathub --assumeyes)
fi

flatpak-builder --user "${deps_args[@]}" \
    --force-clean --disable-rofiles-fuse --jobs="${CARGO_BUILD_JOBS:-2}" \
    --state-dir="$work_dir/state" --repo="$work_dir/repo" --default-branch=stable \
    "$work_dir/build" "packaging/$app_id.yml"
flatpak build-bundle --runtime-repo="$flathub" "$work_dir/repo" "$bundle" "$app_id" stable
printf 'Built %s\n' "$bundle"
