#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Renders the feature-parity screens (fake demo data only) at phone and
# desktop sizes, light and dark, into screenshots/rust/parity/.
#   xvfb-run -a -s "-screen 0 1920x1200x24" build-aux/screenshots-parity.sh [state…]
set -u
cd "$(dirname "$0")/.."
config=$(mktemp -d)
trap 'rm -rf "$config"' EXIT
export PPG_BACKEND=mock GTK_A11Y=none GDK_BACKEND=x11 GSK_RENDERER=cairo
export PPG_CONFIG_DIR="$config"
out=screenshots/rust/parity
mkdir -p "$out"
states=${*:-"generator generator-passphrase search-username trash-view new-identity new-card new-alias new-sshkey detail-identity detail-sshkey detail-wifi detail-alias passkeys vault-new vault-share vault-delete share-item move invites export"}
status=0
for scheme in light dark; do
  for state in $states; do
    for size in phone-360x720 desktop-1280x800; do
      rm -f "$config/settings.ini"
      PPG_SIZE="${size#*-}" PPG_CAPTURE_STATE="$state" PPG_COLOR_SCHEME="$scheme" \
        PPG_CAPTURE="$out/$size-$state-$scheme.png" \
        timeout 20 target/debug/proton-pass-gtk || { echo "FAILED: $size-$state-$scheme" >&2; status=1; }
    done
  done
done
exit $status
