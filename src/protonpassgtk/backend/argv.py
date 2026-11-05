# SPDX-License-Identifier: GPL-3.0-or-later
"""Pure helpers that build pass-cli command lines.

Kept free of GTK so they can be unit tested. Rules:

* never put secrets on the command line (argv is visible in /proc);
* always request JSON output where pass-cli offers it;
* never pass ``--show-secrets``.
"""

from __future__ import annotations

import os
import re
import shutil

PASS_CLI_ENV = "PPG_PASS_CLI"


def find_pass_cli() -> str | None:
    """Locate the pass-cli binary: $PPG_PASS_CLI, $PATH, ~/.local/bin."""
    override = os.environ.get(PASS_CLI_ENV)
    if override:
        return override if os.access(override, os.X_OK) else None
    found = shutil.which("pass-cli")
    if found:
        return found
    for candidate in (
        os.path.expanduser("~/.local/bin/pass-cli"),
        "/app/bin/pass-cli",  # Flatpak
        "/usr/local/bin/pass-cli",
    ):
        if os.access(candidate, os.X_OK):
            return candidate
    return None


def info() -> list[str]:
    return ["info", "--output", "json"]


def login() -> list[str]:
    # Web login: pass-cli prints a URL and polls until the browser flow
    # (SSO / 2FA / security keys supported) completes.
    return ["login"]


def logout() -> list[str]:
    return ["logout"]


def vault_list() -> list[str]:
    return ["vault", "list", "--output", "json"]


def item_list(share_id: str) -> list[str]:
    return [
        "item", "list",
        "--share-id", share_id,
        "--filter-state", "active",
        "--sort-by", "alphabetic-asc",
        "--output", "json",
    ]


def item_view(share_id: str, item_id: str) -> list[str]:
    return ["item", "view", "--share-id", share_id, "--item-id", item_id, "--output", "json"]


def item_field(share_id: str, item_id: str, field_key: str) -> list[str]:
    # Prints only the raw value of one field on stdout.
    return ["item", "view", "--share-id", share_id, "--item-id", item_id, "--field", field_key]


def item_totp(share_id: str, item_id: str, field_key: str) -> list[str]:
    return [
        "item", "totp",
        "--share-id", share_id,
        "--item-id", item_id,
        "--field", field_key,
        "--output", "json",
    ]


def child_env(base: dict[str, str] | None = None) -> dict[str, str]:
    env = dict(os.environ if base is None else base)
    # Logs go to stderr; keep them quiet so we never capture anything
    # verbose. Errors are still reported on stderr as "Error: ...".
    env.setdefault("PASS_LOG_LEVEL", "error")
    env["NO_COLOR"] = "1"
    return env


_URL_RE = re.compile(r"https://[^\s<>\"']+")
_LOG_PREFIX_RE = re.compile(r"^\d{4}-\d\d-\d\dT\S+\s+(ERROR|WARN|INFO|DEBUG|TRACE)\s+\S+\s*")


def find_login_url(line: str) -> str | None:
    m = _URL_RE.search(line)
    return m.group(0).rstrip(".,)") if m else None


NOT_LOGGED_IN_MARKERS = (
    "requires an authenticated client",
    "there is no session",
    "not logged in",
)


def is_not_logged_in(stderr: str) -> bool:
    low = stderr.lower()
    return any(m in low for m in NOT_LOGGED_IN_MARKERS)


def summarize_error(stderr: str, limit: int = 200) -> str:
    """One short human line from pass-cli stderr, for toasts.

    Takes the last "Error:" line (anyhow output) or the last non-empty line.
    """
    lines = [ln.strip() for ln in stderr.splitlines() if ln.strip()]
    if not lines:
        return "pass-cli failed"
    errors = [ln for ln in lines if ln.startswith("Error:")]
    line = errors[-1] if errors else _LOG_PREFIX_RE.sub("", lines[-1])
    line = line.removeprefix("Error:").strip() or "pass-cli failed"
    return line[:limit]
