#!/usr/bin/env bash
set -euo pipefail

if (( $# != 0 )); then
    printf 'Usage: %s\n' "${0##*/}" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

os_release=/etc/os-release
if [[ ! -r $os_release ]]; then
    os_release=/usr/lib/os-release
fi

ID='' ID_LIKE=''
if [[ -r $os_release ]]; then
    source "$os_release"
fi

case " $ID $ID_LIKE " in
    *" debian "* | *" ubuntu "*) ;;
    *)
        printf '%s\n' 'Build this DEB on Debian, Ubuntu, or a derivative, or use a matching container. On Fedora, use ./packaging/build-rpm.sh.' >&2
        exit 1
        ;;
esac

if ! cargo deb --version > /dev/null 2>&1; then
    cargo install --locked cargo-deb --version 3.8.0
fi

cargo build --release --locked
cargo deb --locked --no-build
