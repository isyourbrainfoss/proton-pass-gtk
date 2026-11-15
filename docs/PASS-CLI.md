# pass-cli notes (version 2.7.0, checked 2026-11-05)

Source: `pass-cli --help` (2.7.0, ba80a19), the official docs at
<https://protonpass.github.io/pass-cli/> and the sources at
<https://github.com/protonpass/pass-cli> (GPL-3.0-or-later).

## Install

* Manifest: `https://proton.me/download/pass-cli/versions.json`
  (`.passCliVersions.urls.linux.{x86_64,aarch64}.{url,hash}`, SHA-256).
* Binaries: `https://proton.me/download/pass-cli/<ver>/pass-cli-linux-{x86_64,aarch64}`,
  also attached to GitHub releases. Linux aarch64 is official.
* The aarch64 binary is dynamically linked against glibc (needs GLIBC 2.30 or
  later: libc, libm, libpthread, libdl). It will not run on musl
  (postmarketOS) without gcompat.
* `build-aux/fetch-pass-cli.sh` downloads the binaries and verifies them.

## Login

* `pass-cli login` uses **web login** by default. It prints
  `Please open the following URL…`, then a URL
  (`https://account.proton.me/desktop/login…`) and `Waiting for
  authentication to complete...` on **stdout**, and polls until the browser
  flow finishes. It supports SSO, 2FA and hardware keys. No TTY is needed:
  this was verified with stdin=/dev/null and stdout piped. Rust's stdout is
  line-buffered, so the URL arrives at once.
* Pass "extra password": if the account has one, the CLI reads it from
  `PROTON_PASS_EXTRA_PASSWORD`, from `PROTON_PASS_EXTRA_PASSWORD_FILE` (opened
  and read to EOF, then trimmed; `pass-cli/src/client.rs::get_value`), or from an
  interactive `rpassword` prompt on the TTY. The GUI uses the file variant with
  an inherited pipe: `PROTON_PASS_EXTRA_PASSWORD_FILE=/dev/fd/3`. It never sets
  `PROTON_PASS_EXTRA_PASSWORD` (environment is readable by same-user processes)
  and unsets both variables for every child.
* Personal access token: `pass-cli login --pat pst_…::KEY`. The help says
  `--pat`; the docs say `--personal-access-token`. You can also set the
  `PROTON_PASS_PERSONAL_ACCESS_TOKEN` env var. A PAT session lasts 2 h.
* Success prints `Successfully logged in as <email>`.
* Accounts must be allowed to use the CLI. Third-party reports say this
  needs a paid Pass plan. The CLI prints "Your account is not yet allowed to
  use our CLI".
* `pass-cli logout [--force]`.
* `pass-cli info --output json` returns `{release_track, id, username?,
  email?, personal_access_token_name?, session_has_lock,
  session_lock_after_seconds?}`. Without a session it exits 1, and stderr
  contains `Error: This operation requires an authenticated client`.

## Session and key storage

* Session dir: `~/.local/share/proton-pass-cli/.session/`. Override it with
  `PROTON_PASS_SESSION_DIR`.
* Key provider: `PROTON_PASS_KEY_PROVIDER=keyring` (default) or `fs`
  (`<session-dir>/local.key`). On Linux the keyring backend defaults to the
  **kernel keyring**, which is *cleared on reboot*.
  `PROTON_PASS_LINUX_KEYRING=dbus` uses the Secret Service (gnome-keyring)
  instead and persists. Recommended on Phosh, and required in Flatpak
  because flatpak's seccomp filter blocks keyctl.
* Session lock: `pass-cli session create-lock [--idle-timeout 30..900]|lock|unlock|remove-lock`.
  The lock code is read only via `rpassword` TTY prompts ("Enter lock code: ",
  `pass-cli/src/commands/session/unlock.rs`); there is no env/file input, so a
  GUI without a pseudo-terminal cannot unlock it. `info` reports
  `session_has_lock` / `session_lock_after_seconds`.
* Logs go to stderr. Set `PASS_LOG_LEVEL=trace|debug|info|warn|error|off`.

## Commands used by the app

| Purpose | Command | Output |
|---|---|---|
| Session check | `info --output json` | see above |
| Vaults | `vault list --output json` | `{"vaults":[{"name","vault_id","share_id"}]}` |
| Items | `item list --share-id S --filter-state active --sort-by alphabetic-asc --output json` | `{"items":[{"id","share_id","vault_id","state":"Active","flags":[],"create_time","modify_time","folder_id"?,"title","item_type":"login"}],"folders"?:[...]}`. No secrets unless `--show-secrets`, which the app never uses. |
| Item details | `item view --share-id S --item-id I --output json` | `{"item":{"id","share_id","vault_id","content":{"title","note","item_uuid","content":{"Login":{"email","username","password","urls","totp_uri","passkeys","autofill_urls"}},"extra_fields":[{"name","content":{"Text"/"Hidden"/"Totp"/"Timestamp": v}}]},"state","flags","create_time","modify_time","alias_email"?},"attachments":[...]}`. Serde externally tagged enums: `CreditCard`, `Identity`, `SshKey`, `Wifi`, `Custom`, and `Note`/`Alias` as unit variants. |
| One field value | `item view --share-id S --item-id I --field password` | raw value + `\n` on stdout |
| TOTP | `item totp --share-id S --item-id I [--field F] --output json` | `{"<field>": "123456", …}` (login TOTP field is `totp`) |
| Generate TOTP from URI | `totp generate` | |

### Write commands (wired, disabled for real accounts unless `PPG_ENABLE_REAL_EDITS=1`)

| Purpose | Command | Notes |
|---|---|---|
| Create login | `item create login --share-id S --from-template -` | JSON template on **stdin**: `{title, username, email, password, totp_uri, urls, note, extra_fields:[{kind,title,value}]}`. Output format is undocumented; the app reloads the list. |
| Update fields | `item update --share-id S --item-id I --field k=v …` | Values are **argv**. The app only sends non-secret fields (`title`, `username`, `email`, `url`, `note`) and refuses password changes. TOTP/time fields cannot be updated by the CLI. |
| Trash (reversible) | `item trash --share-id S --item-id I` | `item delete` is permanent and is not used. |
| Attachment download | `item attachment download --share-id S --item-id I --attachment-id A --output PATH` | Read-only. Attachment metadata in `item view` JSON is `attachments:[{id, size, content:{name, mime_type}}]` (from `view.rs`; exact JSON not yet seen from a real account). |

Other commands: `item create/update/delete/trash/untrash/move/share`,
`item attachment`, `item alias`, `password generate|score`, `vault
create/update/delete/share/member/transfer`, `folder`, `share`, `invite`,
`export`, `inject`, `run` (pass:// secret references, `?totp=code`), `ssh-agent`,
`settings view|set|unset` (default vault / default output format),
`personal-access-token`, `agent`, `user info`, `update`.

Field names accepted by `--field`: `title`, `note`, login (`email`,
`username`, `password`, `totp`, `totp_uri`, `urls`), card
(`cardholder_name`, `number`, `verification_number`/`cvv`/`cvc`,
`expiration_date`, `pin`), identity (`full_name`, `email`, `phone_number`,
`address`, `city`, …), ssh (`private_key`, `public_key`), wifi (`ssid`,
`password`), custom fields by title, section fields as `Section.field`.

Each invocation of pass-cli is a fresh process that loads the session and
may hit the network, so expect latency on a phone. The app runs calls
serially and caches item metadata (never secrets) in memory.
