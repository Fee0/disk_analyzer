#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}

install -Dm755 "$here/disk-analyzer" "$prefix/bin/disk-analyzer"
install -Dm644 "$here/disk-analyzer.desktop" "$prefix/share/applications/disk-analyzer.desktop"
install -Dm644 "$here/disk-analyzer.png" "$prefix/share/icons/hicolor/256x256/apps/disk-analyzer.png"
install -Dm644 "$here/disk-analyzer.svg" "$prefix/share/icons/hicolor/scalable/apps/disk-analyzer.svg"

echo "Installed to $prefix. Make sure $prefix/bin is on your PATH."
