# SPDX-License-Identifier: GPL-3.0-or-later
"""Parsers for pass-cli ``--output json``.

Shapes are taken from pass-cli 2.7.0 sources (protonpass/pass-cli and
protonpass/proton-pass-common, default serde derives):

* ``vault list``  -> ``{"vaults": [{"name", "vault_id", "share_id"}]}``
* ``item list``   -> ``{"items": [{"id", "share_id", "vault_id", "state",
  "flags", "create_time", "modify_time", "folder_id"?, "title",
  "item_type"}], "folders"?: [...]}`` (no secrets unless --show-secrets,
  which this app never passes)
* ``item view``   -> ``{"item": {..., "content": {"title", "note",
  "item_uuid", "content": {"Login": {...}}, "extra_fields": [{"name",
  "content": {"Hidden": "..."}}]}}, "attachments": [...]}``
* ``item totp``   -> ``{"<field name>": "<code>", ...}``
* ``info``        -> ``{"release_track", "id", "username"?, "email"?, ...}``

``parse_item_view`` deliberately drops every secret value: it only records
that a secret field exists. Secrets are fetched one at a time with
``item view --field`` when the user copies or reveals them.
"""

from __future__ import annotations

import json
from typing import Any

from .models import (
    Field,
    FieldKind,
    ItemDetail,
    ItemSummary,
    ItemType,
    SessionInfo,
    Vault,
)


class ParseError(ValueError):
    """pass-cli returned something we do not understand.

    The message never contains the raw payload (it may hold secrets).
    """


def _load(text: str | bytes) -> Any:
    try:
        return json.loads(text)
    except (TypeError, ValueError) as exc:
        raise ParseError(f"invalid JSON from pass-cli ({type(exc).__name__})") from None


def _str(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    return str(value)


def _untag(value: Any) -> tuple[str, Any]:
    """Split a serde externally tagged enum ``{"Variant": payload}``.

    Unit variants may be a bare string (``"Note"``) or ``{"Note": null}``.
    """
    if isinstance(value, str):
        return value, None
    if isinstance(value, dict) and len(value) == 1:
        (tag, payload), = value.items()
        return tag, payload
    if isinstance(value, dict) and "type" in value:  # internally tagged fallback
        return _str(value["type"]), value
    raise ParseError("unexpected enum encoding")


# --------------------------------------------------------------------------
# vault list / item list / info / totp
# --------------------------------------------------------------------------

def parse_vault_list(text: str | bytes) -> list[Vault]:
    data = _load(text)
    raw = data.get("vaults") if isinstance(data, dict) else data
    if not isinstance(raw, list):
        raise ParseError("vault list: missing 'vaults' array")
    vaults = []
    for entry in raw:
        if not isinstance(entry, dict) or not entry.get("share_id"):
            continue
        vaults.append(
            Vault(
                name=_str(entry.get("name")) or "Untitled vault",
                share_id=_str(entry["share_id"]),
                vault_id=_str(entry.get("vault_id")),
            )
        )
    return vaults


def parse_item_list(text: str | bytes) -> list[ItemSummary]:
    data = _load(text)
    raw = data.get("items") if isinstance(data, dict) else data
    if not isinstance(raw, list):
        raise ParseError("item list: missing 'items' array")
    items = []
    for entry in raw:
        if not isinstance(entry, dict) or not entry.get("id"):
            continue
        items.append(
            ItemSummary(
                id=_str(entry["id"]),
                share_id=_str(entry.get("share_id")),
                vault_id=_str(entry.get("vault_id")),
                title=_str(entry.get("title")) or "Untitled",
                item_type=ItemType.parse(entry.get("item_type")),
                state=_str(entry.get("state") or "Active").lower(),
                modify_time=_str(entry.get("modify_time")),
                create_time=_str(entry.get("create_time")),
            )
        )
    return items


def parse_totp(text: str | bytes) -> dict[str, str]:
    data = _load(text)
    if not isinstance(data, dict):
        raise ParseError("totp: expected an object")
    return {str(k): _str(v) for k, v in data.items()}


def parse_info(text: str | bytes) -> SessionInfo:
    data = _load(text)
    if not isinstance(data, dict):
        raise ParseError("info: expected an object")
    account = (
        _str(data.get("email"))
        or _str(data.get("username"))
        or _str(data.get("personal_access_token_name"))
    )
    return SessionInfo(
        logged_in=True,
        account=account,
        release_track=_str(data.get("release_track")),
        session_locked=bool(data.get("session_has_lock", False)),
    )


# --------------------------------------------------------------------------
# item view
# --------------------------------------------------------------------------

# (json key, pass-cli field key, label, kind)
_LOGIN_FIELDS = [
    ("email", "email", "Email", FieldKind.EMAIL),
    ("username", "username", "Username", FieldKind.TEXT),
    ("password", "password", "Password", FieldKind.HIDDEN),
    ("totp_uri", "totp", "One-Time Code", FieldKind.TOTP),
]
_CARD_FIELDS = [
    ("cardholder_name", "cardholder_name", "Cardholder Name", FieldKind.TEXT),
    ("number", "number", "Card Number", FieldKind.HIDDEN),
    ("expiration_date", "expiration_date", "Expiration Date", FieldKind.TEXT),
    ("verification_number", "verification_number", "Security Code", FieldKind.HIDDEN),
    ("pin", "pin", "PIN", FieldKind.HIDDEN),
]
_WIFI_FIELDS = [
    ("ssid", "ssid", "Network Name", FieldKind.TEXT),
    ("password", "password", "Password", FieldKind.HIDDEN),
]
_SSH_FIELDS = [
    ("public_key", "public_key", "Public Key", FieldKind.TEXT),
    ("private_key", "private_key", "Private Key", FieldKind.HIDDEN),
]
_IDENTITY_FIELDS = [
    ("full_name", "full_name", "Full Name", FieldKind.TEXT),
    ("email", "email", "Email", FieldKind.EMAIL),
    ("phone_number", "phone_number", "Phone", FieldKind.TEXT),
    ("birthdate", "birthdate", "Birthdate", FieldKind.TEXT),
    ("organization", "organization", "Organization", FieldKind.TEXT),
    ("street_address", "address", "Street Address", FieldKind.TEXT),
    ("zip_or_postal_code", "zip_or_postal_code", "ZIP or Postal Code", FieldKind.TEXT),
    ("city", "city", "City", FieldKind.TEXT),
    ("state_or_province", "state_or_province", "State or Province", FieldKind.TEXT),
    ("country_or_region", "country_or_region", "Country or Region", FieldKind.TEXT),
    ("company", "company", "Company", FieldKind.TEXT),
    ("job_title", "job_title", "Job Title", FieldKind.TEXT),
    ("website", "website", "Website", FieldKind.URL),
    ("social_security_number", "social_security_number", "Social Security Number", FieldKind.HIDDEN),
    ("passport_number", "passport_number", "Passport Number", FieldKind.HIDDEN),
    ("license_number", "license_number", "License Number", FieldKind.HIDDEN),
]


def _scalar_fields(payload: dict, spec, section: str = "") -> list[Field]:
    out = []
    for json_key, cli_key, label, kind in spec:
        raw = payload.get(json_key)
        if raw in (None, ""):
            continue
        is_secret = kind in (FieldKind.HIDDEN, FieldKind.TOTP)
        out.append(
            Field(
                key=cli_key,
                label=label,
                kind=kind,
                value=None if is_secret else _str(raw),
                section=section,
            )
        )
    return out


def _extra_field(entry: Any, section: str = "", prefix: str = "") -> Field | None:
    if not isinstance(entry, dict):
        return None
    name = _str(entry.get("name"))
    if not name:
        return None
    try:
        tag, payload = _untag(entry.get("content"))
    except ParseError:
        return None
    tag = tag.lower()
    key = f"{prefix}.{name}" if prefix else name
    if tag == "hidden":
        return Field(key, name, FieldKind.HIDDEN, None, section)
    if tag == "totp":
        return Field(key, name, FieldKind.TOTP, None, section)
    if tag == "timestamp":
        return Field(key, name, FieldKind.TIMESTAMP, _str(payload), section)
    return Field(key, name, FieldKind.TEXT, _str(payload), section)


def _extra_fields(entries: Any, section: str = "", prefix: str = "") -> list[Field]:
    if not isinstance(entries, list):
        return []
    return [f for f in (_extra_field(e, section, prefix) for e in entries) if f]


def _custom_sections(sections: Any) -> list[Field]:
    out: list[Field] = []
    if not isinstance(sections, list):
        return out
    for sec in sections:
        if not isinstance(sec, dict):
            continue
        name = _str(sec.get("section_name"))
        out += _extra_fields(sec.get("section_fields"), section=name, prefix=name)
    return out


def parse_item_view(text: str | bytes) -> ItemDetail:
    data = _load(text)
    if not isinstance(data, dict):
        raise ParseError("item view: expected an object")
    item = data.get("item", data)
    if not isinstance(item, dict):
        raise ParseError("item view: missing 'item'")
    content = item.get("content") or {}
    if not isinstance(content, dict):
        raise ParseError("item view: missing 'content'")

    tag, payload = _untag(content.get("content") or "Note")
    item_type = ItemType.parse(tag)
    payload = payload if isinstance(payload, dict) else {}

    fields: list[Field] = []
    urls: list[str] = []
    if item_type is ItemType.LOGIN:
        fields += _scalar_fields(payload, _LOGIN_FIELDS)
        urls = [_str(u) for u in payload.get("urls") or [] if u]
    elif item_type is ItemType.CREDIT_CARD:
        fields += _scalar_fields(payload, _CARD_FIELDS)
    elif item_type is ItemType.WIFI:
        fields += _scalar_fields(payload, _WIFI_FIELDS)
        fields += _custom_sections(payload.get("sections"))
    elif item_type is ItemType.SSH_KEY:
        fields += _scalar_fields(payload, _SSH_FIELDS)
        fields += _custom_sections(payload.get("sections"))
    elif item_type is ItemType.IDENTITY:
        fields += _scalar_fields(payload, _IDENTITY_FIELDS)
        for key in (
            "extra_personal_details",
            "extra_address_details",
            "extra_contact_details",
            "extra_work_details",
        ):
            fields += _extra_fields(payload.get(key))
        fields += _custom_sections(payload.get("extra_sections"))
    elif item_type is ItemType.CUSTOM:
        fields += _custom_sections(payload.get("sections"))

    fields += _extra_fields(content.get("extra_fields"), section="Custom Fields")

    attachments = data.get("attachments")
    return ItemDetail(
        id=_str(item.get("id")),
        share_id=_str(item.get("share_id")),
        vault_id=_str(item.get("vault_id")),
        title=_str(content.get("title")) or "Untitled",
        item_type=item_type,
        fields=fields,
        note=_str(content.get("note")),
        urls=urls,
        modify_time=_str(item.get("modify_time")),
        create_time=_str(item.get("create_time")),
        alias_email=_str(item.get("alias_email")),
        attachment_count=len(attachments) if isinstance(attachments, list) else 0,
    )
