#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Regenerate screenshots/ with the mock backend under Xvfb (sequential).
set -eu
cd "$(dirname "$0")/.."
export GTK_A11Y=none GDK_BACKEND=x11
Xvfb :87 -screen 0 1600x1000x24 >/dev/null 2>&1 &
XPID=$!
trap 'kill $XPID' EXIT
sleep 1
export DISPLAY=:87
shot() { # size state scheme name
  python3 build-aux/screenshot.py --size "$1" --state "$2" --scheme "$3" \
    --out "screenshots/$4-$2-$3.png"
}
for scheme in light dark; do
  for state in vaults list detail search signin; do shot 360x720 $state $scheme phone-360x720; done
  for state in list detail; do shot 360x740 $state $scheme phone-360x740; done
  shot 1280x800 list $scheme desktop-1280x800
  shot 1280x800 signin $scheme desktop-1280x800
  shot 800x700 detail $scheme tablet-800x700
  python3 build-aux/screenshot.py --size 360x720 --state detail --scheme $scheme \
    --action copy-password --out "screenshots/phone-360x720-copied-toast-$scheme.png"
done
