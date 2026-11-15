# SPDX-License-Identifier: GPL-3.0-or-later
"""pass-cli JSON parsing. Pure Python, no GTK needed."""

import os

import pytest

from protonpassgtk import parsing
from protonpassgtk.models import FieldKind, ItemType

FIX = os.path.join(os.path.dirname(__file__), "fixtures")


def load(name: str) -> str:
    with open(os.path.join(FIX, name), encoding="utf-8") as fh:
        return fh.read()


def test_vault_list():
    vaults = parsing.parse_vault_list(load("vault_list.json"))
    assert [v.name for v in vaults] == ["Personal", "Work"]
    assert vaults[0].share_id == "shr_1111"
    assert vaults[1].vault_id == "vlt_BBBB"


def test_vault_list_rejects_garbage():
    with pytest.raises(parsing.ParseError):
        parsing.parse_vault_list("not json")
    with pytest.raises(parsing.ParseError):
        parsing.parse_vault_list('{"nope": 1}')


def test_item_list():
    items = parsing.parse_item_list(load("item_list.json"))
    assert len(items) == 3
    mail, card, ssh = items
    assert mail.title == "Example Mail" and mail.item_type is ItemType.LOGIN
    assert card.item_type is ItemType.CREDIT_CARD
    assert ssh.item_type is ItemType.SSH_KEY
    assert ssh.title == "Untitled"
    assert ssh.trashed and not mail.trashed
    assert mail.key == ("shr_1111", "itm_01")


@pytest.mark.parametrize("raw,expected", [
    ("login", ItemType.LOGIN), ("Login", ItemType.LOGIN),
    ("credit_card", ItemType.CREDIT_CARD), ("CreditCard", ItemType.CREDIT_CARD),
    ("credit-card", ItemType.CREDIT_CARD), ("SshKey", ItemType.SSH_KEY),
    ("wifi", ItemType.WIFI), ("whatever", ItemType.UNKNOWN), (None, ItemType.UNKNOWN),
])
def test_item_type_parse(raw, expected):
    assert ItemType.parse(raw) is expected


def test_item_view_login_fields():
    d = parsing.parse_item_view(load("item_view_login.json"))
    assert d.title == "Example Mail"
    assert d.item_type is ItemType.LOGIN
    assert d.note == "Demo note"
    assert d.urls == ["https://mail.example.com"]
    by_key = {f.key: f for f in d.fields}
    assert by_key["email"].value == "demo.user@example.com"
    assert by_key["username"].value == "demo_user"
    assert by_key["password"].kind is FieldKind.HIDDEN
    assert by_key["totp"].kind is FieldKind.TOTP
    assert by_key["Recovery key"].kind is FieldKind.HIDDEN
    assert by_key["Account ID"].value == "12345"
    assert by_key["Admin 2FA"].kind is FieldKind.TOTP
    assert by_key["Renewal"].kind is FieldKind.TIMESTAMP
    assert by_key["Recovery key"].section == "Custom Fields"


@pytest.mark.parametrize("fixture", [
    "item_view_login.json", "item_view_card.json", "item_view_custom.json",
])
def test_item_view_never_keeps_secrets(fixture):
    d = parsing.parse_item_view(load(fixture))
    dumped = repr(d)
    for marker in ("FAKE-", "FAKEFAKE", "4111FAKE", "otpauth://"):
        assert marker not in dumped, f"secret leaked into model: {marker}"
    for f in d.fields:
        if f.secret:
            assert f.value is None


def test_item_view_card():
    d = parsing.parse_item_view(load("item_view_card.json"))
    assert d.item_type is ItemType.CREDIT_CARD
    keys = [(f.key, f.kind) for f in d.fields]
    assert ("cardholder_name", FieldKind.TEXT) in keys
    assert ("number", FieldKind.HIDDEN) in keys
    assert ("verification_number", FieldKind.HIDDEN) in keys
    assert ("pin", FieldKind.HIDDEN) in keys
    assert d.attachment_count == 1


def test_item_view_note_unit_variant():
    d = parsing.parse_item_view(load("item_view_note.json"))
    assert d.item_type is ItemType.NOTE
    assert d.note == "Line one\nLine two"
    assert d.fields == []
    assert d.alias_email == ""


def test_item_view_custom_sections():
    d = parsing.parse_item_view(load("item_view_custom.json"))
    assert d.sections == ["Admin"]
    fields = d.fields_in("Admin")
    assert [f.key for f in fields] == ["Admin.user", "Admin.pass"]
    assert fields[0].value == "admin"
    assert fields[1].secret


def test_totp():
    codes = parsing.parse_totp(load("totp.json"))
    assert codes == {"TOTP 2": "622653", "TOTP 1": "119533", "totp": "152470"}


def test_info():
    info = parsing.parse_info(load("info.json"))
    assert info.logged_in and info.account == "demo.user@example.com"
    assert info.release_track == "stable"


def test_parse_error_does_not_echo_payload():
    with pytest.raises(parsing.ParseError) as exc:
        parsing.parse_item_view('{"item": "FAKE-secret-in-bad-payload"')
    assert "FAKE" not in str(exc.value)
