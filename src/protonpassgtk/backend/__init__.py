# SPDX-License-Identifier: GPL-3.0-or-later
"""Backend selection.

``PPG_BACKEND=mock`` selects the clearly fake demo backend (no account, no
network, no real secrets). Anything else, or unset, uses pass-cli.
"""

import os

from .base import Backend, BackendError, LoginHandle, NotLoggedIn, PassCliMissing


def create_backend() -> Backend:
    name = os.environ.get("PPG_BACKEND", "passcli").strip().lower()
    if name == "mock":
        from .mock import MockBackend

        return MockBackend()
    from .passcli import PassCliBackend

    return PassCliBackend()


__all__ = [
    "Backend",
    "BackendError",
    "LoginHandle",
    "NotLoggedIn",
    "PassCliMissing",
    "create_backend",
]
