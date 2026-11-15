#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Download Proton's official pass-cli into tools/ and verify its SHA-256
# against https://proton.me/download/pass-cli/versions.json.
# Usage: build-aux/fetch-pass-cli.sh [x86_64|aarch64]   (default: both)
set -eu
cd "$(dirname "$0")/.."
mkdir -p tools
curl -fsSL https://proton.me/download/pass-cli/versions.json -o tools/versions.json
version=$(jq -r '.passCliVersions.version' tools/versions.json)
for arch in ${1:-x86_64 aarch64}; do
  url=$(jq -r ".passCliVersions.urls.linux.\"$arch\".url" tools/versions.json)
  hash=$(jq -r ".passCliVersions.urls.linux.\"$arch\".hash" tools/versions.json)
  curl -fsSL "$url" -o "tools/pass-cli-linux-$arch"
  echo "$hash  tools/pass-cli-linux-$arch" | sha256sum -c -
  chmod +x "tools/pass-cli-linux-$arch"
done
host=$(uname -m)
[ -x "tools/pass-cli-linux-$host" ] && ln -sf "pass-cli-linux-$host" tools/pass-cli
echo "pass-cli $version in tools/"
