# Security and bug review

Reviewed: `feat/mock-features` at `723e1a7` (2026-11-11, Europe/Oslo / CET).
`feat/feature-parity` had no commits beyond `723e1a7` at review time, so
there was nothing extra to review there.
Method: read-only code audit of `src/`, `tests/`, `build-aux/`, CI and the
Flatpak manifest, plus `cargo test --locked` (46 tests pass: 39 parsing, 7
backend). Some argument-parsing behaviour was checked against the official
`pass-cli` 2.7.0 binary **without a session** (no login, no Proton account,
no real secrets). Placeholders in this file such as `FAKE-…` are made up.

Severity scale: **High** = a user can reasonably be misled into a weaker
security posture, or secrets are exposed in normal use. **Medium** = real
exposure under plausible conditions, or a missing defence the app claims to
have. **Low** = defence in depth, or narrow conditions. **Info** = notes and
things the code already does well.

## Summary

| ID | Severity | Finding | One-line fix |
|---|---|---|---|
| S1 | High | "Lock" is only a privacy screen: Unlock needs no authentication, and the pass-cli session stays usable | Rename it and say so in the UI now. Later, put a real gate behind it (pass-cli session lock over a PTY, or an app PIN). |
| S2 | Medium | Auto-lock does not count time spent suspended, and there is no lock on suspend or screen lock | Lock on screensaver-active / PrepareForSleep. Measure idle time with a clock that keeps running in suspend. |
| S3 | Medium | Opening an item loads **all** of its secrets (`item view --output json`), and stdout is copied into buffers that are never zeroised | Read stdout into one zeroising buffer and parse into typed structs that skip secret fields. Fix the docs claim. |
| S4 | Medium | Real `item update` (behind `PPG_ENABLE_REAL_EDITS`) puts note/username/email/URL values on argv, which other local users can read | Never send `note` on argv. Treat every argv value as public. Keep the gate. |
| S5 | Medium (inherent) | Flatpak `--talk-name=org.freedesktop.secrets` exposes the user's whole Secret Service | Keep it (pass-cli needs it), but document it and justify it for Flathub. |
| S6 | Low | `--flag value` split arguments: a server-supplied ID or a custom field name starting with `-` is parsed as a flag (verified with 2.7.0) | Always pass `--flag=value`, and add a test. |
| S7 | Low | Process is dumpable: core dumps and same-user ptrace can read secrets | `prctl(PR_SET_DUMPABLE, 0)` at startup. |
| S8 | Low | Clipboard managers can keep copied secrets, because there is no password hint | Also offer `x-kde-passwordManagerHint: secret`. |
| S9 | Low | Full environment inherited by pass-cli; silent backend overrides | Scrub `PROTON_PASS_*` and show a banner when overrides are active. |
| S10 | Low | Any login failure with an extra password is reported as "wrong extra password" | Map only a real rejection to that error (see bug B2). |
| S11 | Low/Info | Secret strings remain in GTK widgets, AT-SPI and GString copies | Accepted limitation. Keep reveal short and document it. |
| S12 | Info | Supply chain: pins are good; a few gaps | Pin Actions by SHA. Add `cargo audit`/`cargo deny`. |

---

## S1 — High: the app lock is a privacy screen, not a lock

**Where:** `src/window.rs` `lock()` / `unlock()` (≈ lines 452–470), and the
locked StatusPage text (≈ 250–256).

`unlock()` only flips `locked` and reloads. Anyone holding the unlocked
phone or sitting at the desk can press **Unlock** and get every vault back.
On top of that:

* `lock()` does not touch pass-cli. The session in
  `~/.local/share/proton-pass-cli/.session/` (in Flatpak:
  `~/.var/app/io.github.isyourbrainfoss.ProtonPassGtk/data/…`) stays valid.
  Its key is in the Secret Service (`PROTON_PASS_LINUX_KEYRING=dbus`). So any
  process running as the same user can run `pass-cli item view …` while the
  app shows "Locked". That includes a terminal.
* With the dbus keyring the session **survives reboot**. gnome-keyring is
  normally unlocked by PAM at login, so after a reboot the app opens straight
  into the vaults with no prompt. With the kernel keyring (the pass-cli
  default outside Flatpak, which this app overrides) a reboot would log out.
* The wording ("Locked", the lock icon, "Unlock") looks like a password
  manager's cryptographic lock. Users of 1Password, Bitwarden and the Proton
  apps expect that a locked vault needs a secret to open.

What the lock does do well: it clears the clipboard (only when this app
still owns it), the detail view, item lists, tracked dialogs and in-memory
metadata, and in-flight results are dropped through the `epoch`/`request`
counters. Keep all of that.

**Recommended fix, in order:**

1. Now: change the copy to something like "Hidden — press Unlock to show
   your vaults again. This hides the app; it does not lock your Proton
   session." Optionally rename the action to "Hide" or "Privacy lock". Say in
   Preferences that "Lock after inactivity" hides content.
2. Next, a real gate, one of:
   * **pass-cli session lock** (`pass-cli session create-lock / lock /
     unlock`). That is the CLI's own barrier, so other pass-cli callers are
     blocked too. The lock code is read only through `rpassword` from the
     controlling TTY (`docs/PASS-CLI.md`), so the app has to run `session
     unlock` under a pseudo-terminal: `openpty`, then `setsid` +
     `TIOCSCTTY` in the child, and write the code to the master. Never put the
     code on argv or in the environment. Prototype this first, and verify how
     strong the pass-cli lock is (what is encrypted, and whether the server
     is involved) before calling it a security boundary.
   * **App PIN**: Argon2id hash with a per-install salt in the config dir
     (0600), attempt limit with back-off, and sign out after N failures. That
     is still only an app-level gate (same-user code can bypass it), but it
     stops casual physical access.
3. Fingerprint unlock (see `LINUX-INTEGRATION.md`) may only open the gate
   from step 2. It does not replace it.

**Tests to add:** the lock screen copy never claims encryption. `unlock()`
with a configured PIN rejects a wrong value and counts attempts. A real lock
runs `pass-cli session lock` (fake CLI asserts it).

## S2 — Medium: auto-lock skips suspend; no lock on screen lock or suspend

**Where:** `src/window.rs` `touch()` / `check_auto_lock()` /
`start_auto_lock()` (≈ 471–495).

`glib::monotonic_time()` is `CLOCK_MONOTONIC`, and on Linux that clock
**stops during suspend**. `timeout_add_seconds_local` also runs on monotonic
time. A phone with the screen off suspends almost at once, so with a 5-minute
auto-lock the app can sit in a pocket for hours and still be open on wake.
The same happens when a laptop lid is closed. The app also does not react to
the session screen lock.

**Fix:**

* Lock when the session locks or the device suspends:
  * `GtkApplication:screensaver-active` (needs `register-session`, and is
    backed by the portal or `org.gnome.ScreenSaver`/logind). Check whether
    it still works with GTK 4.20+/GNOME 51 and on Phosh.
  * or `org.freedesktop.portal.Inhibit.CreateMonitor` → `StateChanged`
    (`screensaver-active`, `session-state`). This needs no extra Flatpak
    permission. xdg-desktop-portal-phosh does not implement Inhibit, but the
    gtk/gnome fallback usually does. Verify on BengalOS.
  * Only if neither works: `org.freedesktop.login1` `PrepareForSleep`
    (needs `--system-talk-name=org.freedesktop.login1`).
* Measure idle time with `CLOCK_BOOTTIME` (through `libc`/`rustix`), or
  compare against `glib::real_time()` as a second check, so that time spent
  suspended counts.
* Optional: lock when the window has been hidden or unfocused for N minutes.

**Test:** inject a clock and simulate a jump of `auto_lock_minutes` + 1
across "suspend". Simulate a `screensaver-active` change → `locked`.

## S3 — Medium: opening an item fetches all its secrets; buffers are not zeroised

**Where:** `src/backend.rs` `detail()` → `commands::view()` (`item view
--output json`), `run_input()` (≈ 152–184), `src/model.rs` `json()`/`Json`.

The design says "one-field-at-a-time fetches", and copy/reveal do follow it
(`item view --field F`, `item totp`). But **every time an item is opened**,
`item view --output json` returns the password, the TOTP URI including its
seed, hidden custom fields, card number/CVV/PIN, the SSH private key and
identity numbers (see `tests/fixtures/item_view_login.json`; `item view` has
no option to leave secrets out). The model then throws them away, and the
`Json` wrapper zeroises the parsed `serde_json::Value` strings on drop. Several
copies still stay in freed memory:

1. `communicate_utf8_future` collects stdout in GLib buffers
   (`GMemoryOutputStream` grows by `realloc`, which leaves old blocks behind)
   and returns a `GString`. `out.map(String::from)` copies it, and the
   `GString` is freed without being zeroed.
2. `serde_json::from_str` builds strings that can reallocate as they grow, and
   escaped strings go through scratch buffers.
3. `Zeroizing<String>` only clears the final allocation.

**Fix:**

* Read stdout yourself: take `stdout_pipe()`, loop `read_bytes_future` into
  one `Zeroizing<Vec<u8>>` that is reserved up front (for example 64 KiB) and
  never grows past that without zeroising the old buffer, then parse from that
  slice.
* Deserialize into typed `serde` structs whose secret members are
  `serde::de::IgnoredAny`. Then secret values are never allocated as Rust
  strings at all (`totp_uri` still needs its `period`: parse it with a
  borrowing visitor and do not keep the rest).
* Ask Proton for an `item view` mode that leaves secrets out (or a
  metadata-only `--output json`). Until then, correct `README`/`docs` so they
  do not claim strict one-field-at-a-time behaviour.

**Test:** with the fake CLI, assert that the in-memory `Detail` has no
`FAKE-…` secret values (already partly covered), and add a test that the
reader never calls `String::from` on a GLib buffer (a structural/unit test of
the new reader).

## S4 — Medium (gated): argv updates expose non-password values

**Where:** `src/commands.rs` `update()` (≈ 73–82), `src/model.rs`
`LoginDraft::changed_fields()`, `src/backend.rs` `update_login()`.

`pass-cli item update` only takes `--field k=v` on the command line. The app
correctly refuses passwords there (`ERR_PASSWORD_ARGV`), but it does send
`title`, `username`, `email`, `url` and **`note`**. `/proc/<pid>/cmdline` can
be read by **every local user** unless `/proc` is mounted with `hidepid=`, and
it is easy to see in `ps`, crash reports and audit logs. People often keep
recovery codes, PINs or answers in notes.

This is off unless `PPG_ENABLE_REAL_EDITS=1`, which is the right default.

**Fix:** remove `note` from `changed_fields`. Tell the user that note edits
need another client until pass-cli has something like `item update
--from-template -`. Treat username/email/URL as low-sensitivity, but say in
the edit dialog that these are "visible to other local processes while
saving". Keep using stdin templates for create.

**Test:** `changed_fields` never returns `note`. The fake CLI rejects
`--field note=…`.

## S5 — Medium (inherent): Secret Service access in the sandbox

**Where:** `build-aux/flatpak/…ProtonPassGtk.json` finish-args.

`--talk-name=org.freedesktop.secrets` lets the app, and the bundled pass-cli,
read and write **every item in every unlocked collection** of the user's
keyring. The Secret Service has no isolation between apps. It is needed:
Flatpak's seccomp filter blocks `keyctl`, so the kernel keyring cannot be
used, and the `fs` key provider would leave the session key as plain text
under `~/.var/app/…`. The other finish-args are minimal and good: no
`--filesystem`, no devices, no session-bus wildcards, wayland plus
fallback-x11, ipc, network. Browser opening and file saving go through the
OpenURI and FileChooser portals, which need no extra permissions.

**Fix:** keep it, write down why in the Flatpak README and metainfo, and
check from time to time whether pass-cli supports the Secret portal
(`org.freedesktop.portal.Secret`, which gives each app its own secret) as a
key provider. If yes, switch.

## S6 — Low: argument construction with values that start with a dash

**Where:** every builder in `src/commands.rs`.

Arguments are passed as separate tokens (`--share-id`, `<id>`). With pass-cli
2.7.0 (clap), `item view --share-id -abc --item-id x` fails with `unexpected
argument '-a' found`, while `--share-id=-abc` is accepted. Proton IDs are
base64url-like and *can* begin with `-`. Custom field names
(`--field <name>`) are free text and can come from a **shared vault owned by
someone else**. A field named `--output` gives `a value is required for
'--field'`. Today this fails closed: the request errors out and nothing is
injected. But it is an injection-shaped pattern, and it breaks real items.

There is no shell anywhere (good): `gio::SubprocessLauncher::spawn` gets an
argv vector.

**Fix:** use `--share-id=<v>`, `--item-id=<v>`, `--field=<v>`,
`--attachment-id=<v>`, `--output=<path>` everywhere. Add a unit test that
every value-taking argument is in `--k=v` form, and a fake-CLI test with an ID
of `-FAKE` and a field named `--output`.

## S7 — Low: process memory can be dumped or traced

There is no `prctl(PR_SET_DUMPABLE, 0)` and no `RLIMIT_CORE = 0`. A crash
while a secret is revealed can write it into a systemd-coredump file. Where
Yama `ptrace_scope` is 0 (depends on the distro), any same-user process can
attach. **Fix:** set non-dumpable at the start of `main()` (also stops
same-user `/proc/<pid>/mem` reads). Optionally `mlock` the few secret buffers.
Check that this does not break the Flatpak or GTK Inspector use you rely on
when debugging (gate it behind a debug env var only in debug builds).

## S8 — Low: clipboard history managers

**Where:** `src/window.rs` `copy_field()` (≈ 782–820).

Clearing is done well: the timer clears only if this app's provider still
owns the clipboard, lock/close clear it, and a later copy replaces it. But
clipboard history tools (KDE Klipper, GPaste, CopyQ, cliphist on wlroots) may
keep the value for good. **Fix:** offer the extra MIME type
`x-kde-passwordManagerHint` with the value `secret` next to the text in a
`gdk::ContentProvider::new_union`. Klipper and others honour it. Also check:
on X11, whether GTK hands the clipboard to a `CLIPBOARD_MANAGER` when the app
exits (that would keep the secret after quit). If it does, clear before exit.
On Wayland the content disappears when the app quits.

## S9 — Low: environment inheritance and silent overrides

**Where:** `src/backend.rs` `launcher()` (≈ 66–78), `Backend::default()`.

pass-cli inherits the whole app environment except the two extra-password
variables. A stray `PROTON_PASS_KEY_PROVIDER=fs` or `PROTON_PASS_SESSION_DIR`
changes where the session key lives. `PPG_PASS_CLI` can point to any binary,
and `PPG_ENABLE_REAL_EDITS=1` turns on writes. You need control of the
environment to set these (for example `flatpak override --env`), so this is
not a remote risk, but nothing in the UI shows that it happened.
**Fix:** unset every `PROTON_PASS_*` except the ones set on purpose. Show an
`adw::Banner` when `PPG_PASS_CLI`, `PPG_ENABLE_REAL_EDITS` or `PPG_BACKEND`
is not the default.

## S10 — Low: misleading extra-password error

See bug B2. Any non-zero exit from `login` with an extra password becomes
`ERR_EXTRA_PASSWORD_REJECTED`. The user is asked again up to three times,
even after pressing **Cancel**, a network failure, or a closed browser. That
trains users to retype the secret.

## S11 — Low/Info: secrets in GTK and accessibility

Revealed values go into `AdwActionRow` subtitles. GTK keeps its own copies,
which are not zeroised. They are exposed through AT-SPI to any same-user
accessibility client, and through GTK Inspector when it is enabled. The extra
password is copied from the entry with `entry.text().to_string()` (the GString
is freed without zeroing). Check whether `AdwPasswordEntryRow` uses
`GtkPasswordEntryBuffer` (non-pageable memory that is wiped). These are normal
limits for a toolkit app. Keep the 30-second reveal, and show reveals only on
request (already done).

## S12 — Info: supply chain

* Good: pass-cli is pinned by URL and SHA-256 in the manifest. Cargo is
  `--locked` with a vendored `cargo-sources.json`, offline Flatpak build.
* `build-aux/fetch-pass-cli.sh` checks the binary against a hash from the
  **same origin** (`versions.json`). That catches corruption but not a
  compromised origin. When bumping, also compare with the GitHub release
  assets/checksums.
* GitHub Actions are pinned by tag (`actions/checkout@v4`,
  `dtolnay/rust-toolchain@stable`). Pin by commit SHA and add Dependabot for
  Actions. Add `cargo audit` or `cargo deny check advisories` to CI.

## Things already done well (keep them)

* No shell. argv vectors only. No `--show-secrets`.
* Extra password: close-on-exec pipe, read end passed as fd 3,
  `PROTON_PASS_EXTRA_PASSWORD_FILE=/dev/fd/3`, written fully and closed before
  spawn (no TOCTOU, no temp file), size limit, both env variants unset for
  every child. The parent's copy of the read end is dropped after spawn.
* stderr is silenced (`STDERR_SILENCE`). pass-cli's logs (even with
  `PASS_LOG_LEVEL=off` it still prints ERROR lines to stderr, verified with
  2.7.0) are never captured or logged.
* The login URL is restricted to `https://account.proton.me/desktop/login…`
  and opened through `GtkUriLauncher` (OpenURI portal), not logged.
* Create uses `--from-template -` on stdin. Password changes on argv are
  refused.
* `PPG_ENABLE_REAL_EDITS` gate. Trash instead of delete.
* Preferences: no secrets, 0600, atomic replace, values clamped so a
  hand-edited file cannot disable clipboard clearing.
* `epoch`/`request`/`generation` counters drop late async results after lock,
  navigation or re-reveal.
* Requests are serialised through one queue, with a 45-second timeout and
  `force_exit`.

---

# Bugs

| ID | Area | Bug | Reproduction | Suggested fix |
|---|---|---|---|---|
| B1 | Lock | Stuck on the lock screen if reload fails after Unlock | Real backend: lock, cut the network after `info` succeeds (or let `vault list` time out), press **Unlock**. `vaults` fails → toast, the stack stays on "locked", `locked=false`, so **Unlock** does nothing again (`unlock()` returns early). Only Ctrl+R or Sign out recovers. | In `unlock()`, set `locked=false` only after `show_vaults`, or show the sign-in/error page with Retry on failure. |
| B2 | Sign-in | Cancelling sign-in with "Ask for extra password" on re-opens the extra-password dialog with "Check your Pass extra password" | Real backend, extra password on: Sign in → enter a value → **Cancel** while the browser is open. `force_exit` → non-zero exit → `ERR_EXTRA_PASSWORD_REJECTED` → prompt again (up to 3 times). The same happens for network errors. | Track a `cancelled` flag in `cancel_login`. Only treat it as a rejection if pass-cli's output says so (match its message on stdout, or exit code if it is documented). |
| B3 | Auto-lock | Suspend time not counted (see S2) | Set auto-lock to 1 min, open a vault, suspend for 10 min, resume: still unlocked, and it locks ~1 min later only if idle. | See S2. |
| B4 | Lock | Edit/trash finishing after a lock reloads items into memory while locked | Mock: open Edit, Save, press Ctrl+L before it returns (easier with a slow fake CLI). `load_items` runs after `Ok` with no lock check and fills `items` (and may `open()` the item) behind the lock screen. | Early return in `load_items`/`open` when `locked`, or check `epoch` in the save/trash futures. |
| B5 | CLI args | IDs or custom field names starting with `-` fail (see S6) | Verified with pass-cli 2.7.0 without a session. | `--k=v`. |
| B6 | Sign-in | `login()` returns early on a read error without clearing `active`, so a later `cancel_login` hits a stale subprocess | Hard to trigger (stdout read error). | Use a guard that clears `active` on every exit path. |
| B7 | Dialogs | Sign-out confirmation and the Shortcuts dialog are not tracked, so they stay on top of the lock screen | Open the Sign-out dialog, wait for auto-lock. | `track_dialog` for both. |
| B8 | Flatpak | Real attachment download writes through pass-cli to a document-portal path (`/run/user/…/doc/…`). It works only if pass-cli writes that exact path in place (no temp file + rename in the same directory, which the portal grant does not allow) | Needs a real account in Flatpak. | Check. If it fails, let pass-cli write to stdout (if supported) or to `$XDG_CACHE_HOME` and copy through `gio::File` to the chosen file, then delete the temp file. |
| B9 | Create | Real create reports no ID, so the item is reopened by title. With duplicate titles the wrong item can open | Create a login with an existing title (real backend, gated). | Parse the create output once it is known, or match on the newest `create_time`. |
| B10 | Shortcuts | Ctrl+Shift+C/U/T do nothing for items without that field (cards, notes) | Open a card, press Ctrl+Shift+C. | Toast "No password field", or map to the primary hidden field. |
| B11 | TOTP | Periods over 300 s or missing are shown as 30 s. Only the countdown label is wrong: pass-cli makes the code | Item with `period=600`. | Allow up to 3600, or hide the countdown when unsure. |
| B12 | Mock | `PPG_MOCK_DOWNLOAD_DIR` joins the attachment name without cleaning it (`../`) | Mock and env var only. | Use `Path::file_name()` of the name. |

No crash paths were found in parsing: `model.rs` uses `get`/`as_*` and
fallbacks. The one `unwrap()` in `prefs::nearest` runs on a non-empty
constant. The `with_mock` unwrap follows an insert. Reveal/TOTP timers use
weak references and generation tokens, and they stop when rows are
destroyed. The clipboard timer compares providers, so copying A then B is
handled.

# Missing security tests (suggested)

1. Every command builder: no value-taking flag is split from its value, and
   no element contains a known `FAKE-` secret (property test over builders).
2. The fake CLI rejects `--field note=…` and any `--password`.
3. The fake CLI asserts that the child environment has no `PROTON_PASS_*`
   except `PROTON_PASS_LINUX_KEYRING` and (only for login)
   `PROTON_PASS_EXTRA_PASSWORD_FILE=/dev/fd/3`.
4. Extra password: fd 3 is not inherited by later, unrelated pass-cli calls
   (the fake CLI checks that `/dev/fd/3` is absent for non-login commands).
5. The preferences file never contains anything outside the known keys, even
   after the UI ran with fake secrets (serialize round trip with a fake
   secret in `last_vault` is rejected by the `is_ascii_graphic`/length check,
   which is already there; extend it to reject whitespace/newlines).
6. Lock: after `lock()`, `items`, `current` and list children are empty, and
   no late `load_items`/`open` repopulates them (B4).
7. Auto-lock with an injected clock across a simulated suspend (S2).
8. Cancelling login does not re-prompt for the extra password (B2).
