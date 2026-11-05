# SPDX-License-Identifier: GPL-3.0-or-later
"""Item detail view: an AdwPreferencesPage built from an ItemDetail.

Secret values are fetched from the backend only when the user copies or
reveals them. Revealed values are hidden again after a timeout and when the
page is destroyed. One-time codes are refreshed every 30 s period while the
page is shown.
"""

from __future__ import annotations

import time
from gettext import gettext as _
from typing import TYPE_CHECKING

from gi.repository import Adw, GLib, Gtk, Pango

from .models import ITEM_TYPE_ICONS, Field, FieldKind, ItemDetail, ItemType
from .widgets import format_time, icon_button, toggle_icon_button

if TYPE_CHECKING:  # pragma: no cover
    from .window import Window

MASK = "●" * 12
REVEAL_SECONDS = 30
TOTP_PERIOD = 30

TYPE_LABELS = {
    ItemType.LOGIN: _("Login"),
    ItemType.NOTE: _("Note"),
    ItemType.ALIAS: _("Alias"),
    ItemType.CREDIT_CARD: _("Credit Card"),
    ItemType.IDENTITY: _("Identity"),
    ItemType.SSH_KEY: _("SSH Key"),
    ItemType.WIFI: _("Wi-Fi Network"),
    ItemType.CUSTOM: _("Custom Item"),
    ItemType.UNKNOWN: _("Item"),
}


def _group_code(code: str) -> str:
    return f"{code[:3]} {code[3:]}" if len(code) == 6 else code


class ItemView(Adw.Bin):
    def __init__(self, window: "Window", detail: ItemDetail, vault_name: str = "") -> None:
        super().__init__()
        self._win = window
        self.detail = detail
        self._reveal_timeouts: dict[str, int] = {}
        self._totp_rows: list[tuple[Field, Adw.ActionRow, Gtk.Label]] = []
        self._totp_tick = 0
        self._totp_counter = -1

        page = Adw.PreferencesPage()
        page.add(self._header_group(vault_name))

        main = Adw.PreferencesGroup()
        if detail.alias_email:
            main.add(self._text_row(Field("alias", _("Alias Address"), FieldKind.EMAIL, detail.alias_email)))
        for f in detail.fields_in(""):
            main.add(self._row_for(f))
        if detail.fields_in("") or detail.alias_email:
            page.add(main)

        if detail.urls:
            urls = Adw.PreferencesGroup(title=_("Websites"))
            for url in detail.urls:
                urls.add(self._url_row(url))
            page.add(urls)

        for section in detail.sections:
            if not section:
                continue
            grp = Adw.PreferencesGroup(title=section)
            for f in detail.fields_in(section):
                grp.add(self._row_for(f))
            page.add(grp)

        if detail.note:
            page.add(self._note_group(detail.note))

        meta = Adw.PreferencesGroup()
        if detail.modify_time:
            meta.add(self._meta_row(_("Last Modified"), format_time(detail.modify_time)))
        if detail.create_time:
            meta.add(self._meta_row(_("Created"), format_time(detail.create_time)))
        if detail.attachment_count:
            meta.add(self._meta_row(_("Attachments"), str(detail.attachment_count)))
        page.add(meta)

        self.set_child(page)

        if self._totp_rows:
            self._refresh_totp()
            self._totp_tick = GLib.timeout_add_seconds(1, self._on_totp_tick)

    # ------------------------------------------------------------ building

    def _header_group(self, vault_name: str) -> Adw.PreferencesGroup:
        d = self.detail
        icon = Gtk.Image(icon_name=ITEM_TYPE_ICONS[d.item_type], pixel_size=32)
        icon.add_css_class("item-icon-large")
        icon.set_halign(Gtk.Align.CENTER)
        icon.update_property([Gtk.AccessibleProperty.LABEL], [TYPE_LABELS[d.item_type]])
        title = Gtk.Label(label=d.title, wrap=True, justify=Gtk.Justification.CENTER)
        title.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        title.add_css_class("title-2")
        sub_text = TYPE_LABELS[d.item_type] + (f" · {vault_name}" if vault_name else "")
        sub = Gtk.Label(label=sub_text, wrap=True, justify=Gtk.Justification.CENTER)
        sub.add_css_class("dim-label")
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, margin_bottom=6)
        box.append(icon)
        box.append(title)
        box.append(sub)
        grp = Adw.PreferencesGroup()
        grp.add(box)
        return grp

    def _base_row(self, f: Field, subtitle: str) -> Adw.ActionRow:
        row = Adw.ActionRow(title=f.label, subtitle=subtitle)
        row.add_css_class("property")
        row.set_subtitle_lines(3)
        row.set_use_markup(False)
        return row

    def _copy_button(self, f: Field) -> Gtk.Button:
        btn = icon_button("edit-copy-symbolic", _("Copy {label}").format(label=f.label))
        btn.connect("clicked", lambda *_: self._win.copy_field(self.detail, f))
        return btn

    def _row_for(self, f: Field) -> Adw.ActionRow:
        if f.kind is FieldKind.HIDDEN:
            return self._hidden_row(f)
        if f.kind is FieldKind.TOTP:
            return self._totp_row(f)
        if f.kind is FieldKind.URL:
            return self._url_row(f.value or "", f.label)
        if f.kind is FieldKind.TIMESTAMP and f.value and f.value.isdigit():
            ts = GLib.DateTime.new_from_unix_local(int(f.value)).format("%Y-%m-%d %H:%M")
            return self._text_row(Field(f.key, f.label, FieldKind.TEXT, ts, f.section))
        return self._text_row(f)

    def _text_row(self, f: Field) -> Adw.ActionRow:
        row = self._base_row(f, f.value or "")
        copy = self._copy_button(f)
        row.add_suffix(copy)
        row.set_activatable_widget(copy)
        return row

    def _hidden_row(self, f: Field) -> Adw.ActionRow:
        row = self._base_row(f, MASK)
        row.update_property([Gtk.AccessibleProperty.DESCRIPTION], [_("Hidden")])
        reveal = toggle_icon_button("view-reveal-symbolic", _("Show {label}").format(label=f.label))
        reveal.connect("toggled", self._on_reveal_toggled, f, row)
        copy = self._copy_button(f)
        row.add_suffix(reveal)
        row.add_suffix(copy)
        row.set_activatable_widget(copy)
        return row

    def _totp_row(self, f: Field) -> Adw.ActionRow:
        row = self._base_row(f, "––– –––")
        remaining = Gtk.Label(valign=Gtk.Align.CENTER)
        remaining.add_css_class("dim-label")
        remaining.add_css_class("numeric")
        remaining.add_css_class("caption")
        copy = self._copy_button(f)
        row.add_suffix(remaining)
        row.add_suffix(copy)
        row.set_activatable_widget(copy)
        self._totp_rows.append((f, row, remaining))
        return row

    def _url_row(self, url: str, label: str | None = None) -> Adw.ActionRow:
        f = Field("url", label or _("Website"), FieldKind.URL, url)
        row = self._base_row(f, url)
        open_btn = icon_button("web-browser-symbolic", _("Open {url}").format(url=url))
        open_btn.connect("clicked", lambda *_: self._win.open_url(url))
        row.add_suffix(open_btn)
        row.add_suffix(self._copy_text_button(url, _("Website")))
        row.set_activatable_widget(open_btn)
        return row

    def _copy_text_button(self, text: str, label: str) -> Gtk.Button:
        btn = icon_button("edit-copy-symbolic", _("Copy {label}").format(label=label))
        btn.connect("clicked", lambda *_: self._win.copy_plain(text, label))
        return btn

    def _note_group(self, note: str) -> Adw.PreferencesGroup:
        grp = Adw.PreferencesGroup(title=_("Note"))
        label = Gtk.Label(label=note, wrap=True, selectable=True, xalign=0,
                          margin_top=12, margin_bottom=12, margin_start=12, margin_end=12)
        label.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        frame = Gtk.Frame(child=label)
        frame.add_css_class("card")
        frame.add_css_class("view")
        grp.add(frame)
        copy = self._copy_text_button(note, _("Note"))
        grp.set_header_suffix(copy)
        return grp

    def _meta_row(self, title: str, value: str) -> Adw.ActionRow:
        row = Adw.ActionRow(title=title)
        lbl = Gtk.Label(label=value, valign=Gtk.Align.CENTER)
        lbl.add_css_class("dim-label")
        lbl.add_css_class("numeric")
        row.add_suffix(lbl)
        return row

    # ------------------------------------------------------------- reveal

    def _on_reveal_toggled(self, btn: Gtk.ToggleButton, f: Field, row: Adw.ActionRow) -> None:
        self._cancel_reveal(f.key)
        if not btn.get_active():
            row.set_subtitle(MASK)
            btn.set_icon_name("view-reveal-symbolic")
            btn.set_tooltip_text(_("Show {label}").format(label=f.label))
            return

        def got(value, error):
            if error is not None or not btn.get_active():
                btn.set_active(False)
                if error is not None:
                    self._win.show_error(error)
                return
            row.set_subtitle(value)
            del value
            btn.set_icon_name("view-conceal-symbolic")
            btn.set_tooltip_text(_("Hide {label}").format(label=f.label))
            self._reveal_timeouts[f.key] = GLib.timeout_add_seconds(
                REVEAL_SECONDS, lambda: (btn.set_active(False), GLib.SOURCE_REMOVE)[1]
            )

        self._win.backend.get_secret(self.detail.share_id, self.detail.id, f.key, got)

    def _cancel_reveal(self, key: str) -> None:
        src = self._reveal_timeouts.pop(key, 0)
        if src:
            GLib.source_remove(src)

    # --------------------------------------------------------------- TOTP

    def _on_totp_tick(self) -> bool:
        if not self.get_mapped():
            # Not on screen (e.g. phone showing the list): stop asking
            # pass-cli for codes, refresh as soon as we are visible again.
            self._totp_counter = -1
            return GLib.SOURCE_CONTINUE
        now = time.time()
        left = TOTP_PERIOD - int(now) % TOTP_PERIOD
        for _f, _row, label in self._totp_rows:
            label.set_label(_("{n} s").format(n=left))
        counter = int(now // TOTP_PERIOD)
        if counter != self._totp_counter:
            self._refresh_totp()
        return GLib.SOURCE_CONTINUE

    def _refresh_totp(self) -> None:
        self._totp_counter = int(time.time() // TOTP_PERIOD)
        left = TOTP_PERIOD - int(time.time()) % TOTP_PERIOD
        for f, row, label in self._totp_rows:
            label.set_label(_("{n} s").format(n=left))

            def got(code, error, row=row):
                if error is None and code:
                    row.set_subtitle(_group_code(code))

            self._win.backend.get_totp(self.detail.share_id, self.detail.id, f.key, got)

    # ------------------------------------------------------------ cleanup

    def dispose_timers(self) -> None:
        for key in list(self._reveal_timeouts):
            self._cancel_reveal(key)
        if self._totp_tick:
            GLib.source_remove(self._totp_tick)
            self._totp_tick = 0
