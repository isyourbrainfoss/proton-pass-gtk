# SPDX-License-Identifier: GPL-3.0-or-later
"""Plain data models shared by the UI and the backends.

Nothing in here imports GTK so it can be unit tested anywhere.

Secret values (passwords, card numbers, private keys, TOTP seeds...) are
never stored in these models. A :class:`Field` marked ``secret`` only
records that the value exists; the UI asks the backend for the value at the
moment the user copies or reveals it, and drops it right after.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum


class ItemType(str, Enum):
    LOGIN = "login"
    NOTE = "note"
    ALIAS = "alias"
    CREDIT_CARD = "credit_card"
    IDENTITY = "identity"
    SSH_KEY = "ssh_key"
    WIFI = "wifi"
    CUSTOM = "custom"
    UNKNOWN = "unknown"

    @classmethod
    def parse(cls, value: object) -> "ItemType":
        if isinstance(value, str):
            norm = value.strip().lower().replace("-", "_")
            # pass-cli JSON uses snake_case ("credit_card"); the serde enum
            # tag inside "content" is PascalCase ("CreditCard").
            aliases = {"creditcard": "credit_card", "sshkey": "ssh_key"}
            norm = aliases.get(norm, norm)
            for member in cls:
                if member.value == norm:
                    return member
        return cls.UNKNOWN


ITEM_TYPE_ICONS = {
    ItemType.LOGIN: "dialog-password-symbolic",
    ItemType.NOTE: "x-office-document-symbolic",
    ItemType.ALIAS: "mail-unread-symbolic",
    ItemType.CREDIT_CARD: "auth-smartcard-symbolic",
    ItemType.IDENTITY: "user-info-symbolic",
    ItemType.SSH_KEY: "system-lock-screen-symbolic",
    ItemType.WIFI: "network-wireless-symbolic",
    ItemType.CUSTOM: "document-properties-symbolic",
    ItemType.UNKNOWN: "dialog-question-symbolic",
}


class FieldKind(str, Enum):
    TEXT = "text"
    EMAIL = "email"
    URL = "url"
    HIDDEN = "hidden"  # secret, value never kept in the model
    TOTP = "totp"  # secret seed; only generated codes are shown
    NOTE = "note"  # multi-line text
    TIMESTAMP = "timestamp"


@dataclass(frozen=True)
class Vault:
    name: str
    share_id: str
    vault_id: str = ""


@dataclass(frozen=True)
class ItemSummary:
    id: str
    share_id: str
    vault_id: str
    title: str
    item_type: ItemType
    state: str = "active"
    modify_time: str = ""
    create_time: str = ""

    @property
    def key(self) -> tuple[str, str]:
        return (self.share_id, self.id)

    @property
    def trashed(self) -> bool:
        return self.state.lower() == "trashed"


@dataclass(frozen=True)
class Field:
    """One displayable field of an item.

    ``key`` is the name pass-cli accepts for ``item view --field`` /
    ``item totp --field`` (e.g. ``username``, ``password``, ``totp`` or a
    custom field title). ``value`` is ``None`` for secret kinds.
    """

    key: str
    label: str
    kind: FieldKind
    value: str | None = None
    section: str = ""

    @property
    def secret(self) -> bool:
        return self.kind in (FieldKind.HIDDEN, FieldKind.TOTP)


@dataclass
class ItemDetail:
    id: str
    share_id: str
    vault_id: str
    title: str
    item_type: ItemType
    fields: list[Field] = field(default_factory=list)
    note: str = ""
    urls: list[str] = field(default_factory=list)
    modify_time: str = ""
    create_time: str = ""
    alias_email: str = ""
    attachment_count: int = 0

    def fields_in(self, section: str) -> list[Field]:
        return [f for f in self.fields if f.section == section]

    @property
    def sections(self) -> list[str]:
        seen: list[str] = []
        for f in self.fields:
            if f.section not in seen:
                seen.append(f.section)
        return seen

    def first(self, kind: FieldKind) -> Field | None:
        return next((f for f in self.fields if f.kind == kind), None)


@dataclass(frozen=True)
class SessionInfo:
    logged_in: bool
    account: str = ""  # email/username or PAT name, for display only
    release_track: str = ""
    session_locked: bool = False
