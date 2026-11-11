# Feature parity: Proton Pass vs. pass-cli vs. Proton Pass GTK

Checked 2026-11-11 (Europe/Oslo). Branch `feat/feature-parity`.

Sources:

1. **Official Proton Pass apps** (web, desktop, browser extension, Android,
   iOS): <https://proton.me/pass/pricing>,
   <https://proton.me/support/item-history>,
   <https://proton.me/support/pass-folders>,
   <https://proton.me/support/how-to-use-pass-monitor>,
   <https://proton.me/blog/pass-secure-link-sharing>,
   <https://proton.me/blog/introducing-pass-folders-url-matching> (2026-10-15).
2. **Official `pass-cli` 2.7.0** (2026-11-03). Still the newest version in
   <https://proton.me/download/pass-cli/versions.json> on 2026-11-11. Every
   subcommand's `--help` was read, and the GPL sources at
   <https://github.com/protonpass/pass-cli> (commit `ff1a3c9`) were checked for
   JSON shapes. See [PASS-CLI.md](PASS-CLI.md).
3. **This app**: PR #2 (Rust rewrite), PR #3 (mock features) and this branch.

The CLI is a Pass Plus (paid) feature according to Proton's pricing page, so
anyone using this app already has a paid plan.

Legend:

* *Official*: ✅ all plans · 💎 paid plans only · — not offered.
* *pass-cli*: ✅ yes · ◐ partial · ✗ no.
* *GTK app*: ✅ done · ◐ partial · ✗ no.
* *Plan*: **done** · **mock-done** (implemented and tested against the demo
  backend; real-account writes stay behind `PPG_ENABLE_REAL_EDITS=1` until
  validated) · **needs login** (code exists, but a real account must confirm
  the JSON/behaviour) · **later** (possible with pass-cli, not built yet) ·
  **blocked** (pass-cli has no API for it) · **out of scope** (not something
  a CLI frontend can do).

> We cannot ship “all of Proton Pass”. Everything the official apps do on
> Proton's servers or inside the browser, with no pass-cli command behind
> it, is listed as blocked or out of scope. The UI does not pretend
> otherwise.

## Summary

| | Features |
|---|---|
| Implemented now (mock-tested) | Vault list/create/rename/delete, all 8 item types view + create + edit, Hide-my-email alias creation, password generator (random + passphrase) with session history, search across usernames/URLs/notes/aliases, trash view with restore and permanent delete, move item between vaults, vault sharing + member roles + removal, item sharing, invitations accept/decline, passkey metadata, attachment list + download, TOTP (any period), export (unencrypted HTML archive), extra password, privacy lock + auto-lock, preferences incl. generator defaults and search scope |
| Partial | Editing hidden values on real accounts (argv), SSH key creation (no passphrase), custom items (two fixed fields), alias management (create only), export (no encryption), item members (read + invite; no role change/removal UI for items) |
| Blocked: no pass-cli API | Item history/restore, Secure Links, Pass Monitor (weak/reused/2FA/dark web), import, attachment upload, alias mailboxes/domains/stats/toggle, emergency access, Sentinel, organization admin, notifications |
| Later (pass-cli supports it, not built yet) | Folders and subfolders, vault ownership transfer, dynamic custom fields, item-member role changes/removal, type filter chips, “empty trash”, SSH key import |
| Out of scope for a CLI frontend | Browser autofill, OS passkey provider, system-wide autofill on Linux/phone, Proton account settings |
| Needs a real login to finish | Every real-account write, `item view` JSON for passkeys/attachments, member/invite JSON, export runtime, Flatpak keyring/portal |

## Full table

### Vaults, folders, organisation

| Feature | Official | pass-cli | GTK app | Plan / notes |
|---|---|---|---|---|
| List vaults | ✅ | ✅ `vault list --output json` | ✅ | done |
| Create vault | ✅ (Free: 2 vaults, paid: 50) | ✅ `vault create --name` (prints share ID) | ✅ main menu → New Vault | mock-done |
| Rename vault | ✅ | ✅ `vault update --share-id --name` | ✅ vault menu | mock-done |
| Delete vault | ✅ | ✅ `vault delete --share-id` | ✅ vault menu; you must type the name | mock-done. Irreversible, so it is behind a typed confirmation. |
| Vault icon/colour | ✅ | ✗ | ✗ | blocked |
| Transfer vault ownership | ✅ | ✅ `vault transfer <member-share-id>` | ✗ | later (rare; needs a careful confirmation flow) |
| Folders and subfolders | 💎 (since 2026-10) | ✅ `folder list/create/update/delete/move`, `item move --to-folder-id`, `--folder-id` on create | ✗ (items in folders are listed flat, and search covers them) | **later**: folder tree in the item pane, create/rename/delete, move to folder. The CLI supports it fully. Deferred to keep this PR reviewable. |
| Default vault | ✅ | ✅ `settings set default-vault` | ◐ remembers the last opened vault locally | done (local) |
| Pin/favourite items | ✅ | ✗ (`flags` only) | ✗ | blocked |

### Items

| Feature | Official | pass-cli | GTK app | Plan / notes |
|---|---|---|---|---|
| Login: view | ✅ | ✅ `item view --output json` | ✅ | done |
| Login: create | ✅ | ✅ `item create login --from-template -` (stdin) | ✅ | mock-done |
| Login: edit non-secret fields | ✅ | ✅ `item update --field k=v` | ✅ | mock-done |
| Login: change password | ✅ | ◐ `item update --field password=…` puts the value on **argv** | ◐ demo only | **blocked for real accounts** by design: the value would be visible to other local processes through `/proc/<pid>/cmdline`. |
| Note | ✅ | ✅ create/update | ✅ view/create/edit | mock-done |
| Credit card | ✅ | ✅ template incl. `cvv`, `pin`, `expiration_date` | ✅ view/create/edit (YYYY-MM checked) | mock-done |
| Identity | ✅ | ✅ template (30+ fields) | ✅ view (all fields + extra sections); create/edit covers 14 common fields | mock-done; the less common fields are later |
| SSH key | ✅ | ✅ `item create ssh-key generate|import` | ◐ view; generate ed25519/RSA (key made inside pass-cli) | mock-done. A passphrase (`--password`) and import need a TTY prompt or a key file, so they are later. |
| Wi-Fi | ✅ | ✅ template (`ssid` required) | ✅ view/create/edit | mock-done |
| Custom item | ✅ | ✅ template with sections + extra fields | ◐ view any sections; create/edit offers one text and one hidden field | later: dynamic field list |
| Extra/custom fields on any item | ✅ | ✅ template `extra_fields`, `update --field-type` | ◐ shown and copyable, not editable | later |
| Hide-my-email alias: create | ✅ (Free: 10, paid: unlimited) | ✅ `item alias create --prefix --output json` | ✅ New → Hide-my-email Alias | mock-done. pass-cli picks the suffix and mailbox. |
| Alias: view address | ✅ | ✅ `alias_email` in `item view` | ✅ | done |
| Alias: enable/disable, mailboxes, custom domains, catch-all, contacts, send from alias, stats | 💎 (mostly) | ✗ | ✗ | blocked (SimpleLogin/alias API is not in pass-cli) |
| Move item to another vault | ✅ | ✅ `item move --from-share-id --item-id --to-share-id` | ✅ item menu | mock-done |
| Trash item | ✅ | ✅ `item trash` | ✅ | mock-done (PR #3) |
| Trash view | ✅ | ✅ `item list --filter-state trashed` | ✅ main menu → Trash (all vaults) | mock-done |
| Restore from trash | ✅ | ✅ `item untrash` | ✅ | mock-done |
| Delete permanently / empty trash | ✅ | ✅ `item delete` | ✅ per item, with confirmation; only for trashed items | mock-done; “empty trash” in one click is later |
| Item history / revisions / restore old version | 💎 | ✗ | ✗ | **blocked** |
| Attachments: list + download | 💎 | ✅ metadata in `item view`, `item attachment download` | ✅ | needs login (JSON not yet seen from a real account) |
| Attachments: upload/delete | 💎 | ✗ | ✗ | blocked |
| Passkeys stored in logins | ✅ | ◐ inside `item view` JSON (`passkeys[]`) | ✅ read-only list (site, user, created date); key material is dropped when parsing | done (display). Signing in with them is out of scope (see below). |
| TOTP / 2FA codes | 💎 | ✅ `item totp`, `totp generate` | ✅ live countdown, any period | done |
| Password generator | ✅ | ✅ `password generate random|passphrase --output json` (works offline, no session) | ✅ Ctrl+G, main menu, and “Generate” on password fields | done. Random passwords use the kernel CSPRNG inside the app (no subprocess, uniform rejection sampling). Passphrases use pass-cli's official word list; the demo uses a fake list. |
| Generator history | ✅ | ✗ | ◐ last 5, in memory only, masked, wiped on lock | done (deliberately never written to disk) |
| Password strength | ✅ | ◐ `password score <PASSWORD>` takes the password on argv | ◐ local entropy estimate for generated passwords | The CLI scorer is not used because of argv exposure. |
| Search | ✅ title, username, URL, note… | ◐ list has titles only; details via `item view` | ✅ title + type everywhere; usernames, emails, websites, notes, aliases and passkey sites when “Search item contents” is on (always on in the demo) | done. Real accounts: off by default, because it runs one `item view` per item, one at a time in the background. Hidden/TOTP values are dropped by the parser, so they never reach the index; it lives in memory and is wiped on lock. `--show-secrets` is never used. |
| Filter by type / sort | ✅ | ✅ `--filter-type`, `--sort-by` | ◐ type words match in search (“card”, “wifi”…) | later: type chips |

### Sharing

| Feature | Official | pass-cli | GTK app | Plan / notes |
|---|---|---|---|---|
| Share vault by email with a role | ✅ (Free: 2 people, paid: 10) | ✅ `vault share <email> --role` | ✅ vault menu → Share Vault | mock-done |
| List vault members | ✅ | ✅ `vault member list --output json` | ✅ | needs login (shape from `ShareMember` source) |
| Change member role / remove member | ✅ | ✅ `vault member update|remove` | ✅ | mock-done; owner protected |
| Share single item | 💎 | ✅ `item share <email> --role` | ✅ item menu → Share Item | mock-done |
| Item members | 💎 | ✅ `item member list|update|remove` | ◐ list + invite | later: role change/removal for item members |
| Pending invitations | ✅ | ✅ `invite list|accept|reject` | ✅ main menu → Invitations | needs login: `invite list` JSON shows `token`, while `accept` takes an invite ID. Which value works must be checked with a real invite. |
| Secure Links (public, expiring links) | 💎 | ✗ | ✗ | **blocked** |
| Groups / organisation sharing | business | ◐ `is_group_share` in member JSON; `user generate-report` | ✗ | out of scope for now |

### Security, account and settings

| Feature | Official | pass-cli | GTK app | Plan / notes |
|---|---|---|---|---|
| Sign in (SSO, 2FA, hardware keys) | ✅ | ✅ web login | ✅ | done (needs login to validate) |
| Pass extra password | ✅ | ✅ env/file/TTY | ✅ through a private pipe | done |
| App lock with PIN/password | ✅ (PIN, biometrics) | ◐ `session create-lock|unlock`, but the code is read only from a **TTY prompt** | ◐ privacy lock (Ctrl+L, auto-lock) without a PIN | **No PIN is implemented on purpose.** A short PIN stored or hashed locally can be brute-forced offline and would only pretend to be security. The lock clears all item data, revealed values, dialogs, the search index and the clipboard. The real protection is the CLI session plus the Secret Service keyring. Driving `session unlock` through a pseudo-terminal is a later option (see the integration section). |
| Biometric unlock | ✅ (mobile/desktop) | ✗ | ✗ | later, see the integration section |
| Pass Monitor: weak/reused passwords, missing 2FA | ✅ | ✗ | ✗ | **blocked / skipped**: it would need every password fetched into memory. A one-at-a-time local scan is possible but risky, so it is not built. |
| Dark web monitoring, compromised-password alerts | 💎 | ✗ | ✗ | blocked |
| Proton Sentinel, emergency access | 💎 | ✗ | ✗ | blocked |
| Import from other managers | ✅ | ✗ | ✗ | **blocked** |
| Export | ✅ (encrypted or CSV/JSON) | ◐ `export archive` (HTML, `--encrypt` reads its password from an env var or TTY only) | ◐ main menu → Export (unencrypted, with a strong warning and attachments option) | needs login. Encrypted export is blocked in the GUI (no env/TTY secrets); the dialog points to the terminal command. |
| Sync across devices | ✅ | ✅ (server-side; every CLI call is fresh) | ✅ Refresh (Ctrl+R) | done |
| Offline access | ✅ (cached) | ✗ (network per call) | ✗ | blocked |
| Notifications / in-app alerts | ✅ | ✗ | ✗ | blocked |
| Personal access tokens, AI agents, `inject`/`run`, ssh-agent | — (CLI-only) | ✅ | ✗ | out of scope for the GUI for now; ssh-agent integration is a later idea |
| App settings | ✅ | ◐ `settings` (default vault/format) | ✅ clipboard timeout, auto-lock, extra password, search scope, generator defaults, theme | done |
| Proton account settings, plans, billing | ✅ | ✗ | ✗ | out of scope (opens in a browser) |

### Platform integration

| Feature | Official | pass-cli | GTK app | Plan / notes |
|---|---|---|---|---|
| Browser autofill | ✅ extension | ✗ | ✗ | **out of scope**: use the official Proton Pass extension, which runs fine on Linux browsers |
| OS passkey provider | ✅ (Android/iOS/desktop) | ✗ | ✗ | **out of scope**: no Linux platform API exists, and we must not pretend to be one |
| Autofill in other apps (Linux/phone) | ✅ (Android/iOS) | ✗ | ✗ | see below; no implementation in this branch |
| Adaptive phone + desktop UI | ✅ | n/a | ✅ 360 px – desktop, light/dark, keyboard shortcuts, accessible labels | done |

## Linux desktop and phone integration (documented only)

This section is research only. Nothing here is built in this branch, and the
parity PR does not depend on it. A separate security review and design will
decide what to build.

### 1. Fingerprint unlock (fprintd, portals, GNOME, Phosh, Fairphone 5)

* **fprintd** (system D-Bus `net.reactivated.Fprint`) is the standard Linux
  fingerprint service. GNOME Settings enrols fingers, and PAM
  (`pam_fprintd`) uses them for login, sudo and polkit. An app can ask
  fprintd to *verify* a finger directly through `Device.Claim` and
  `VerifyStart`. polkit allows that for the active local session
  (`net.reactivated.fprint.device.verify`).
* **There is no xdg-desktop-portal for biometric verification.** A Flatpak
  would need `--system-talk-name=net.reactivated.Fprint`. That is a sandbox
  hole Flathub reviewers question, and it would only answer “a finger
  matched”, without releasing any key. The other route is polkit's
  `CheckAuthorization` with a custom action. The polkit agent may offer
  fingerprint or password depending on the PAM config, but a Flatpak cannot
  install its own polkit action.
* **What a fingerprint can protect here:** only this app's privacy lock.
  The CLI session key already sits in the Secret Service keyring, which is
  unlocked at login. A fingerprint gate is therefore a convenience, not a
  cryptographic barrier. It becomes a real barrier only together with
  pass-cli's own session lock (`session create-lock` / `unlock`), and that
  lock reads its code from a TTY. Combining them would need a
  pseudo-terminal wrapper and a lock code stored in the keyring behind the
  fingerprint check. That needs its own threat model.
* **GNOME:** fprintd + PAM works on supported readers. **Phosh:** the lock
  screen authenticates through PAM, so fingerprint unlock works where
  fprintd has a driver. App-level fingerprint prompts are not standard on
  Phosh either.
* **Fairphone 5:** the side-mounted reader needs a libfprint driver for its
  sensor. Linux FP5 ports (postmarketOS/Mobian, Qualcomm QCM6490) are not
  known to us to have working fingerprint support yet. This must be checked
  on the device (`fprintd-list`, `fprintd-enroll`) before any design work.
* **Plan:** *implement later* (privacy-lock convenience via fprintd or
  polkit, opt-in, after the security review). *Blocked* as a real secret
  barrier until pass-cli accepts a lock code without a TTY, or a PTY design
  is approved.

### 2. Autofill on Linux

* **Browser:** the official Proton Pass extension (Firefox, Chromium) already
  autofills on Linux, with its own sign-in. A native-messaging host that lets
  the extension talk to this app is **not possible**: the extension's
  protocol is Proton's, and it only talks to Proton's servers and apps. A
  custom extension talking to us over native messaging is possible in theory.
  It would duplicate the official extension, and Flatpak browsers need the
  still-experimental WebExtensions portal for native messaging. *Out of
  scope.*
* **Desktop portals:** there is no autofill or credential portal in
  xdg-desktop-portal. The RemoteDesktop/InputCapture portals could
  synthesise keystrokes (“type password”). That is powerful, needs user
  consent each session and is easy to misuse, so it is *implement-later at
  most*, after the security review. Copy with auto-clearing clipboard
  (already built) is the safe default.
* **Phone (Phosh):** autofill could in theory be done by an input method
  (Wayland `zwp_input_method_v2`, e.g. a squeekboard / phosh-osk-stub
  plug-in) that offers credentials for the focused field. No such framework
  exists today. It would run inside the compositor's trust boundary and needs
  a design of its own. *Blocked / research.*
* **pass-cli** offers nothing for autofill (`inject`/`run` are for files
  and environment variables, not GUI fields).

## Needs a real login before claiming “works”

1. Real `item view` JSON for passkeys, attachments, aliases, identities and
   custom items (the shapes come from pass-cli sources and fixtures).
2. Every write behind `PPG_ENABLE_REAL_EDITS=1`: create for each type, update
   field names (`urls`, `address`, …). Unknown names make pass-cli **add a
   new custom field** instead of failing. Also trash/untrash/delete, move
   (new item ID?), vault create/rename/delete, share, member role/remove,
   invite accept/reject (token vs. ID), alias create, SSH key generate.
3. `export archive` runtime and file permissions inside the Flatpak (portal
   file chooser path).
4. Background “Search item contents” speed on a phone with many items.
