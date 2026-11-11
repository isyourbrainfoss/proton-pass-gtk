# Proton Pass GTK — Rust edition

An **unofficial** adaptive GTK4/libadwaita frontend for Proton’s official
[pass-cli](https://github.com/protonpass/pass-cli). Not affiliated with Proton AG.
App ID: `io.github.isyourbrainfoss.ProtonPassGtk`. GPL-3.0-or-later.

This branch starts from `main`; the Python prototype in PR #1 / `feat/scaffold`
is deliberately untouched. No Proton session or real credentials are included.

## What is implemented

- Rust with gtk4-rs 0.10 / libadwaita-rs 0.8, Meson + Cargo. Minimum GTK 4.12 /
  libadwaita 1.5; these conservative bindings also work on the GNOME 51 runtime.
- `AdwApplication` and nested `AdwNavigationSplitView`s: three panes on desktop,
  two below 860sp, one below 600sp. On phones: **Vaults → Items → Details**, with
  native back navigation. Light/dark mode follows the system; menu override.
- Browser sign-in with `pass-cli login`, safe Proton-URL recognition, cancellation,
  sign-out confirmation, and serialized asynchronous `GioSubprocess` calls.
- Vault selection, active-item list, current-vault search and type-to-search.
- Login, card, identity, secure note, alias, SSH key, Wi-Fi, custom/section fields.
  Unknown types retain their title/note; attachments are counted, not downloaded.
- Copy username/password/one-time code with toasts; the clipboard is cleared after
  **30 seconds** (configurable 10–90 s) only if this app still owns it. Revealed
  values hide again after 30 seconds.
- **TOTP periods per field:** the `period` of each `otpauth://` URI is read at the
  parsing boundary (the seed is discarded), so 15/30/60-second (any 1–300 s) codes
  get a correct countdown and refresh on their own rollover. Bare base32 seeds
  and URIs without `period` use 30 s. Copy always fetches a fresh code.
- **Pass extra password:** an Adwaita password dialog (opt-in switch on the sign-in
  page and in Preferences, or shown automatically when the demo backend asks for
  it). The value goes to pass-cli through an inherited close-on-exec **pipe** as
  `PROTON_PASS_EXTRA_PASSWORD_FILE=/dev/fd/3` — never argv, never the environment,
  never disk. Up to three attempts, like pass-cli itself.
- **Lock and auto-lock:** Ctrl+L / lock button, and automatic lock after
  inactivity (default 5 min; never/1/2/5/10/15/30 min). Locking closes dialogs,
  clears this app's clipboard, revealed values, item details and lists, then shows
  a lock screen; Unlock reloads from pass-cli. This is a privacy lock, not a PIN:
  pass-cli's own server-enforced `session` lock is the real barrier (see below).
- **Preferences** (Ctrl+,): clipboard timeout, auto-lock, extra-password prompt,
  style (follow system/light/dark). Window size/maximized and the last vault are
  restored on start. Stored as a 0600 `key=value` file in
  `$XDG_CONFIG_HOME/proton-pass-gtk/settings.ini` (Flatpak:
  `~/.var/app/io.github.isyourbrainfoss.ProtonPassGtk/config/…`). No secrets.
- **Item editing (demo backend only):** new login (Ctrl+N), edit login (Ctrl+E) and
  *Move to Trash* with a confirmation dialog. The real backend wiring exists but is
  **disabled** unless `PPG_ENABLE_REAL_EDITS=1`: create sends a JSON template on
  stdin (`item create login --from-template -`), trash is reversible
  (`item trash`), and updates only pass non-secret fields because
  `item update --field k=v` puts values in argv; password changes are refused.
- **Attachments:** name/size/type list with a Save dialog. The demo writes a tiny
  fake text file. The real path calls `item attachment download … --output PATH`
  (read-only, untested against a real account).
- Accessible button/search/list labels, wrapping details, keyboard navigation.
- Explicit demo backend (`PPG_BACKEND=mock`) with a permanent **fake data** banner.
  `PPG_MOCK_EXTRA_PASSWORD=1` makes demo sign-in require an extra password
  (anything except `wrong` is accepted).

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Ctrl+F / type text | Search current vault |
| Ctrl+R | Refresh |
| Ctrl+L | Lock |
| Ctrl+N / Ctrl+E | New login / Edit login (demo backend) |
| Ctrl+, | Preferences |
| Ctrl+W / Ctrl+Q | Close / Quit |
| Ctrl+? | Shortcuts |
| Ctrl+Shift+U | Copy username (email fallback) |
| Ctrl+Shift+C | Copy password |
| Ctrl+Shift+T | Copy one-time code |

## Flatpak (intended installation method)

The manifest targets **GNOME 51** and the matching **rust-stable 26.08** SDK
extension, the latest stable runtime checked on 2026-11-06. It vendors all Cargo
sources for network-free compilation. It bundles the official **pass-cli 2.7.0**
for x86_64/aarch64 with hashes from Proton’s versions manifest, not a host binary.
Secret Service storage is enabled with `PROTON_PASS_LINUX_KEYRING=dbus`.

See [build instructions and permissions](build-aux/flatpak/README.md).
The GitHub Actions workflow tests Rust and builds local `.flatpak` artifacts on
native x86_64 and aarch64 runners; it does **not** publish to Flathub or a registry.

**Local packaging status:** `flatpak-builder` cannot initialize here because
`org.gnome.Sdk/x86_64/51` is not installed. The SDK (~2.5 GB installed) plus Rust
extension (~2.0 GB) exceed this box’s available space even before the runtime and
build output. No local `.flatpak` bundle is claimed. Use the CI artifact or build
on a machine with at least 8–10 GB free. Packaging remains unverified until that
build succeeds.

## Native development

```sh
# Debian 13; Rust stable, Meson and Ninja required
sudo apt install libgtk-4-dev libadwaita-1-dev meson ninja-build pkg-config
cargo build --locked
PPG_BACKEND=mock cargo run --locked
# Native real backend (requires official CLI + Secret Service):
PPG_PASS_CLI="$PWD/tools/pass-cli" cargo run --locked
```

`build-aux/fetch-pass-cli.sh` fetches/verifies the official CLI for development.
Never enter credentials in this app or put secrets in shell arguments. Sign-in
is on Proton’s website. For a Pass extra password, turn on “Ask for Pass extra
password” before signing in. CLI account eligibility depends on Proton;
third-party paid-plan reports are not an official guarantee.

GNOME-style Meson entry point (all Cargo dependencies must be fetched first):

```sh
cargo fetch --locked
meson setup _build --prefix=/usr
meson compile -C _build
meson test -C _build --print-errorlogs
# install via DESTDIR for testing, not into your host by default
DESTDIR="$PWD/_build/stage" meson install -C _build
```

## Tests and screenshots

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
# Display-dependent regression test (also runs in CI):
PPG_BACKEND=mock PPG_SIZE=360x720 GTK_A11Y=none GDK_BACKEND=x11 GSK_RENDERER=cairo xvfb-run -a cargo test --locked --bin proton-pass-gtk -- --ignored --test-threads=1
# Parser/argument tests without GUI libraries:
cargo test --locked --no-default-features
# Rust-only ARM64 type check using host pkg-config metadata (NOT an ARM link):
rustup target add aarch64-unknown-linux-gnu
PKG_CONFIG_ALLOW_CROSS=1 cargo check --locked --target aarch64-unknown-linux-gnu
# Deterministic GTK-rendered mock screenshots:
cargo build --locked
xvfb-run -a -s '-screen 0 1920x1200x24' dbus-run-session -- build-aux/screenshots.sh
```

Fixtures and CLI notes are reused from `feat/scaffold`. Tests cover parsing,
secret redaction, all item types, field arguments, URL allowlisting, a real async
subprocess contract with an offline fake CLI (which asserts that the extra
password and template passwords never reach argv or the environment and reads the
extra password from `/dev/fd/N`), gated real edit commands, mock create/edit/trash,
TOTP period parsing/countdown, and preference parsing/clamping/0600 saving. The
separate GTK regression test verifies all three adaptive widths, search, copying,
clipboard ownership preservation, 30-second clipboard clear/password re-hide,
attachment demo download, TOTP period labels, create/edit/trash reopening,
persisted preferences, lock/unlock cleanup, auto-lock timing and sign-out cleanup.
Screenshots are in [`screenshots/rust/`](screenshots/rust/): 360×720 phone vaults,
list, details, search, sign-in, copied toast, extra password, locked, edit, new
login, trash confirmation, preferences, TOTP periods and attachments; 1280×800
desktop list, sign-in, extra password, locked, edit, TOTP, preferences,
attachments and trash; all in light and dark. They render real GTK widgets, not
drawn mockups.

## Security boundaries and unfinished validation

- No `--show-secrets`, export, shell evaluation, or secret values in command lines.
- Hidden fields/TOTP seeds are omitted from retained detail models; values are
  fetched **one field at a time** only for copy/reveal. CLI detail JSON itself can
  contain secrets, so raw response strings and parsed JSON string values are
  zeroized on normal drop. Nothing is persisted or logged by this frontend.
- CLI stderr is silenced and errors use static messages rather than echoing data.
  Non-login requests time out after 45 seconds. Login is explicitly cancellable.
- Revealed/clipboard text necessarily lives transiently in GTK/GDK memory. This is
  not a hardened secret-memory allocator; zeroization cannot undo copies in GTK,
  the OS, swap or a clipboard manager. Clipboard managers may keep copies beyond
  our 30-second clear. Do not use one for sensitive credentials.
- The official CLI owns its session on disk and its key in Secret Service. This
  frontend does not implement cryptography or store passwords itself.
- Real account login, authenticated CLI JSON, portal/keyring behavior inside the
  final Flatpak, and native aarch64 linking/running still require validation.
  The ARM64 `cargo check` checks Rust code, not cross-library ABI/runtime behavior.
- The extra password crosses a pipe into pass-cli; GTK's entry buffer is cleared
  after reading, but like other typed text it transiently lives in GTK memory.
- pass-cli's server-side session lock (`pass-cli session create-lock|unlock`)
  reads its lock code only from an interactive TTY prompt (no env/file option in
  2.7.0), so this app cannot unlock it. If you use one, unlock it in a terminal;
  requests fail with a session-lock hint until then. The app's own lock is a
  privacy screen that wipes in-app data; it is not a cryptographic barrier.
- Editing a real account is disabled by default and untested. Editing secrets
  through `item update` is deliberately not offered (argv exposure). TOTP and
  custom fields are not editable here.
- This is a development build, not a security-audited password manager replacement.

[CLI commands, output formats and licensing](docs/PASS-CLI.md).
