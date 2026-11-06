#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
cd "$(dirname "$0")/.."
export PPG_BACKEND=mock GTK_A11Y=none GDK_BACKEND=x11 GSK_RENDERER=cairo
mkdir -p screenshots/rust
shot() {
  PPG_SIZE="$1" PPG_CAPTURE_STATE="$2" PPG_COLOR_SCHEME="$3" \
    PPG_CAPTURE="screenshots/rust/$4-$2-$3.png" \
    timeout 20 target/debug/proton-pass-gtk
}
for scheme in light dark; do
  for state in vaults list detail search signin copied-toast; do
    shot 360x720 "$state" "$scheme" phone-360x720
  done
  for state in list signin; do shot 1280x800 "$state" "$scheme" desktop-1280x800; done
done
