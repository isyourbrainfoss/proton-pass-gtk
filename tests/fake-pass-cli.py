#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Test double only. Never talks to Proton or reads a session.
import json, os, pathlib, sys
args = sys.argv[1:]
assert "--show-secrets" not in args
# Secrets must never be passed on the command line or in the environment.
assert not any(s in a for a in args for s in ("FAKE-extra-password", "FAKE-template-password", "FAKE-card-number"))
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
elif args[:3] == ["item", "create", "credit-card"]:
    assert args[3:] == ["--share-id", "shr_1111", "--from-template", "-"], args
    t = json.load(sys.stdin)
    assert t["number"] == "FAKE-card-number" and t["cvv"] is None, t
    print("itm_new_card")
elif args[:2] == ["item", "untrash"]:
    assert args[2:] == ["--share-id", "shr_1111", "--item-id", "itm_03"], args
elif args[:2] == ["vault", "create"]:
    assert args[2:] == ["--name", "Demo vault"], args
    print("shr_new")
elif args[:3] == ["item", "alias", "create"]:
    assert args[3:7] == ["--share-id", "shr_1111", "--prefix", "shop"], args
    print(json.dumps({"id": "itm_alias", "alias": "shop.fake@passinbox.invalid"}))
elif args[:3] == ["password", "generate", "passphrase"]:
    print(json.dumps({"password": "FAKE-Demo-Phrase"}))
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
