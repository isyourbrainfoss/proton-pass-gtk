# SPDX-License-Identifier: GPL-3.0-or-later
"""Backend interface.

All methods are asynchronous: they return immediately and later invoke
``callback(result, error)`` on the GLib main loop, so the UI never blocks.
Exactly one of ``result`` / ``error`` is meaningful.

Secret-returning calls (``get_secret``, ``get_totp``) hand the value to the
callback only; backends must not cache, log or print it.
"""

from __future__ import annotations

from typing import Callable, Optional

from ..models import ItemDetail, ItemSummary, SessionInfo, Vault

Callback = Callable[[object, Optional["BackendError"]], None]


class BackendError(Exception):
    """User-presentable error. Messages must never include secret values."""


class NotLoggedIn(BackendError):
    pass


class PassCliMissing(BackendError):
    pass


class LoginHandle:
    """Returned by :meth:`Backend.login`; lets the UI cancel the flow."""

    def cancel(self) -> None:  # pragma: no cover - interface
        raise NotImplementedError


class Backend:
    #: shown in the About dialog / debug info
    name = "abstract"

    def session_info(self, callback: Callback) -> None:
        """Result: :class:`SessionInfo` (``logged_in`` False if no session)."""
        raise NotImplementedError

    def login(
        self,
        on_url: Callable[[str], None],
        callback: Callback,
    ) -> LoginHandle:
        """Start pass-cli's web login. ``on_url`` gets the browser URL.

        ``callback(SessionInfo, None)`` on success.
        """
        raise NotImplementedError

    def logout(self, callback: Callback) -> None:
        raise NotImplementedError

    def list_vaults(self, callback: Callback) -> None:
        """Result: ``list[Vault]``."""
        raise NotImplementedError

    def list_items(self, vault: Vault, callback: Callback) -> None:
        """Result: ``list[ItemSummary]`` (active items, no secrets)."""
        raise NotImplementedError

    def view_item(self, item: ItemSummary, callback: Callback) -> None:
        """Result: :class:`ItemDetail` with secret values stripped."""
        raise NotImplementedError

    def get_secret(self, share_id: str, item_id: str, field_key: str, callback: Callback) -> None:
        """Result: ``str`` – the single field value. Do not keep it."""
        raise NotImplementedError

    def get_totp(self, share_id: str, item_id: str, field_key: str, callback: Callback) -> None:
        """Result: ``str`` – the current one-time code."""
        raise NotImplementedError


__all__ = [
    "Backend",
    "BackendError",
    "Callback",
    "ItemDetail",
    "ItemSummary",
    "LoginHandle",
    "NotLoggedIn",
    "PassCliMissing",
    "SessionInfo",
    "Vault",
]
