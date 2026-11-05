# SPDX-License-Identifier: GPL-3.0-or-later
"""Small widget helpers."""

from __future__ import annotations

from gi.repository import Adw, Gtk


def spinner() -> Gtk.Widget:
    if hasattr(Adw, "Spinner"):  # libadwaita >= 1.6
        sp = Adw.Spinner()
    else:  # pragma: no cover - older Phosh distros
        sp = Gtk.Spinner(spinning=True)
    sp.set_size_request(32, 32)
    sp.set_halign(Gtk.Align.CENTER)
    sp.set_valign(Gtk.Align.CENTER)
    return sp


def icon_button(icon: str, tooltip: str, *, action: str | None = None, flat: bool = True) -> Gtk.Button:
    """Icon-only button with tooltip *and* accessible label (HIG + a11y)."""
    btn = Gtk.Button(icon_name=icon, tooltip_text=tooltip, valign=Gtk.Align.CENTER)
    btn.update_property([Gtk.AccessibleProperty.LABEL], [tooltip])
    if flat:
        btn.add_css_class("flat")
    if action:
        btn.set_action_name(action)
    return btn


def toggle_icon_button(icon: str, tooltip: str) -> Gtk.ToggleButton:
    btn = Gtk.ToggleButton(icon_name=icon, tooltip_text=tooltip, valign=Gtk.Align.CENTER)
    btn.update_property([Gtk.AccessibleProperty.LABEL], [tooltip])
    btn.add_css_class("flat")
    return btn


def toolbar_page(title: str, content: Gtk.Widget, header: Adw.HeaderBar | None = None,
                 tag: str | None = None) -> tuple[Adw.NavigationPage, Adw.ToolbarView, Adw.HeaderBar]:
    header = header or Adw.HeaderBar()
    view = Adw.ToolbarView()
    view.add_top_bar(header)
    view.set_content(content)
    page = Adw.NavigationPage(title=title, child=view)
    if tag:
        page.set_tag(tag)
    return page, view, header


def status_page(icon: str, title: str, description: str = "", child: Gtk.Widget | None = None,
                compact: bool = False) -> Adw.StatusPage:
    sp = Adw.StatusPage(icon_name=icon, title=title, description=description or None)
    if child is not None:
        sp.set_child(child)
    if compact:
        sp.add_css_class("compact")
    return sp


def loading_page(title: str = "Loading…") -> Adw.StatusPage:
    sp = Adw.StatusPage(title=title)
    if hasattr(Adw, "SpinnerPaintable"):
        sp.set_paintable(Adw.SpinnerPaintable.new(sp))
    else:  # pragma: no cover
        sp.set_child(spinner())
    return sp


def format_time(iso: str) -> str:
    """'2026-10-02T09:30:00' -> '2026-10-02 09:30' (pass-cli civil time)."""
    if not iso:
        return ""
    return iso.replace("T", " ")[:16]
