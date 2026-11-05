# SPDX-License-Identifier: GPL-3.0-or-later
"""Command lines we send to pass-cli."""

from protonpassgtk.backend import argv as A


def test_json_everywhere():
    for args in (A.info(), A.vault_list(), A.item_list("s"), A.item_view("s", "i"),
                 A.item_totp("s", "i", "totp")):
        assert args[-2:] == ["--output", "json"]


def test_never_show_secrets():
    for args in (A.item_list("s"), A.item_view("s", "i"), A.item_field("s", "i", "password")):
        assert "--show-secrets" not in args


def test_item_field():
    assert A.item_field("shr", "itm", "password") == [
        "item", "view", "--share-id", "shr", "--item-id", "itm", "--field", "password"]


def test_login_is_web_login_without_credentials():
    assert A.login() == ["login"]


def test_find_login_url():
    line = "https://account.proton.me/authorize?code=abc123"
    assert A.find_login_url(line) == line
    assert A.find_login_url("Waiting for authentication to complete...") is None


def test_not_logged_in_detection():
    stderr = ("2026-11-05T07:44:24.051624Z ERROR pass-cli/src/main.rs:360: Command is not "
              "logout there is no session\nError: This operation requires an authenticated client\n")
    assert A.is_not_logged_in(stderr)
    assert A.summarize_error(stderr) == "This operation requires an authenticated client"


def test_summarize_error_strips_log_prefix():
    stderr = "2026-11-05T07:44:24Z ERROR pass-cli/src/x.rs:1: Vault not found\n"
    assert A.summarize_error(stderr) == "Vault not found"
    assert A.summarize_error("") == "pass-cli failed"


def test_child_env_quiet():
    env = A.child_env({"PATH": "/usr/bin"})
    assert env["PASS_LOG_LEVEL"] == "error"
    assert env["PATH"] == "/usr/bin"
