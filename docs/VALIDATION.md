# Local verification — feature parity (`feat/feature-parity`)

Date: 2026-11-11 (Europe/Oslo / CET). Demo backend and fake pass-cli only; no
Proton account, credentials or real secrets used.

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo test --locked` | PASS: 39 parsing/command/preference tests + 11 parity tests (generator, search, drafts, templates, members) + 9 backend tests (fake CLI + mock flows) |
| `cargo test --locked --no-default-features` | PASS: 39 + 11 tests without GTK |
| Mock-only ignored GTK regression test under Xvfb | PASS (~36 s): previous checks plus search by username/email/alias, generator, opening every item type, trash/vault/share/invite/move/export dialogs, lock wipes the search index and generator history |
| Meson build + `meson test` | PASS (2/2) |
| Parity screenshots (`build-aux/screenshots-parity.sh`, Xvfb 1920x1200) | 80 images (20 states × 360x720 / 1280x800 × light/dark) in `screenshots/rust/parity/`, sampled and visually inspected |
| Fake CLI argv checks | PASS: credit-card number and other hidden template values never appear in argv |
| Real login, real writes, sharing, invites, export, passphrase via pass-cli | NOT TESTED (needs a real account; see the checklist in FEATURE-PARITY.md) |

# Local verification — mock features (`feat/mock-features`)

Date: 2026-11-11 (Europe/Oslo / CET). Demo backend only; no Proton account,
credentials or real secrets used.

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo test --locked` | PASS: 39 parsing/command/preference tests + 7 backend tests (fake CLI + mock) |
| `cargo test --locked --no-default-features` | PASS: 39 tests without GTK |
| Mock-only ignored GTK regression test under Xvfb | PASS (~35 s): previous checks plus attachment demo download, TOTP period labels, create/edit/trash, persisted preferences, lock/unlock wipe, auto-lock timing |
| Meson build + `meson test` | PASS: desktop metadata + Cargo suite (appstreamcli not installed locally; CI unchanged) |
| Screenshots (`build-aux/screenshots.sh`, Xvfb 1920x1200) | 46 images at exact sizes, light and dark, visually inspected |
| Extra password via `/dev/fd/3` pipe | PASS against the fake CLI (asserts absence from argv and environment) |
| Local Flatpak build | NOT RUN (SDK not installed here); CI builds both architectures |
| Real login, extra password, attachment download, real edits | NOT TESTED (needs a real account) |

# Local verification — Rust rewrite (PR #2)

Date: 2026-11-06 (Europe/Oslo / CET). No real Proton account or credentials used.

| Check | Result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo test --locked` | PASS: 33 parsing/command tests + 2 async backend tests; separate GUI test ignored by default |
| `cargo test --locked --no-default-features` | PASS: 33 tests without GTK |
| Mock-only ignored GTK regression test under Xvfb | PASS: breakpoints at 360 / 800 / 1280, search, clipboard ownership, 30-second clipboard expiry/re-hide, logout cleanup |
| Meson release build | PASS |
| `meson test -C _build --print-errorlogs` | PASS: desktop metadata, AppStream metadata, Cargo suite (3/3 groups) |
| DESTDIR staged installation + metadata validation | PASS; nothing installed into host /usr |
| ARM64 `cargo check` with `PKG_CONFIG_ALLOW_CROSS=1` | PASS: Rust type check only, host pkg-config metadata, no ARM linking/runtime claim |
| Official x86_64/aarch64 pass-cli SHA-256 | PASS: matches Proton versions.json (2.7.0) |
| Flatpak expanded manifest / cargo-sources | PASS: manifest parses and all 72 archive checksums match Cargo.lock |
| Offline Cargo vendor resolution from a Meson-style build subdirectory | PASS with the generated config/checksum layout |
| Local x86_64 `flatpak-builder` | BLOCKED at initialization: `org.gnome.Sdk/x86_64/51 not installed` |
| Local `.flatpak` bundle | NOT CREATED; do not claim a bundle path |
| Real login / authenticated JSON / final sandbox keyring + portal | NOT TESTED |

SDK 51 reports ~2.5 GB installed, matching rust-stable 26.08 extension ~2.0 GB.
Their combined size exceeds this box's free space before runtime/build output.
No unrelated project files or installed runtimes were deleted to make space.
The workflow builds both native architectures on GitHub Actions, producing private
workflow artifacts only; no release, registry or Flathub publication is configured.
CI results must be checked separately from these local results.

Native release executable on this box: `/workspace/proton-pass-gtk/_build/proton-pass-gtk`.
Run with `PPG_BACKEND=mock`; use `PPG_PASS_CLI` for the real backend.
Build caches are cleaned after verification; the executable and screenshots remain.

## GTK-rendered screenshots

All 16 images have the exact dimensions in their filenames. Light and dark contact
sheets were opened and visually inspected; the narrow breakpoint really collapses
both split views. These images contain only deliberately fake demo data.

- `screenshots/rust/desktop-1280x800-list-dark.png`
- `screenshots/rust/desktop-1280x800-list-light.png`
- `screenshots/rust/desktop-1280x800-signin-dark.png`
- `screenshots/rust/desktop-1280x800-signin-light.png`
- `screenshots/rust/phone-360x720-copied-toast-dark.png`
- `screenshots/rust/phone-360x720-copied-toast-light.png`
- `screenshots/rust/phone-360x720-detail-dark.png`
- `screenshots/rust/phone-360x720-detail-light.png`
- `screenshots/rust/phone-360x720-list-dark.png`
- `screenshots/rust/phone-360x720-list-light.png`
- `screenshots/rust/phone-360x720-search-dark.png`
- `screenshots/rust/phone-360x720-search-light.png`
- `screenshots/rust/phone-360x720-signin-dark.png`
- `screenshots/rust/phone-360x720-signin-light.png`
- `screenshots/rust/phone-360x720-vaults-dark.png`
- `screenshots/rust/phone-360x720-vaults-light.png`
