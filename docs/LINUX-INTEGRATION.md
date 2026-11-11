# Linux integration design

Date: 2026-11-11 (Europe/Oslo / CET). Target devices: an Arch desktop (GNOME
assumed, Wayland) and a Fairphone 5 running BengalOS/Phosh (Debian-based).
Read this together with `SECURITY-REVIEW.md`, especially S1: the current app
lock is a privacy screen, and nothing below changes that unless a real gate
(pass-cli session lock or an app PIN) is added first.

Principles:

* pass-cli and Proton's servers do the cryptography. This app is a viewer.
  Local integrations (fingerprint, autofill) are **convenience on top of an
  unlocked pass-cli session**. They are not a second layer of encryption.
* Only use portals or narrow, named D-Bus permissions. No `--filesystem=home`,
  no `--socket=session-bus`, no `--device=all`.
* Do not clone the official Proton Pass browser extension or desktop client.
  Fit in next to them.

---

## A. Fingerprint unlock of the app lock

### What it can and cannot be

The fingerprint reader can say "the enrolled user touched the sensor now".
fprintd returns a **yes/no**, not a key. So fingerprint unlock can only
dismiss an app-level gate. It cannot decrypt anything, and same-user code
could skip it. That is fine for a convenience unlock, *provided* the gate
it opens is honest about what it is (S1).

### Desktop (Arch, GNOME): fprintd over the system bus

* Service: `net.reactivated.Fprint` on the **system** bus (fprintd + libfprint;
  Arch packages `fprintd`, `libfprint`). The user enrols with GNOME Settings
  or `fprintd-enroll`.
* Flow (all asynchronous, from the GTK main loop through `gio::DBusProxy`/`zbus`):
  1. `Manager.GetDefaultDevice()` → device object path (error
     `NoSuchDevice` → hide the option).
  2. `Device.Claim("")` (empty string = the calling user). Polkit action
     `net.reactivated.fprint.device.verify` is allowed for active local
     sessions by default.
  3. `Device.VerifyStart("any")`, then listen to `VerifyStatus(result, done)`:
     `verify-match` → unlock; `verify-no-match` → count an attempt;
     `verify-retry-scan`, `verify-swipe-too-short`, `verify-finger-not-centered`,
     `verify-remove-and-retry` → show a hint; `verify-disconnected`,
     `verify-unknown-error` → fall back.
  4. Always `VerifyStop()` + `Release()`, also on window hide, lock-screen
     teardown and timeout (for example 30 s). Otherwise the device stays
     claimed and the GNOME lock screen or `sudo` cannot use it.
* Errors to handle: `AlreadyInUse` (the shell or another app has the
  device), `PermissionDenied` (polkit), `NoEnrolledPrints`.
* Flatpak: `--system-talk-name=net.reactivated.Fprint`. That is the only new
  permission. There is no portal for this yet: the "Local Authentication
  (Biometrics)" portal is still an open discussion
  (flatpak/xdg-desktop-portal#1275). The experimental *Credential* portal is
  for WebAuthn/passkeys, not app unlock. If a biometrics portal is merged,
  switch to it and drop the system-bus permission.
* UX: on the lock page, start verification automatically when the window is
  focused, show "Touch the fingerprint sensor" with a **Use PIN** button.
  After 3 failures or 30 s, stop and require the PIN/lock code. Require the
  PIN after reboot, and after N hours (like phones do).

### Phone (Fairphone 5, BengalOS/Phosh)

* Phosh does not include its own fingerprint stack. Where fingerprint unlock
  works on Phosh, it goes through **fprintd/libfprint**. If fprintd works on
  the device, the desktop code above works unchanged.
* **FP5 status is not ready, so don't promise it.** From the public
  `isyourbrainfoss/fp5-qtee` README (2026-10):
  * It is an **experimental** userspace session to the signed `focal32`
    trustlet over `/dev/tee0`. It is not an fprintd driver and has no D-Bus
    API.
  * Its Phosh unlock watcher is a demo. It runs the session binary through
    `sudo -n` and calls `loginctl unlock-session` on a match.
  * Saved templates match only on a specific kernel and module combination
    (7.2.0 + an out-of-tree `qcomtee-bridge.ko`). On the built-in 7.2.9
    driver every finger misses. The sensor IRQ does not wake the CPU from
    suspend.
  * A separate path by Marcus Ramberg (QSEECOM TEE driver + `focaltech-fp`
    misc driver + a libfprint `focaltechqsee` match-on-chip driver) aims for
    **fprintd**. That is the route that would make this app's fingerprint
    support work on FP5 without device-specific code.
* Recommendation for FP5: **build only against fprintd**. Do not call fp5-qtee,
  `sudo`, or `/dev/tee0` from this app. Hide the fingerprint option when
  `GetDefaultDevice` fails. When an fprintd driver for FP5 exists, test it
  then.
* Optional, off by default: "Unlock when the phone is unlocked". The app stays
  unlocked if the session lock was cleared within the last few seconds (logind
  `LockedHint`, or the screensaver-active monitor going false). That would let
  fp5-qtee's Phosh unlock (or the PIN) count for the app too. The trade-off is
  stated in the UI: then the app lock adds nothing on top of the phone lock.

### Fallback and the gate itself

* **Strong option**: pass-cli's own session lock (`session create-lock
  --idle-timeout …`, `session lock`, `session unlock`). The app runs `session
  unlock` under a PTY and writes the lock code to it (never argv or env). This
  blocks *all* pass-cli use until it is unlocked, not just this window.
  Fingerprint cannot unlock it unless the lock code is stored somewhere.
  Storing it in the Secret Service "behind" a fingerprint would make it as
  weak as the keyring, so **don't**. Use the fingerprint only for the app gate
  and the lock code after a timeout or reboot.
* **Simple option**: app PIN (Argon2id, salt, 0600 file, attempt back-off,
  sign out after 10 failures). Fingerprint can replace typing the PIN for N
  hours after the last PIN entry.
* Either way, write it in the UI: "Fingerprint and PIN protect this window.
  Your Proton session stays signed in on this device."

---

## B. Autofill on Linux

Linux has nothing like Android's Autofill framework or iOS's Password AutoFill.
As of late 2026, xdg-desktop-portal has **no password or autofill portal**.
The Secret portal only gives each app its own secret. The Credential portal is
experimental and only for WebAuthn. Wayland deliberately forbids global key
sniffing and synthetic input without consent. So "autofill" means one of
these:

| Option | Where | Verdict |
|---|---|---|
| Official Proton Pass browser extension (Firefox/Chromium) | Browser | **Use it.** It handles web forms, passkeys and URL matching properly. It signs in to Proton itself. Point to it in the README. |
| Native-messaging host from this app to a browser extension | Browser | **No.** We would need our own extension, which competes with Proton's, increases the attack surface, and needs a filesystem install outside Flatpak. |
| Quick picker + copy (global shortcut opens a small search window; Enter copies the password, Tab/Shift+Enter copies the username; clipboard auto-clear) | Desk | **Do it (next).** Uses the GlobalShortcuts portal: GNOME 48+, KDE. Without the portal, a custom keyboard shortcut in Settings runs `flatpak run … --quick` (GApplication action). |
| GNOME Shell search provider (`org.gnome.Shell.SearchProvider2`) | Desk | **Prototype, opt-in.** Item titles only, never secrets. No results while locked or signed out. Activating a result opens the item in the app (or copies after a confirmation). Note: this sends titles to the Shell, which is metadata exposure. |
| Auto-type through the RemoteDesktop portal (the KeePassXC 2.8 approach) | Desk | **Prototype later, opt-in.** Needs a consent dialog. Works on GNOME 48+/KDE, not wlroots, not Phosh. It can type into the wrong window, so require a fresh shortcut press and a short delay, and never type into this app itself. |
| X11 `xdotool`/XTest, `uinput`, `ydotool` | Desk | **No.** Insecure (X11 lets anyone sniff), needs root or the `input` group, breaks the sandbox. |
| Input-method engine (IBus, Wayland `input-method-v2`) offering "fill password" | Desk/Phone | **No for now.** Compositor- and OSK-specific (squeekboard/stevia on Phosh), large, and an IME sees every keystroke. That is too much trust for a side project. Look again only if Phosh gains an autofill hook. |

### Phone (Phosh) specifics

* Phosh's portal backend has no GlobalShortcuts or RemoteDesktop, so on the
  phone the realistic flow is **copy and paste**. Make it smooth:
  * a "Copy username, then password" flow: after copying the username, a toast
    with **Copy password** (one tap). Clear the clipboard as usual.
  * large touch targets, and copy buttons that are easy to reach with the
    thumb (already mostly there).
  * optional haptic feedback on copy through feedbackd (`org.sigxcpu.Feedback`,
    event `button-pressed`). Needs `--talk-name=org.sigxcpu.Feedback`, or use
    libfeedback.
* The on-screen keyboard: our password entries use `AdwPasswordEntryRow`
  (input purpose *password*), so squeekboard turns off suggestions and
  learning. Keep custom `GtkText` widgets with `InputPurpose::Password` /
  `InputHints::PRIVATE` for any new secret field.
* The browser on the phone (Firefox ESR / GNOME Web) can also run the official
  Proton Pass extension where supported. Firefox on Phosh can.

### What belongs where

* **This app:** browsing, search, copy with auto-clear, quick picker, search
  provider (titles), OTP display, privacy lock / real lock, fingerprint as
  convenience.
* **Proton's extension or official client:** web form filling, passkeys, URL
  matching, autosave of new logins.
* **Not us:** OS-wide autofill frameworks, input methods, kernel input
  injection.

---

## C. Other Linux / GNOME / Phosh integrations

| Integration | Value | Notes |
|---|---|---|
| Lock on screen lock and suspend (Inhibit portal monitor / `GtkApplication:screensaver-active`) | **High** | Fixes S2. No new permission when done through the portal. |
| Secret Service (already used by pass-cli with `PROTON_PASS_LINUX_KEYRING=dbus`) | Done | Explain the reboot behaviour: the session survives while the login keyring auto-unlocks. On Phosh, check that the keyring is unlocked at login (PIN = keyring password), otherwise every start gives a keyring prompt. |
| OpenURI portal for the login URL (already) | Done | Also use it for "Open website" on login items. Allow only `http(s)`, and show the host before opening. |
| FileChooser portal for attachments (already) | Done | Check B8 (document-portal paths). |
| `DBusActivatable=true` + desktop actions ("New login", "Lock", "Quick search") | Medium | Gives launcher right-click actions on GNOME and Phosh's app grid. Actions must open the app's UI, not show secrets. |
| GlobalShortcuts portal | Medium | Quick picker (desk only). |
| GNOME Shell search provider | Medium | Opt-in, titles only, respects lock. |
| Notifications (`GNotification` via the portal) | Low | Only content-free messages ("Clipboard cleared"). Never titles or values. Currently `X-GNOME-UsesNotifications=false`. Change that only if used. |
| Background portal / autostart | Low | Only needed if the quick picker should be instant. Otherwise start it on demand. |
| feedbackd haptics (Phosh) | Low | Nice on the phone. |
| `pass-cli ssh-agent` integration (expose `SSH_AUTH_SOCK`) | Prototype (desk, maybe outside Flatpak) | Real value for developers, but socket lifetime and sandbox paths need design. Keep it separate from the GUI's lock semantics. |
| Camera portal QR scan to create a login with TOTP | Later | pass-cli can set `totp_uri` only on create (template), not update. |
| System tray / StatusNotifier | **No** | Not in stock GNOME or Phosh. Background + search provider covers it. |
| GNOME Online Accounts | **No** | Proton is not a GOA provider. Login stays in pass-cli. |

---

## Staged plan

**Stage 0 — next PR (small, safe, high value; mock-testable):**

1. Honest wording for the lock (S1 step 1).
2. Lock on screen lock and suspend, and an idle clock that counts suspend (S2/B3).
3. `--flag=value` argument form + tests (S6/B5).
4. Fix B1 (stuck after failed unlock), B2 (cancel re-prompts for the extra
   password), B4 (reload while locked), B7 (untracked dialogs).
5. `PR_SET_DUMPABLE=0` (S7). Clipboard password hint (S8). Env scrub and
   override banner (S9).
6. Drop `note` from argv updates (S4).
7. Zeroising stdout reader + typed parsing that skips secrets (S3).
8. The missing security tests listed in `SECURITY-REVIEW.md`.

**Stage 1 — a real gate, then fingerprint:**

1. Prototype the pass-cli session lock under a PTY. Decide between that and an
   app PIN. Check how strong pass-cli's lock is before calling it a lock.
2. fprintd convenience unlock on the desk behind a setting, with Flatpak
   `--system-talk-name=net.reactivated.Fprint`, PIN fallback, and the PIN
   required after a reboot or timeout.

**Stage 2 — faster access:**

1. Quick picker window + GlobalShortcuts portal (desk). Copy-username-then-password
   flow (phone). Optional haptics.
2. Desktop actions and `DBusActivatable`.
3. Opt-in GNOME Shell search provider (titles only, respects lock).

**Stage 3 — prototypes, only if Stage 0–2 is solid:**

* Auto-type through the RemoteDesktop portal (desk, opt-in).
* FP5 fingerprint, **only once an fprintd driver exists** (Marcus Ramberg's
  libfprint path or an fprintd wrapper around fp5-qtee). No device-specific
  code in this app.
* `pass-cli ssh-agent` integration.

**Do not do:**

* A browser extension, native-messaging host, or anything that competes with
  Proton's official extension.
* X11 key injection, `uinput`/`ydotool`, an input-method engine.
* Storing the pass-cli lock code or any Proton secret "behind" a fingerprint
  as if that were encryption.
* Calling fp5-qtee, `sudo`, or `/dev/tee0` from this app.
* Showing item values in Shell search, notifications or desktop actions.
