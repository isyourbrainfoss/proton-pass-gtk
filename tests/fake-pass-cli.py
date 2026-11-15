#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Test double only. Never talks to Proton or reads a session.
import json, os, pathlib, sys
args = sys.argv[1:]
assert "--show-secrets" not in args
# Secrets must never be passed on the command line or in the environment.
assert not any("FAKE-extra-password" in a or "FAKE-template-password" in a for a in args)
assert "PROTON_PASS_EXTRA_PASSWORD" not in os.environ
fixtures = pathlib.Path(__file__).parent / "fixtures"
if args == ["login"]:
    print("Please open https://account.proton.me/desktop/login?FAKE-test-only=1", flush=True)
    path = os.environ.get("PROTON_PASS_EXTRA_PASSWORD_FILE")
    if path is not None:
        assert path.startswith("/dev/fd/"), path
        with open(path) as f:
            if f.read().strip() != "FAKE-extra-password":
                sys.exit(1)
elif args[:1] == ["logout"]:
    pass
elif args[:3] == ["item", "create", "login"]:
    assert args[-2:] == ["--from-template", "-"]
    t = json.load(sys.stdin)
    assert t["title"] == "Demo template" and t["password"] == "FAKE-template-password"
    assert t["urls"] == ["https://demo.example.invalid"]
elif args[:2] == ["item", "update"]:
    assert args[2:6] == ["--share-id", "shr_1111", "--item-id", "itm_01"], args
    assert args[6:] == ["--field", "title=Renamed demo"], args
elif args[:2] == ["item", "trash"]:
    assert args[2:] == ["--share-id", "shr_1111", "--item-id", "itm_01"], args
elif "--field" in args and args[:2] == ["item", "view"]:
    print("FAKE-single-field-secret")
else:
    name = {("info",): "info", ("vault", "list"): "vault_list", ("item", "list"): "item_list", ("item", "view"): "item_view_login", ("item", "totp"): "totp"}.get(tuple(args[:2]), "info")
    if args[:1] != ["info"]:
        assert args[-2:] == ["--output", "json"]
    print((fixtures / (name + ".json")).read_text())
