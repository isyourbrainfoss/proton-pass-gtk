# SPDX-License-Identifier: GPL-3.0-or-later
"""Clipboard with automatic clearing.

Copied secrets are offered as ``text/plain`` plus the
``x-kde-passwordManagerHint: secret`` hint so clipboard managers that honour
it (KDE Klipper, GNOME's Clipboard Indicator, cliphist...) skip them.

After ``timeout`` seconds the clipboard is cleared, but only if it still
holds the content we put there (we never wipe something the user copied
afterwards). It is also cleared on application shutdown.
"""

from __future__ import annotations

import os

from gi.repository import Gdk, GLib

DEFAULT_CLEAR_SECONDS = 30


def clear_seconds() -> int:
    try:
        return max(0, int(os.environ.get("PPG_CLIPBOARD_CLEAR_SECONDS", DEFAULT_CLEAR_SECONDS)))
    except ValueError:
        return DEFAULT_CLEAR_SECONDS


class SecretClipboard:
    def __init__(self, display: Gdk.Display | None = None) -> None:
        self._display = display or Gdk.Display.get_default()
        self._provider: Gdk.ContentProvider | None = None
        self._timeout_id = 0

    @property
    def clipboard(self) -> Gdk.Clipboard:
        return self._display.get_clipboard()

    def copy(self, text: str, secret: bool = True, timeout: int | None = None) -> int:
        """Copy ``text``; returns the auto-clear delay in seconds (0 = never)."""
        providers = [
            Gdk.ContentProvider.new_for_bytes(
                "text/plain;charset=utf-8", GLib.Bytes.new(text.encode("utf-8"))
            ),
            Gdk.ContentProvider.new_for_bytes(
                "text/plain", GLib.Bytes.new(text.encode("utf-8"))
            ),
            Gdk.ContentProvider.new_for_bytes(
                "UTF8_STRING", GLib.Bytes.new(text.encode("utf-8"))
            ),
        ]
        if secret:
            providers.append(
                Gdk.ContentProvider.new_for_bytes(
                    "x-kde-passwordManagerHint", GLib.Bytes.new(b"secret")
                )
            )
        provider = Gdk.ContentProvider.new_union(providers)
        self.clipboard.set_content(provider)
        self._provider = provider
        del text

        self._cancel_timeout()
        delay = clear_seconds() if timeout is None else timeout
        if secret and delay > 0:
            self._timeout_id = GLib.timeout_add_seconds(delay, self._on_timeout)
            return delay
        return 0

    def _cancel_timeout(self) -> None:
        if self._timeout_id:
            GLib.source_remove(self._timeout_id)
            self._timeout_id = 0

    def _on_timeout(self) -> bool:
        self._timeout_id = 0
        self.clear_if_ours()
        return GLib.SOURCE_REMOVE

    def clear_if_ours(self) -> bool:
        cb = self.clipboard
        if self._provider is not None and cb.is_local() and cb.get_content() is self._provider:
            # Replace with an empty string rather than unsetting: some
            # clipboard managers restore the last value when ownership is lost.
            cb.set_content(
                Gdk.ContentProvider.new_for_bytes("text/plain;charset=utf-8", GLib.Bytes.new(b""))
            )
            self._provider = None
            return True
        self._provider = None
        return False

    def shutdown(self) -> None:
        self._cancel_timeout()
        self.clear_if_ours()
