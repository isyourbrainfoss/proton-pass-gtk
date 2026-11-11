#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Renders the mock UI (fake data only) at phone and desktop sizes.
# Run under a display larger than 1280x800 plus window shadows, e.g.:
#   xvfb-run -a -s "-screen 0 1920x1200x24" build-aux/screenshots.sh
set -u
cd "$(dirname "$0")/.."
config=$(mktemp -d)
trap 'rm -rf "$config"' EXIT
export PPG_BACKEND=mock GTK_A11Y=none GDK_BACKEND=x11 GSK_RENDERER=cairo
# Never read or write the developer's real preferences.
export PPG_CONFIG_DIR="$config"
mkdir -p screenshots/rust
status=0
shot() {
  rm -f "$config/settings.ini"
  PPG_SIZE="$1" PPG_CAPTURE_STATE="$2" PPG_COLOR_SCHEME="$3" \
    PPG_CAPTURE="screenshots/rust/$4-$2-$3.png" \
    timeout 20 target/debug/proton-pass-gtk || { echo "FAILED: $4-$2-$3" >&2; status=1; }
}
phone="vaults list detail search signin copied-toast extra-password locked edit new trash preferences totp attachments"
desktop="list signin extra-password locked edit totp preferences attachments trash"
for scheme in light dark; do
  for state in $phone; do shot 360x720 "$state" "$scheme" phone-360x720; done
  for state in $desktop; do shot 1280x800 "$state" "$scheme" desktop-1280x800; done
done
exit $status
