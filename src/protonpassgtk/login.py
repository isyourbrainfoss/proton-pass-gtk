# SPDX-License-Identifier: GPL-3.0-or-later
"""Sign-in dialog driving ``pass-cli login`` (web login).

pass-cli prints a URL; we open it with the desktop's browser (portal-aware
Gtk.UriLauncher) and wait for pass-cli to finish polling. The browser flow
supports every Proton login method (SSO, 2FA, security keys). This app
never sees the account password.
"""

from __future__ import annotations

from gettext import gettext as _
from typing import Callable

from gi.repository import Adw, Gtk

from .backend import Backend, BackendError, LoginHandle
from .widgets import spinner


class LoginDialog(Adw.Dialog):
    def __init__(self, backend: Backend, parent: Gtk.Widget,
                 on_done: Callable[[object, BackendError | None], None]) -> None:
        super().__init__(title=_("Sign In"), content_width=420, content_height=480)
        self._backend = backend
        self._parent = parent
        self._on_done = on_done
        self._url: str | None = None
        self._handle: LoginHandle | None = None
        self._finished = False

        self._open_btn = Gtk.Button(label=_("Open Browser"), sensitive=False)
        self._open_btn.add_css_class("pill")
        self._open_btn.add_css_class("suggested-action")
        self._open_btn.connect("clicked", lambda *_: self._launch())
        self._copy_btn = Gtk.Button(label=_("Copy Link"), sensitive=False)
        self._copy_btn.add_css_class("pill")
        self._copy_btn.connect("clicked", self._on_copy)

        buttons = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12,
                          halign=Gtk.Align.CENTER)
        buttons.append(spinner())
        buttons.append(self._open_btn)
        buttons.append(self._copy_btn)

        self._status = Adw.StatusPage(
            icon_name="web-browser-symbolic",
            title=_("Continue in Your Browser"),
            description=_("Sign in with your Proton account in the browser. "
                          "SSO, two-factor authentication and security keys are supported. "
                          "This dialog closes by itself when you are done."),
            child=buttons,
        )
        self._status.add_css_class("compact")
        view = Adw.ToolbarView()
        view.add_top_bar(Adw.HeaderBar())
        view.set_content(self._status)
        self.set_child(view)
        self.connect("closed", self._on_closed)

    def start(self) -> None:
        try:
            self._handle = self._backend.login(self._on_url, self._on_result)
        except BackendError as exc:
            self._on_result(None, exc)
            return
        self.present(self._parent)

    def _on_url(self, url: str) -> None:
        self._url = url
        self._open_btn.set_sensitive(True)
        self._copy_btn.set_sensitive(True)
        self._launch()

    def _launch(self) -> None:
        if not self._url:
            return
        launcher = Gtk.UriLauncher.new(self._url)
        root = self._parent.get_root()
        launcher.launch(root if isinstance(root, Gtk.Window) else None, None, None)

    def _on_copy(self, *_):
        if self._url:
            self.get_clipboard().set(self._url)

    def _on_result(self, result, error) -> None:
        if self._finished:
            return
        self._finished = True
        self._handle = None
        self.force_close()
        self._on_done(result, error)

    def _on_closed(self, *_):
        if not self._finished and self._handle is not None:
            self._finished = True
            self._handle.cancel()
            self._on_done(None, BackendError(_("Sign-in cancelled")))
