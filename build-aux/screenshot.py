#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render screenshots of the app with the mock backend (no account).

Run under a display (e.g. ``xvfb-run``)::

    PPG_BACKEND=mock python3 build-aux/screenshot.py --size 360x720 \
        --scheme dark --state detail --out screenshots/phone-detail-dark.png

States: signin, vaults, list, search, detail.
The window content is rendered offscreen with GtkWidgetPaintable, so the
PNG is exactly WIDTHxHEIGHT logical pixels times --scale.
"""

import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "src"))

ap = argparse.ArgumentParser()
ap.add_argument("--size", default="360x720")
ap.add_argument("--scheme", default="light", choices=["light", "dark"])
ap.add_argument("--state", default="list", choices=["signin", "vaults", "list", "search", "detail"])
ap.add_argument("--item", default="Example Mail")
ap.add_argument("--query", default="example")
ap.add_argument("--action", default="", help="window action to activate before the shot, e.g. copy-password")
ap.add_argument("--out", required=True)
args = ap.parse_args()

os.environ["PPG_BACKEND"] = "mock"
os.environ["PPG_COLOR_SCHEME"] = args.scheme
if args.state == "signin":
    os.environ["PPG_MOCK_SIGNED_OUT"] = "1"

import gi  # noqa: E402

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Graphene", "1.0")
from gi.repository import GLib, Graphene, Gtk  # noqa: E402

from protonpassgtk.main import Application  # noqa: E402

W, H = (int(v) for v in args.size.split("x"))


class ShotApp(Application):
    def do_activate(self):
        Application.do_activate(self)
        settings = Gtk.Settings.get_default()
        settings.set_property("gtk-font-name", "Cantarell 11")
        settings.set_property("gtk-enable-animations", False)
        self.window.set_default_size(W, H)
        GLib.timeout_add(900, self.step_navigate)

    def step_navigate(self):
        win = self.window
        content = win.get_content()
        dw, dh = W - content.get_width(), H - content.get_height()
        if (dw or dh) and not getattr(self, "_resized", False):
            # Non-composited X (Xvfb) adds a solid CSD frame; compensate so
            # the rendered content is exactly WxH.
            self._resized = True
            win.set_default_size(W + dw, H + dh)
            GLib.timeout_add(500, self.step_navigate)
            return False
        if args.state in ("signin",):
            pass
        elif args.state == "vaults":
            win._outer.set_show_content(False)
        else:
            win._outer.set_show_content(True)
            if args.state == "search":
                win.focus_search()
                win._search_entry.set_text(args.query)
            if args.state == "detail" or (args.state == "list" and not win._inner.get_collapsed()):
                for i in range(win._filtered.get_n_items()):
                    if win._filtered.get_item(i).summary.title == args.item:
                        win._selection.set_selected(i)
                        break
                if args.state == "list":
                    win._inner.set_show_content(False)
        if args.action:
            GLib.timeout_add(900, lambda: win.activate_action(f"win.{args.action}", None) and False)
        GLib.timeout_add(1500 if args.action else 1200, self.step_shoot)
        return False

    def step_shoot(self):
        win = self.window
        widget = win.get_content()
        w, h = widget.get_width(), widget.get_height()
        if (w, h) != (W, H):
            tries = getattr(self, "_tries", 0)
            self._tries = tries + 1
            if tries < 10:
                win.set_default_size(win.get_width() + W - w, win.get_height() + H - h)
                GLib.timeout_add(400, self.step_shoot)
                return False
        # Render the whole window (so its background is included) and crop
        # to the content area, skipping the Xvfb CSD frame.
        ok, origin = widget.compute_point(win, Graphene.Point().init(0, 0))
        ww, wh = win.get_width(), win.get_height()
        paintable = Gtk.WidgetPaintable.new(win)
        snap = Gtk.Snapshot()
        paintable.snapshot(snap, ww, wh)
        node = snap.to_node()
        rect = Graphene.Rect().init(origin.x, origin.y, w, h)
        texture = win.get_renderer().render_texture(node, rect)
        os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
        texture.save_to_png(args.out)
        print(f"{args.out}: {w}x{h} (collapsed outer={win._outer.get_collapsed()} "
              f"inner={win._inner.get_collapsed()})")
        self.quit()
        return False


sys.exit(ShotApp().run([sys.argv[0]]))
