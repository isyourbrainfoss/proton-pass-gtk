# SPDX-License-Identifier: GPL-3.0-or-later
"""Main window.

Layout (GNOME HIG "triple pane" built from two nested split views)::

    AdwNavigationSplitView (outer)
    ├── sidebar: Vaults (All Items + one row per vault)
    └── content: AdwNavigationSplitView (inner)
        ├── sidebar: item list + search
        └── content: item details

Breakpoints collapse the outer view below 860sp and both views below
600sp, so on a 360 px phone you navigate Vaults → Items → Details.
"""

from __future__ import annotations

from gettext import gettext as _

from gi.repository import Adw, Gio, GLib, GObject, Gtk, Pango

from . import APP_ID
from .backend import Backend, BackendError, NotLoggedIn, PassCliMissing
from .clipboard import SecretClipboard
from .item_view import ItemView, TYPE_LABELS
from .login import LoginDialog
from .models import ITEM_TYPE_ICONS, Field, FieldKind, ItemDetail, ItemSummary, Vault
from .widgets import icon_button, loading_page, status_page, toolbar_page

ALL_ITEMS = Vault(_("All Items"), "__all__")


class ItemObject(GObject.Object):
    __gtype_name__ = "PpgItemObject"

    def __init__(self, summary: ItemSummary, vault_name: str) -> None:
        super().__init__()
        self.summary = summary
        self.vault_name = vault_name
        self.search_text = f"{summary.title} {vault_name} {TYPE_LABELS[summary.item_type]}".casefold()


class Window(Adw.ApplicationWindow):
    def __init__(self, app: Adw.Application, backend: Backend) -> None:
        super().__init__(application=app, title=_("Proton Pass"))
        self.backend = backend
        self.clipboard_mgr = SecretClipboard(self.get_display())
        self.set_default_size(1100, 720)
        self.set_size_request(360, 294)  # HIG minimum for mobile
        self.set_icon_name(APP_ID)

        self._vaults: list[Vault] = []
        self._vault_rows: dict[str, Gtk.ListBoxRow] = {}
        self._items_cache: dict[str, list[ItemSummary]] = {}
        self._current_vault: Vault = ALL_ITEMS
        self._current_item: ItemSummary | None = None
        self._current_detail: ItemDetail | None = None
        self._item_view: ItemView | None = None
        self._load_generation = 0

        self._toasts = Adw.ToastOverlay()
        self._stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)
        self._toasts.set_child(self._stack)
        self.set_content(self._toasts)

        self._stack.add_named(self._simple_page(loading_page(_("Connecting to pass-cli…"))), "loading")
        self._stack.add_named(self._build_signin(), "signin")
        self._missing_page = status_page(
            "dialog-warning-symbolic", _("pass-cli Not Found"),
            _("This app is a front end for Proton’s official command-line tool. "
              "Install pass-cli from protonpass.github.io/pass-cli and try again."),
        )
        retry = Gtk.Button(label=_("Try Again"), halign=Gtk.Align.CENTER)
        retry.add_css_class("pill")
        retry.connect("clicked", lambda *_: self.check_session())
        self._missing_page.set_child(retry)
        self._stack.add_named(self._simple_page(self._missing_page), "missing")
        self._stack.add_named(self._build_main(), "main")

        self._setup_actions()
        self._setup_breakpoints()
        self.connect("close-request", self._on_close_request)

    # ================================================================ build

    def _primary_menu(self) -> Gtk.MenuButton:
        menu = Gio.Menu()
        sec1 = Gio.Menu()
        sec1.append(_("_Refresh"), "win.refresh")
        menu.append_section(None, sec1)
        sec2 = Gio.Menu()
        sec2.append(_("_Keyboard Shortcuts"), "win.show-help-overlay")
        sec2.append(_("_About Proton Pass GTK"), "app.about")
        menu.append_section(None, sec2)
        sec3 = Gio.Menu()
        sec3.append(_("_Sign Out"), "win.sign-out")
        menu.append_section(None, sec3)
        btn = Gtk.MenuButton(icon_name="open-menu-symbolic", menu_model=menu,
                             tooltip_text=_("Main Menu"), primary=True)
        btn.update_property([Gtk.AccessibleProperty.LABEL], [_("Main Menu")])
        return btn

    def _simple_page(self, content: Gtk.Widget) -> Gtk.Widget:
        view = Adw.ToolbarView()
        header = Adw.HeaderBar()
        header.set_title_widget(Adw.WindowTitle(title=""))
        view.add_top_bar(header)
        view.set_content(content)
        return view

    def _build_signin(self) -> Gtk.Widget:
        btn = Gtk.Button(label=_("Sign In"), halign=Gtk.Align.CENTER)
        btn.add_css_class("pill")
        btn.add_css_class("suggested-action")
        btn.set_action_name("win.sign-in")
        page = status_page(
            f"{APP_ID}-symbolic",
            _("Proton Pass"),
            _("Sign in with your Proton account to see your vaults. "
              "Sign-in runs through Proton’s official pass-cli in your browser."),
            child=btn,
        )
        self._signin_page = page
        return self._simple_page(page)

    def _build_main(self) -> Gtk.Widget:
        # ---- vaults (outer sidebar)
        self._vault_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        self._vault_list.add_css_class("navigation-sidebar")
        self._vault_list.connect("row-activated", self._on_vault_activated)
        self._vault_list.update_property([Gtk.AccessibleProperty.LABEL], [_("Vaults")])
        vault_scroll = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER,
                                          child=self._vault_list)
        vault_header = Adw.HeaderBar()
        vault_header.set_title_widget(Adw.WindowTitle(title=_("Vaults")))
        vault_header.pack_end(self._primary_menu())
        self._vault_page, _v, _h = toolbar_page(_("Vaults"), vault_scroll, vault_header, "vaults")

        # ---- items (inner sidebar)
        self._store = Gio.ListStore(item_type=ItemObject)
        self._filter = Gtk.CustomFilter.new(self._filter_func)
        self._filtered = Gtk.FilterListModel(model=self._store, filter=self._filter)
        self._filtered.connect("items-changed", lambda *_: self._update_list_state())
        self._selection = Gtk.SingleSelection(model=self._filtered, autoselect=False,
                                              can_unselect=True)
        self._selection.set_selected(Gtk.INVALID_LIST_POSITION)
        self._selection.connect("notify::selected", self._on_item_selected)
        factory = Gtk.SignalListItemFactory()
        factory.connect("setup", self._item_setup)
        factory.connect("bind", self._item_bind)
        self._list_view = Gtk.ListView(model=self._selection, factory=factory)
        self._list_view.add_css_class("navigation-sidebar")
        self._list_view.connect("activate", self._on_item_activate)
        self._list_view.update_property([Gtk.AccessibleProperty.LABEL], [_("Items")])
        list_scroll = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER,
                                         child=self._list_view, vexpand=True)

        self._list_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)
        self._list_stack.add_named(list_scroll, "list")
        self._list_stack.add_named(loading_page(""), "loading")
        self._list_stack.add_named(status_page("system-search-symbolic", _("No Results Found"),
                                               _("Try a different search"), compact=True), "no-results")
        self._list_stack.add_named(status_page("folder-symbolic", _("No Items"),
                                               _("This vault is empty"), compact=True), "empty")

        self._search_entry = Gtk.SearchEntry(placeholder_text=_("Search items"), hexpand=True)
        self._search_entry.update_property([Gtk.AccessibleProperty.LABEL], [_("Search items")])
        self._search_entry.connect("search-changed", lambda *_: self._filter.changed(Gtk.FilterChange.DIFFERENT))
        self._search_entry.connect("activate", self._on_search_activate)
        self._search_entry.connect("stop-search", lambda *_: self._search_bar.set_search_mode(False))
        clamp = Adw.Clamp(maximum_size=400, child=self._search_entry)
        self._search_bar = Gtk.SearchBar(child=clamp)
        self._search_bar.connect_entry(self._search_entry)
        self._search_bar.set_key_capture_widget(self)

        self._search_btn = Gtk.ToggleButton(icon_name="system-search-symbolic",
                                            tooltip_text=_("Search"))
        self._search_btn.update_property([Gtk.AccessibleProperty.LABEL], [_("Search")])
        self._search_btn.bind_property("active", self._search_bar, "search-mode-enabled",
                                       GObject.BindingFlags.BIDIRECTIONAL | GObject.BindingFlags.SYNC_CREATE)
        self._items_title = Adw.WindowTitle(title=ALL_ITEMS.name)
        items_header = Adw.HeaderBar()
        items_header.set_title_widget(self._items_title)
        items_header.pack_end(self._search_btn)
        items_view = Adw.ToolbarView()
        items_view.add_top_bar(items_header)
        items_view.add_top_bar(self._search_bar)
        items_view.set_content(self._list_stack)
        self._items_page = Adw.NavigationPage(title=ALL_ITEMS.name, child=items_view, tag="items")

        # ---- details (inner content)
        self._detail_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE)
        self._detail_stack.add_named(status_page(f"{APP_ID}-symbolic", _("No Item Selected"),
                                                 _("Select an item to see its details")), "none")
        self._detail_stack.add_named(loading_page(""), "loading")
        self._detail_title = Adw.WindowTitle(title="")
        detail_header = Adw.HeaderBar()
        detail_header.set_title_widget(self._detail_title)
        self._detail_page, _dv, _dh = toolbar_page(_("Details"), self._detail_stack, detail_header, "details")

        self._inner = Adw.NavigationSplitView(min_sidebar_width=280, max_sidebar_width=400,
                                              sidebar_width_fraction=0.4)
        self._inner.set_sidebar(self._items_page)
        self._inner.set_content(self._detail_page)
        self._inner.connect("notify::show-content", self._on_inner_show_content)
        # Only offer "back" on the details page when it actually leads back to
        # the item list (inner view collapsed); otherwise the item list header
        # already has the back button to the vaults.
        self._inner.bind_property("collapsed", detail_header, "show-back-button",
                                  GObject.BindingFlags.SYNC_CREATE)
        self._inner_page = Adw.NavigationPage(title=ALL_ITEMS.name, child=self._inner, tag="vault")

        self._outer = Adw.NavigationSplitView(min_sidebar_width=200, max_sidebar_width=260,
                                              sidebar_width_fraction=0.22)
        self._outer.set_sidebar(self._vault_page)
        self._outer.set_content(self._inner_page)
        return self._outer

    def _setup_breakpoints(self) -> None:
        mid = Adw.Breakpoint.new(Adw.BreakpointCondition.parse("max-width: 860sp"))
        mid.add_setter(self._outer, "collapsed", True)
        self.add_breakpoint(mid)
        narrow = Adw.Breakpoint.new(Adw.BreakpointCondition.parse("max-width: 600sp"))
        narrow.add_setter(self._outer, "collapsed", True)
        narrow.add_setter(self._inner, "collapsed", True)
        self.add_breakpoint(narrow)

    # ---------------------------------------------------------- list rows

    def _item_setup(self, _factory, list_item: Gtk.ListItem) -> None:
        icon = Gtk.Image(pixel_size=16, valign=Gtk.Align.CENTER)
        icon.add_css_class("item-icon")
        title = Gtk.Label(xalign=0, ellipsize=Pango.EllipsizeMode.END)
        sub = Gtk.Label(xalign=0, ellipsize=Pango.EllipsizeMode.END)
        sub.add_css_class("caption")
        sub.add_css_class("dim-label")
        texts = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, valign=Gtk.Align.CENTER, hexpand=True)
        texts.append(title)
        texts.append(sub)
        box = Gtk.Box(spacing=12, margin_top=6, margin_bottom=6)
        box.append(icon)
        box.append(texts)
        list_item.set_child(box)
        list_item._widgets = (icon, title, sub)

    def _item_bind(self, _factory, list_item: Gtk.ListItem) -> None:
        obj: ItemObject = list_item.get_item()
        icon, title, sub = list_item._widgets
        s = obj.summary
        icon.set_from_icon_name(ITEM_TYPE_ICONS[s.item_type])
        title.set_label(s.title)
        parts = [TYPE_LABELS[s.item_type]]
        if self._current_vault is ALL_ITEMS:
            parts.append(obj.vault_name)
        sub.set_label(" · ".join(parts))
        if hasattr(list_item, "set_accessible_label"):  # GTK >= 4.12
            list_item.set_accessible_label(f"{s.title}, {' · '.join(parts)}")

    def _filter_func(self, obj: ItemObject) -> bool:
        query = self._search_entry.get_text().strip().casefold()
        if not query:
            return True
        return all(tok in obj.search_text for tok in query.split())

    # ============================================================ actions

    def _setup_actions(self) -> None:
        def add(name, cb, enabled=True):
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", cb)
            act.set_enabled(enabled)
            self.add_action(act)
            return act

        add("search", lambda *_: self.focus_search())
        add("refresh", lambda *_: self.refresh())
        add("sign-in", lambda *_: self.sign_in())
        add("sign-out", lambda *_: self.confirm_sign_out())
        add("copy-username", lambda *_: self._copy_kind((FieldKind.TEXT, FieldKind.EMAIL), "username"))
        add("copy-password", lambda *_: self._copy_kind((FieldKind.HIDDEN,), "password"))
        add("copy-totp", lambda *_: self._copy_kind((FieldKind.TOTP,), "totp"))
        add("back", lambda *_: self.go_back())

    def focus_search(self) -> None:
        if self._stack.get_visible_child_name() != "main":
            return
        self._outer.set_show_content(True)
        self._inner.set_show_content(False)
        self._search_bar.set_search_mode(True)
        self._search_entry.grab_focus()

    def go_back(self) -> None:
        if self._inner.get_collapsed() and self._inner.get_show_content():
            self._inner.set_show_content(False)
        elif self._outer.get_collapsed() and self._outer.get_show_content():
            self._outer.set_show_content(False)

    # ============================================================= session

    def check_session(self) -> None:
        self._stack.set_visible_child_name("loading")
        try:
            self.backend.session_info(self._on_session)
        except PassCliMissing:
            self._stack.set_visible_child_name("missing")

    def _on_session(self, info, error) -> None:
        if isinstance(error, PassCliMissing):
            self._stack.set_visible_child_name("missing")
            return
        if error is not None:
            self.show_error(error)
            self._stack.set_visible_child_name("signin")
            return
        if not info.logged_in:
            self._stack.set_visible_child_name("signin")
            return
        self._stack.set_visible_child_name("main")
        self.load_vaults()

    def sign_in(self) -> None:
        try:
            dialog = LoginDialog(self.backend, self, self._on_login_done)
            dialog.start()
        except PassCliMissing:
            self._stack.set_visible_child_name("missing")

    def _on_login_done(self, info, error) -> None:
        if error is not None:
            self.show_error(error)
            return
        self.toast(_("Signed in as {account}").format(account=info.account) if info.account
                   else _("Signed in"))
        self._on_session(info, None)

    def confirm_sign_out(self) -> None:
        dialog = Adw.AlertDialog(heading=_("Sign Out?"),
                                 body=_("pass-cli will forget this session. "
                                        "You will need to sign in again in your browser."))
        dialog.add_response("cancel", _("_Cancel"))
        dialog.add_response("sign-out", _("_Sign Out"))
        dialog.set_response_appearance("sign-out", Adw.ResponseAppearance.DESTRUCTIVE)
        dialog.set_default_response("cancel")
        dialog.set_close_response("cancel")

        def on_response(_d, response):
            if response == "sign-out":
                self.clipboard_mgr.shutdown()
                self.backend.logout(lambda _r, err: self._after_logout(err))

        dialog.connect("response", on_response)
        dialog.present(self)

    def _after_logout(self, error) -> None:
        if error is not None:
            self.show_error(error)
        self._reset_data()
        self._stack.set_visible_child_name("signin")

    def _reset_data(self) -> None:
        self._vaults = []
        self._items_cache.clear()
        self._store.remove_all()
        self._clear_detail()
        child = self._vault_list.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self._vault_list.remove(child)
            child = nxt
        self._vault_rows.clear()

    # ============================================================== vaults

    def refresh(self) -> None:
        if self._stack.get_visible_child_name() != "main":
            return
        self._items_cache.clear()
        self.load_vaults()

    def load_vaults(self) -> None:
        self.backend.list_vaults(self._on_vaults)

    def _on_vaults(self, vaults, error) -> None:
        if self._handle_auth_error(error):
            return
        self._reset_data()
        self._vaults = list(vaults)
        self._add_vault_row(ALL_ITEMS, "view-list-bullet-symbolic")
        for v in self._vaults:
            self._add_vault_row(v, "folder-symbolic")
        target = next((v for v in [ALL_ITEMS, *self._vaults]
                       if v.share_id == self._current_vault.share_id), ALL_ITEMS)
        self.select_vault(target)

    def _add_vault_row(self, vault: Vault, icon: str) -> None:
        img = Gtk.Image(icon_name=icon)
        label = Gtk.Label(label=vault.name, xalign=0, hexpand=True, ellipsize=Pango.EllipsizeMode.END)
        count = Gtk.Label(xalign=1)
        count.add_css_class("dim-label")
        count.add_css_class("numeric")
        box = Gtk.Box(spacing=12, margin_top=6, margin_bottom=6)
        box.append(img)
        box.append(label)
        box.append(count)
        row = Gtk.ListBoxRow(child=box)
        row.vault = vault
        row.count_label = count
        self._vault_list.append(row)
        self._vault_rows[vault.share_id] = row

    def _on_vault_activated(self, _lb, row) -> None:
        self.select_vault(row.vault)
        self._outer.set_show_content(True)

    def select_vault(self, vault: Vault) -> None:
        self._current_vault = vault
        row = self._vault_rows.get(vault.share_id)
        if row is not None:
            self._vault_list.select_row(row)
        self._items_title.set_title(vault.name)
        self._items_page.set_title(vault.name)
        self._inner_page.set_title(vault.name)
        self._load_items(vault)

    def _vault_name(self, share_id: str) -> str:
        return next((v.name for v in self._vaults if v.share_id == share_id), "")

    # =============================================================== items

    def _load_items(self, vault: Vault) -> None:
        self._load_generation += 1
        gen = self._load_generation
        targets = self._vaults if vault is ALL_ITEMS else [vault]
        missing = [v for v in targets if v.share_id not in self._items_cache]
        if not missing:
            self._show_items(vault)
            return
        self._list_stack.set_visible_child_name("loading")
        pending = {"n": len(missing)}

        def done(v: Vault):
            def cb(items, error):
                if self._handle_auth_error(error):
                    return
                self._items_cache[v.share_id] = list(items or [])
                row = self._vault_rows.get(v.share_id)
                if row is not None:
                    row.count_label.set_label(str(len(self._items_cache[v.share_id])))
                pending["n"] -= 1
                if pending["n"] == 0 and gen == self._load_generation:
                    self._show_items(vault)
            return cb

        for v in missing:
            self.backend.list_items(v, done(v))

    def _show_items(self, vault: Vault) -> None:
        targets = self._vaults if vault is ALL_ITEMS else [vault]
        summaries: list[ItemSummary] = []
        for v in targets:
            summaries += self._items_cache.get(v.share_id, [])
        summaries.sort(key=lambda s: s.title.casefold())
        all_row = self._vault_rows.get(ALL_ITEMS.share_id)
        if all_row is not None and all(v.share_id in self._items_cache for v in self._vaults):
            all_row.count_label.set_label(str(sum(len(self._items_cache[v.share_id]) for v in self._vaults)))
        objs = [ItemObject(s, self._vault_name(s.share_id)) for s in summaries]
        self._store.splice(0, self._store.get_n_items(), objs)
        self._selection.set_selected(Gtk.INVALID_LIST_POSITION)
        self._items_title.set_subtitle(
            GLib.dngettext(None, "{n} item", "{n} items", len(objs)).format(n=len(objs)))
        self._update_list_state()
        # Keep the open item if it is still in this list.
        if self._current_item is not None:
            for i in range(self._filtered.get_n_items()):
                if self._filtered.get_item(i).summary.key == self._current_item.key:
                    self._selection.set_selected(i)
                    break
            else:
                self._clear_detail()

    def _update_list_state(self) -> None:
        if self._list_stack.get_visible_child_name() == "loading" and \
                self._store.get_n_items() == 0 and self._load_pending():
            return
        if self._store.get_n_items() == 0:
            self._list_stack.set_visible_child_name("empty")
        elif self._filtered.get_n_items() == 0:
            self._list_stack.set_visible_child_name("no-results")
        else:
            self._list_stack.set_visible_child_name("list")

    def _load_pending(self) -> bool:
        targets = self._vaults if self._current_vault is ALL_ITEMS else [self._current_vault]
        return any(v.share_id not in self._items_cache for v in targets)

    def _on_search_activate(self, *_):
        if self._filtered.get_n_items() > 0:
            self._selection.set_selected(0)
            self._open_selected(force_navigate=True)

    def _on_item_selected(self, *_):
        self._open_selected(force_navigate=False)

    def _on_item_activate(self, _lv, position: int) -> None:
        self._selection.set_selected(position)
        self._open_selected(force_navigate=True)

    def _on_inner_show_content(self, *_):
        # On phones, deselect when going back so tapping the same row works.
        if self._inner.get_collapsed() and not self._inner.get_show_content():
            self._selection.set_selected(Gtk.INVALID_LIST_POSITION)

    def _open_selected(self, force_navigate: bool) -> None:
        obj = self._selection.get_selected_item()
        if obj is None:
            return
        summary = obj.summary
        self._inner.set_show_content(True)
        if self._current_item is not None and self._current_item.key == summary.key \
                and self._item_view is not None:
            return
        self._current_item = summary
        self._detail_title.set_title(summary.title)
        self._detail_page.set_title(summary.title)
        self._detail_stack.set_visible_child_name("loading")
        self.backend.view_item(summary, lambda d, e: self._on_detail(summary, d, e))

    def _on_detail(self, summary: ItemSummary, detail, error) -> None:
        if self._current_item is None or self._current_item.key != summary.key:
            return  # user moved on
        if self._handle_auth_error(error):
            return
        self._drop_item_view()
        self._current_detail = detail
        view = ItemView(self, detail, self._vault_name(detail.share_id))
        self._item_view = view
        self._detail_stack.add_named(view, "detail")
        self._detail_stack.set_visible_child_name("detail")

    def _drop_item_view(self) -> None:
        if self._item_view is not None:
            self._item_view.dispose_timers()
            self._detail_stack.remove(self._item_view)
            self._item_view = None

    def _clear_detail(self) -> None:
        self._drop_item_view()
        self._current_item = None
        self._current_detail = None
        self._detail_title.set_title("")
        self._detail_page.set_title(_("Details"))
        self._detail_stack.set_visible_child_name("none")
        self._inner.set_show_content(False)

    # ============================================================= copying

    def _copy_kind(self, kinds, preferred_key: str) -> None:
        d = self._current_detail
        if d is None:
            return
        cands = [f for f in d.fields if f.kind in kinds]
        f = next((f for f in cands if f.key == preferred_key), cands[0] if cands else None)
        if f is None:
            self.toast(_("This item has no such field"))
            return
        self.copy_field(d, f)

    def copy_field(self, detail: ItemDetail, f: Field) -> None:
        if f.kind is FieldKind.TOTP:
            self.backend.get_totp(detail.share_id, detail.id, f.key,
                                  lambda v, e: self._finish_copy(v, e, _("One-time code"), True))
        elif f.kind is FieldKind.HIDDEN:
            self.backend.get_secret(detail.share_id, detail.id, f.key,
                                    lambda v, e: self._finish_copy(v, e, f.label, True))
        else:
            self.copy_plain(f.value or "", f.label)

    def copy_plain(self, text: str, label: str) -> None:
        self._finish_copy(text, None, label, False)

    def _finish_copy(self, value, error, label: str, secret: bool) -> None:
        if error is not None:
            self.show_error(error)
            return
        delay = self.clipboard_mgr.copy(value, secret=secret, timeout=None if secret else 0)
        del value
        if delay:
            self.toast(_("{label} copied · clears in {n} s").format(label=label, n=delay))
        else:
            self.toast(_("{label} copied").format(label=label))

    def open_url(self, url: str) -> None:
        if not url.startswith(("http://", "https://")):
            url = "https://" + url
        Gtk.UriLauncher.new(url).launch(self, None, None)

    # =============================================================== misc

    def toast(self, text: str) -> None:
        toast = Adw.Toast(title=text, timeout=3)
        self._toasts.add_toast(toast)

    def show_error(self, error: BackendError) -> None:
        self.toast(str(error) or _("Something went wrong"))

    def _handle_auth_error(self, error) -> bool:
        if error is None:
            return False
        if isinstance(error, NotLoggedIn):
            self._reset_data()
            self._stack.set_visible_child_name("signin")
            self.toast(_("Session expired, please sign in again"))
        elif isinstance(error, PassCliMissing):
            self._stack.set_visible_child_name("missing")
        else:
            self.show_error(error)
            if self._list_stack.get_visible_child_name() == "loading":
                self._list_stack.set_visible_child_name("empty")
            if self._detail_stack.get_visible_child_name() == "loading":
                self._detail_stack.set_visible_child_name("none")
        return True

    def _on_close_request(self, *_):
        self._drop_item_view()
        self.clipboard_mgr.shutdown()
        return False
