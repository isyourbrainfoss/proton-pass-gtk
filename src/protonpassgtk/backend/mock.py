# SPDX-License-Identifier: GPL-3.0-or-later
"""Clearly fake demo backend (``PPG_BACKEND=mock``).

For screenshots, UI work and tests without a Proton account. All data is
invented; "secrets" are obviously fake placeholder strings. The TOTP seed
is the public RFC 6238 / Google Authenticator documentation example.

``PPG_MOCK_SIGNED_OUT=1`` starts signed out to show the sign-in page.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import os
import struct
import time

from gi.repository import GLib

from ..models import Field, FieldKind, ItemDetail, ItemSummary, ItemType, SessionInfo, Vault
from .base import Backend, BackendError, LoginHandle

_DEMO_TOTP_SEED = "JBSWY3DPEHPK3PXP"  # well-known public example seed

VAULTS = [
    Vault("Personal (demo)", "demo-share-personal", "demo-vault-1"),
    Vault("Work (demo)", "demo-share-work", "demo-vault-2"),
    Vault("Family (demo)", "demo-share-family", "demo-vault-3"),
]

# (vault index, id, title, type, username/email, has totp, urls, note)
_RAW = [
    (0, "i01", "Example Mail", ItemType.LOGIN, "demo.user@example.com", True,
     ["https://mail.example.com"], "Demo item. Not a real account."),
    (0, "i02", "Example Bank", ItemType.LOGIN, "demo-user-0042", True,
     ["https://bank.example.org"], ""),
    (0, "i03", "Example Forum", ItemType.LOGIN, "demo_forum_user", False,
     ["https://forum.example.net"], ""),
    (0, "i04", "Home Wi-Fi (demo)", ItemType.WIFI, "", False, [], ""),
    (0, "i05", "Demo Visa Card", ItemType.CREDIT_CARD, "", False, [], ""),
    (0, "i06", "Recipe Notes", ItemType.NOTE, "", False, [],
     "Pancakes: flour, milk, eggs.\nThis is demo text."),
    (0, "i07", "Newsletter alias", ItemType.ALIAS, "", False, [], ""),
    (1, "i08", "Example Git Host", ItemType.LOGIN, "demo-dev", True,
     ["https://git.example.com", "https://ci.example.com"], ""),
    (1, "i09", "Example Chat", ItemType.LOGIN, "demo.user@work.example", False,
     ["https://chat.example.com"], ""),
    (1, "i10", "Build Server SSH Key", ItemType.SSH_KEY, "", False, [], ""),
    (1, "i11", "Example Cloud Console", ItemType.LOGIN, "demo-admin", True,
     ["https://console.example.cloud"], ""),
    (2, "i12", "Example Streaming", ItemType.LOGIN, "family@example.com", False,
     ["https://tv.example.com"], ""),
    (2, "i13", "Demo Identity", ItemType.IDENTITY, "", False, [], ""),
    (2, "i14", "Example Shop", ItemType.LOGIN, "demo.shopper@example.com", False,
     ["https://shop.example.com"], ""),
]

_FAKE = "DEMO-not-a-real-secret"


def _summary(row) -> ItemSummary:
    v, iid, title, typ, *_ = row
    vault = VAULTS[v]
    return ItemSummary(
        id=iid, share_id=vault.share_id, vault_id=vault.vault_id, title=title,
        item_type=typ, state="active",
        modify_time="2026-10-%02dT09:30:00" % (int(iid[1:]) + 1),
        create_time="2026-01-15T12:00:00",
    )


def _detail(row) -> ItemDetail:
    v, iid, title, typ, user, totp, urls, note = row
    s = _summary(row)
    fields: list[Field] = []
    if typ is ItemType.LOGIN:
        kind = FieldKind.EMAIL if "@" in user else FieldKind.TEXT
        fields.append(Field("email" if "@" in user else "username",
                            "Email" if "@" in user else "Username", kind, user))
        fields.append(Field("password", "Password", FieldKind.HIDDEN))
        if totp:
            fields.append(Field("totp", "One-Time Code", FieldKind.TOTP))
    elif typ is ItemType.WIFI:
        fields += [Field("ssid", "Network Name", FieldKind.TEXT, "DemoNet-5G"),
                   Field("password", "Password", FieldKind.HIDDEN)]
    elif typ is ItemType.CREDIT_CARD:
        fields += [Field("cardholder_name", "Cardholder Name", FieldKind.TEXT, "Demo User"),
                   Field("number", "Card Number", FieldKind.HIDDEN),
                   Field("expiration_date", "Expiration Date", FieldKind.TEXT, "2030-12"),
                   Field("verification_number", "Security Code", FieldKind.HIDDEN)]
    elif typ is ItemType.SSH_KEY:
        fields += [Field("public_key", "Public Key", FieldKind.TEXT,
                         "ssh-ed25519 AAAADEMOONLYnotarealkey demo@example"),
                   Field("private_key", "Private Key", FieldKind.HIDDEN)]
    elif typ is ItemType.IDENTITY:
        fields += [Field("full_name", "Full Name", FieldKind.TEXT, "Demo User"),
                   Field("email", "Email", FieldKind.EMAIL, "demo.user@example.com"),
                   Field("city", "City", FieldKind.TEXT, "Example City")]
    elif typ is ItemType.ALIAS:
        pass
    if iid == "i08":
        fields.append(Field("Recovery codes", "Recovery codes", FieldKind.HIDDEN,
                            section="Custom Fields"))
    return ItemDetail(
        id=s.id, share_id=s.share_id, vault_id=s.vault_id, title=title, item_type=typ,
        fields=fields, note=note, urls=list(urls), modify_time=s.modify_time,
        create_time=s.create_time,
        alias_email="newsletter.demo123@passmail.example" if typ is ItemType.ALIAS else "",
    )


def demo_totp(seed: str = _DEMO_TOTP_SEED, at: float | None = None, step: int = 30) -> str:
    key = base64.b32decode(seed)
    counter = int((time.time() if at is None else at) // step)
    digest = hmac.new(key, struct.pack(">Q", counter), hashlib.sha1).digest()
    offset = digest[-1] & 0x0F
    code = (struct.unpack(">I", digest[offset:offset + 4])[0] & 0x7FFFFFFF) % 1_000_000
    return f"{code:06d}"


class _Handle(LoginHandle):
    def __init__(self):
        self.source = 0

    def cancel(self):
        if self.source:
            GLib.source_remove(self.source)
            self.source = 0


class MockBackend(Backend):
    name = "mock (demo data)"
    latency_ms = 120

    def __init__(self) -> None:
        self._logged_in = os.environ.get("PPG_MOCK_SIGNED_OUT") != "1"

    def _later(self, callback, result=None, error=None, delay=None):
        def fire():
            callback(result, error)
            return GLib.SOURCE_REMOVE

        return GLib.timeout_add(self.latency_ms if delay is None else delay, fire)

    def _session(self) -> SessionInfo:
        if not self._logged_in:
            return SessionInfo(logged_in=False)
        return SessionInfo(logged_in=True, account="demo.user@example.com", release_track="stable")

    def session_info(self, callback):
        self._later(callback, self._session())

    def login(self, on_url, callback):
        handle = _Handle()
        GLib.idle_add(lambda: on_url("https://account.proton.me/demo-login-not-real") and False)

        def done():
            handle.source = 0
            self._logged_in = True
            callback(self._session(), None)
            return GLib.SOURCE_REMOVE

        handle.source = GLib.timeout_add(2500, done)
        return handle

    def logout(self, callback):
        self._logged_in = False
        self._later(callback, True)

    def _guard(self, callback) -> bool:
        if not self._logged_in:
            from .base import NotLoggedIn

            self._later(callback, None, NotLoggedIn("Not signed in"))
            return False
        return True

    def list_vaults(self, callback):
        if self._guard(callback):
            self._later(callback, list(VAULTS))

    def list_items(self, vault, callback):
        if not self._guard(callback):
            return
        items = [_summary(r) for r in _RAW if VAULTS[r[0]].share_id == vault.share_id]
        items.sort(key=lambda i: i.title.lower())
        self._later(callback, items)

    def _row(self, share_id, item_id):
        for r in _RAW:
            if r[1] == item_id and VAULTS[r[0]].share_id == share_id:
                return r
        return None

    def view_item(self, item, callback):
        if not self._guard(callback):
            return
        row = self._row(item.share_id, item.id)
        if row is None:
            self._later(callback, None, BackendError("Item not found"))
        else:
            self._later(callback, _detail(row))

    def get_secret(self, share_id, item_id, field_key, callback):
        if self._guard(callback):
            self._later(callback, f"{_FAKE}-{field_key}", delay=60)

    def get_totp(self, share_id, item_id, field_key, callback):
        if self._guard(callback):
            self._later(callback, demo_totp(), delay=60)
