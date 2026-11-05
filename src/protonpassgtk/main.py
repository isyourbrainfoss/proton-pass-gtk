# SPDX-License-Identifier: GPL-3.0-or-later
"""Application entry point."""

from __future__ import annotations

import os
import sys
from gettext import gettext as _

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gdk, Gio, Gtk  # noqa: E402

from . import APP_ID, VERSION  # noqa: E402
from .backend import create_backend  # noqa: E402

CSS = b"""
.item-icon-large {
  padding: 16px;
  border-radius: 999px;
  background: alpha(var(--accent-bg-color, @accent_bg_color), 0.15);
  color: var(--accent-color, @accent_color);
}
"""

SHORTCUTS_UI = """
<interface>
  <object class="GtkShortcutsWindow" id="help_overlay">
    <property name="modal">True</property>
    <child>
      <object class="GtkShortcutsSection">
        <property name="section-name">shortcuts</property>
        <child>
          <object class="GtkShortcutsGroup">
            <property name="title" translatable="yes">General</property>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Search</property>
              <property name="action-name">win.search</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Refresh</property>
              <property name="action-name">win.refresh</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Go Back</property>
              <property name="action-name">win.back</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Keyboard Shortcuts</property>
              <property name="action-name">win.show-help-overlay</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Close Window</property>
              <property name="action-name">window.close</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Quit</property>
              <property name="action-name">app.quit</property></object></child>
          </object>
        </child>
        <child>
          <object class="GtkShortcutsGroup">
            <property name="title" translatable="yes">Selected Item</property>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Copy Username</property>
              <property name="action-name">win.copy-username</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Copy Password</property>
              <property name="action-name">win.copy-password</property></object></child>
            <child><object class="GtkShortcutsShortcut">
              <property name="title" translatable="yes">Copy One-Time Code</property>
              <property name="action-name">win.copy-totp</property></object></child>
          </object>
        </child>
      </object>
    </child>
  </object>
</interface>
"""

ACCELS = {
    "app.quit": ["<Control>q"],
    "window.close": ["<Control>w"],
    "win.search": ["<Control>f"],
    "win.refresh": ["<Control>r", "F5"],
    "win.back": ["<Alt>Left"],
    "win.show-help-overlay": ["<Control>question"],
    "win.copy-username": ["<Control><Shift>u"],
    "win.copy-password": ["<Control><Shift>c"],
    "win.copy-totp": ["<Control><Shift>t"],
}


class Application(Adw.Application):
    def __init__(self) -> None:
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.DEFAULT_FLAGS)
        self.backend = create_backend()
        self.window = None

    def do_startup(self) -> None:
        Adw.Application.do_startup(self)
        Gtk.Window.set_default_icon_name(APP_ID)
        scheme = os.environ.get("PPG_COLOR_SCHEME", "").lower()
        sm = Adw.StyleManager.get_default()
        # Default follows the system (light/dark) as the HIG asks.
        sm.set_color_scheme({
            "dark": Adw.ColorScheme.FORCE_DARK,
            "light": Adw.ColorScheme.FORCE_LIGHT,
        }.get(scheme, Adw.ColorScheme.DEFAULT))

        # Running from a source checkout: find our icons without installing.
        src_icons = os.path.join(os.path.dirname(__file__), "..", "..", "data", "icons")
        if os.path.isdir(src_icons):
            Gtk.IconTheme.get_for_display(Gdk.Display.get_default()).add_search_path(
                os.path.abspath(src_icons))

        provider = Gtk.CssProvider()
        provider.load_from_data(CSS, -1)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)

        for name, cb in (("quit", lambda *_: self.quit()), ("about", lambda *_: self.show_about())):
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", cb)
            self.add_action(act)
        for action, accels in ACCELS.items():
            self.set_accels_for_action(action, accels)

    def do_activate(self) -> None:
        if self.window is None:
            from .window import Window

            self.window = Window(self, self.backend)
            builder = Gtk.Builder.new_from_string(SHORTCUTS_UI, -1)
            self.window.set_help_overlay(builder.get_object("help_overlay"))
            self.window.check_session()
        self.window.present()

    def do_shutdown(self) -> None:
        if self.window is not None:
            self.window.clipboard_mgr.shutdown()
        Adw.Application.do_shutdown(self)

    def show_about(self) -> None:
        kwargs = dict(
            application_name=_("Proton Pass GTK"),
            application_icon=APP_ID,
            developer_name="Isyour Brainfoss",
            version=VERSION,
            website="https://github.com/isyourbrainfoss/proton-pass-gtk",
            issue_url="https://github.com/isyourbrainfoss/proton-pass-gtk/issues",
            license_type=Gtk.License.GPL_3_0,
            comments=_("Unofficial GTK front end for Proton’s official pass-cli. "
                       "Not affiliated with Proton AG.\n\nBackend: {name}").format(
                           name=self.backend.name),
        )
        if hasattr(Adw, "AboutDialog"):
            Adw.AboutDialog(**kwargs).present(self.window)
        else:  # pragma: no cover
            Adw.AboutWindow(transient_for=self.window, **kwargs).present()


def main(argv: list[str] | None = None) -> int:
    app = Application()
    return app.run(sys.argv if argv is None else argv)


if __name__ == "__main__":
    sys.exit(main())
