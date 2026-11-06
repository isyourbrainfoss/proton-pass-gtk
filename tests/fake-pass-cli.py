#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Test double only. Never talks to Proton or reads a session.
import json, pathlib, sys
args = sys.argv[1:]
assert "--show-secrets" not in args
fixtures = pathlib.Path(__file__).parent / "fixtures"
if args == ["login"]:
    print("Please open https://account.proton.me/desktop/login?FAKE-test-only=1", flush=True)
elif args[:1] == ["logout"]:
    pass
elif "--field" in args and args[:2] == ["item", "view"]:
    print("FAKE-single-field-secret")
else:
    name = {("info",): "info", ("vault", "list"): "vault_list", ("item", "list"): "item_list", ("item", "view"): "item_view_login", ("item", "totp"): "totp"}.get(tuple(args[:2]), "info")
    if args[:1] != ["info"]:
        assert args[-2:] == ["--output", "json"]
    print((fixtures / (name + ".json")).read_text())
