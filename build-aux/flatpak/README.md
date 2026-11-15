# Flatpak packaging

GNOME 51 is the latest stable runtime as checked on 2026-11-06.
The checked-in `cargo-sources.json` is generated from `Cargo.lock` by the official
flatpak-cargo-generator (flatpak/flatpak-builder-tools). Regenerate after changing
Cargo dependencies:

```sh
uv run https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py Cargo.lock -o build-aux/flatpak/cargo-sources.json
```

```sh
flatpak install --user flathub org.gnome.Platform//51 org.gnome.Sdk//51 org.freedesktop.Sdk.Extension.rust-stable//26.08
flatpak-builder --user --force-clean --repo=flatpak-repo flatpak-build build-aux/flatpak/io.github.isyourbrainfoss.ProtonPassGtk.json
flatpak build-bundle flatpak-repo proton-pass-gtk-x86_64.flatpak io.github.isyourbrainfoss.ProtonPassGtk
flatpak install --user ./proton-pass-gtk-x86_64.flatpak
```

Use native aarch64 builds (or the CI ARM runner) for aarch64; the same manifest
selects the official aarch64 pass-cli binary by architecture. No host filesystem,
host process execution, session-bus wildcard or device access is requested.
IPC is needed for the fallback-X11 shared-memory path. Secret Service is the only
explicit D-Bus talk name. The network is required by pass-cli.

The official pass-cli 2.7.0 is GPL-3.0-or-later. Sources and matching version:
https://github.com/protonpass/pass-cli/tree/2.7.0 . Download SHA-256 hashes are
pinned from https://proton.me/download/pass-cli/versions.json; see ../fetch-pass-cli.sh.
No binaries are committed. This is not a Flathub submission or publication.
