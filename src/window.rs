// SPDX-License-Identifier: GPL-3.0-or-later
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use proton_pass_gtk::{
    backend::{self, Backend},
    model::{self, Attachment, Detail, Field, Item, Kind, LoginDraft, Vault},
    prefs::{self, ColorScheme, Prefs},
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
use zeroize::Zeroizing;

const HIDDEN: &str = "••••••••";
/// Revealed values hide again after this many seconds.
const REVEAL_SECONDS: u64 = 30;

pub struct Window {
    win: adw::ApplicationWindow,
    backend: Backend,
    toast: adw::ToastOverlay,
    stack: gtk::Stack,
    outer: adw::NavigationSplitView,
    inner: adw::NavigationSplitView,
    vault_list: gtk::ListBox,
    item_list: gtk::ListBox,
    search: gtk::SearchEntry,
    detail_box: gtk::Box,
    detail_scroll: gtk::ScrolledWindow,
    item_page: adw::NavigationPage,
    detail_page: adw::NavigationPage,
    sign_label: gtk::Label,
    sign_button: gtk::Button,
    cancel_button: gtk::Button,
    extra_switch: adw::SwitchRow,
    new_button: gtk::Button,
    edit_button: gtk::Button,
    trash_button: gtk::Button,
    items: RefCell<Vec<Item>>,
    current: RefCell<Option<Detail>>,
    vault: RefCell<Option<Vault>>,
    epoch: Cell<u64>,
    request: Cell<u64>,
    clipboard: RefCell<Option<gdk::ContentProvider>>,
    login: Rc<RefCell<Option<gio::Subprocess>>>,
    prefs: RefCell<Prefs>,
    prefs_path: PathBuf,
    /// Monotonic time (µs) of the last user input, for auto-lock.
    last_activity: Cell<i64>,
    locked: Cell<bool>,
    /// Dialogs that may show item data; force-closed on lock/sign-out.
    dialogs: RefCell<Vec<adw::Dialog>>,
}
fn boxed() -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, 0)
}
fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(child)
        .vexpand(true)
        .build()
}
fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .subtitle_lines(2)
        .build()
}
fn icon_button(icon: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}
fn toolbar(content: &impl IsA<gtk::Widget>, title: &str) -> (adw::ToolbarView, adw::HeaderBar) {
    let t = adw::ToolbarView::new();
    let h = adw::HeaderBar::new();
    h.set_title_widget(Some(&adw::WindowTitle::new(title, "")));
    t.add_top_bar(&h);
    t.set_content(Some(content));
    (t, h)
}
fn apply_color_scheme(c: ColorScheme) {
    adw::StyleManager::default().set_color_scheme(match c {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
    });
}
fn unix_now() -> i64 {
    glib::real_time() / 1_000_000
}
fn minutes_label(m: u32) -> String {
    match m {
        0 => "Never".into(),
        1 => "1 minute".into(),
        m => format!("{m} minutes"),
    }
}
fn entry_row(title: &str, text: &str) -> adw::EntryRow {
    adw::EntryRow::builder().title(title).text(text).build()
}
impl Window {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let backend = Backend::default();
        let prefs_path = prefs::default_path();
        let prefs = Prefs::load(&prefs_path);
        let (w, h) = std::env::var("PPG_SIZE")
            .ok()
            .and_then(|size| {
                let (w, h) = size.split_once('x')?;
                Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))
            })
            .unwrap_or((prefs.width, prefs.height));
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title("Proton Pass GTK")
            .default_width(w)
            .default_height(h)
            .width_request(320)
            .height_request(400)
            .build();
        if prefs.maximized && std::env::var_os("PPG_SIZE").is_none() {
            win.maximize();
        }
        let vault_list = gtk::ListBox::new();
        vault_list.add_css_class("navigation-sidebar");
        vault_list.update_property(&[gtk::accessible::Property::Label("Vaults")]);
        let (vaults, vault_header) = toolbar(&scroll(&vault_list), "Vaults");
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search items…")
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .build();
        search.update_property(&[gtk::accessible::Property::Label(
            "Search items in this vault",
        )]);
        let item_list = gtk::ListBox::new();
        item_list.set_selection_mode(gtk::SelectionMode::Single);
        item_list.add_css_class("navigation-sidebar");
        item_list.update_property(&[gtk::accessible::Property::Label("Items")]);
        let ib = boxed();
        ib.append(&search);
        ib.append(&scroll(&item_list));
        let (items, items_header) = toolbar(&ib, "Items");
        let new_button = icon_button("list-add-symbolic", "New login (Ctrl+N)");
        new_button.set_action_name(Some("win.new-login"));
        new_button.set_visible(backend.can_edit());
        items_header.pack_start(&new_button);
        let detail_box = boxed();
        detail_box.set_margin_top(20);
        detail_box.set_margin_bottom(24);
        detail_box.set_margin_start(16);
        detail_box.set_margin_end(16);
        detail_box.set_spacing(16);
        let clamp = adw::Clamp::builder()
            .maximum_size(640)
            .tightening_threshold(400)
            .child(&detail_box)
            .build();
        let detail_scroll = scroll(&clamp);
        let (details, detail_header) = toolbar(&detail_scroll, "Details");
        let detail_page = adw::NavigationPage::new(&details, "Details");
        let inner = adw::NavigationSplitView::builder()
            .sidebar(&adw::NavigationPage::new(&items, "Items"))
            .content(&detail_page)
            .min_sidebar_width(280.0)
            .max_sidebar_width(340.0)
            .sidebar_width_fraction(0.42)
            .build();
        let item_page = adw::NavigationPage::new(&inner, "Items");
        let outer = adw::NavigationSplitView::builder()
            .sidebar(&adw::NavigationPage::new(&vaults, "Vaults"))
            .content(&item_page)
            .min_sidebar_width(210.0)
            .max_sidebar_width(260.0)
            .sidebar_width_fraction(0.22)
            .build();
        let mid =
            adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 860sp").unwrap());
        mid.add_setter(&outer, "collapsed", Some(&true.to_value()));
        win.add_breakpoint(mid);
        // Only the last matching breakpoint is active: repeat the outer setter.
        let narrow =
            adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 600sp").unwrap());
        narrow.add_setter(&outer, "collapsed", Some(&true.to_value()));
        narrow.add_setter(&inner, "collapsed", Some(&true.to_value()));
        win.add_breakpoint(narrow);

        // Sign-in page
        let sign_label = gtk::Label::new(Some("Sign in securely using Proton’s website."));
        sign_label.set_wrap(true);
        sign_label.set_justify(gtk::Justification::Center);
        let sign_button = gtk::Button::with_label("Sign in with Proton");
        sign_button.add_css_class("suggested-action");
        sign_button.add_css_class("pill");
        sign_button.set_halign(gtk::Align::Center);
        let cancel_button = gtk::Button::with_label("Cancel sign-in");
        cancel_button.set_halign(gtk::Align::Center);
        cancel_button.set_visible(false);
        let extra_switch = adw::SwitchRow::builder()
            .title("Ask for Pass extra password")
            .subtitle("Only if you set one in Proton Pass")
            .active(prefs.extra_password)
            .build();
        let extra_list = gtk::ListBox::new();
        extra_list.add_css_class("boxed-list");
        extra_list.set_selection_mode(gtk::SelectionMode::None);
        extra_list.append(&extra_switch);
        let sign_child = boxed();
        sign_child.set_spacing(16);
        sign_child.append(&sign_label);
        sign_child.append(&sign_button);
        sign_child.append(&cancel_button);
        sign_child.append(&extra_list);
        let sign_clamp = adw::Clamp::builder()
            .maximum_size(420)
            .child(&sign_child)
            .build();
        let signin = adw::StatusPage::builder()
            .title("Your passwords, everywhere")
            .description("An unofficial Proton Pass client.\nAuthentication is handled by the official pass-cli.")
            .icon_name("dialog-password-symbolic")
            .child(&sign_clamp)
            .build();
        let (signin, _) = toolbar(&signin, "Proton Pass GTK");

        // Lock page
        let unlock_button = gtk::Button::with_label("Unlock");
        unlock_button.add_css_class("suggested-action");
        unlock_button.add_css_class("pill");
        unlock_button.set_halign(gtk::Align::Center);
        unlock_button.set_action_name(Some("win.unlock"));
        let lock_signout = gtk::Button::with_label("Sign out…");
        lock_signout.add_css_class("flat");
        lock_signout.set_halign(gtk::Align::Center);
        lock_signout.set_action_name(Some("win.logout"));
        let lock_child = boxed();
        lock_child.set_spacing(12);
        lock_child.append(&unlock_button);
        lock_child.append(&lock_signout);
        let locked = adw::StatusPage::builder()
            .title("Locked")
            .description("Revealed values, item details and this app’s clipboard were cleared.\nUnlock to reload your vaults.")
            .icon_name("system-lock-screen-symbolic")
            .child(&lock_child)
            .build();
        let (locked, _) = toolbar(&locked, "Proton Pass GTK");

        let stack = gtk::Stack::new();
        stack.add_named(&outer, Some("vaults"));
        stack.add_named(&signin, Some("signin"));
        stack.add_named(&locked, Some("locked"));
        let root = boxed();
        if backend.mock {
            let banner = adw::Banner::new("DEMO · Fake data only — no Proton account");
            banner.set_revealed(true);
            root.append(&banner);
        }
        root.append(&stack);
        stack.set_vexpand(true);
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&root));
        win.set_content(Some(&toast));
        let menu = gio::Menu::new();
        let section = gio::Menu::new();
        section.append(Some("Refresh"), Some("win.refresh"));
        section.append(Some("Lock"), Some("win.lock"));
        menu.append_section(None, &section);
        let section = gio::Menu::new();
        section.append(Some("Preferences"), Some("win.preferences"));
        section.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
        section.append(Some("Sign Out…"), Some("win.logout"));
        menu.append_section(None, &section);
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main menu")
            .build();
        vault_header.pack_end(&menu_button);
        let lock_button = icon_button("system-lock-screen-symbolic", "Lock (Ctrl+L)");
        lock_button.set_action_name(Some("win.lock"));
        vault_header.pack_start(&lock_button);
        let copy = icon_button("edit-copy-symbolic", "Copy password (Ctrl+Shift+C)");
        copy.set_action_name(Some("win.copy-password"));
        detail_header.pack_end(&copy);
        let edit_button = icon_button("document-edit-symbolic", "Edit (Ctrl+E)");
        edit_button.set_action_name(Some("win.edit"));
        detail_header.pack_end(&edit_button);
        let trash_button = icon_button("user-trash-symbolic", "Move to trash");
        trash_button.set_action_name(Some("win.trash"));
        detail_header.pack_start(&trash_button);
        let s = Rc::new(Self {
            win,
            backend,
            toast,
            stack,
            outer,
            inner,
            vault_list,
            item_list,
            search,
            detail_box,
            detail_scroll,
            item_page,
            detail_page,
            sign_label,
            sign_button,
            cancel_button,
            extra_switch,
            new_button,
            edit_button,
            trash_button,
            items: RefCell::new(vec![]),
            current: RefCell::new(None),
            vault: RefCell::new(None),
            epoch: Cell::new(0),
            request: Cell::new(0),
            clipboard: RefCell::new(None),
            login: Rc::new(RefCell::new(None)),
            prefs: RefCell::new(prefs),
            prefs_path,
            last_activity: Cell::new(glib::monotonic_time()),
            locked: Cell::new(false),
            dialogs: RefCell::new(vec![]),
        });
        s.actions(app);
        s.connections();
        match std::env::var("PPG_COLOR_SCHEME").as_deref() {
            Ok("dark") => apply_color_scheme(ColorScheme::Dark),
            Ok("light") => apply_color_scheme(ColorScheme::Light),
            _ => apply_color_scheme(s.prefs.borrow().color_scheme),
        }
        s.empty_detail();
        s.refresh();
        s.start_auto_lock();
        s.win.present();
        if s.backend.mock && std::env::var_os("PPG_CAPTURE").is_some() {
            s.capture_setup(w, h);
        }
        s
    }

    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.win
    }
    fn notify(&self, s: &str) {
        self.toast.add_toast(adw::Toast::new(s));
    }
    fn update_prefs(&self, f: impl FnOnce(&mut Prefs)) {
        let mut p = self.prefs.borrow_mut();
        let before = p.clone();
        f(&mut p);
        if *p != before && p.save(&self.prefs_path).is_err() {
            drop(p);
            self.notify("Could not save preferences");
        }
    }
    fn track_dialog(&self, d: &impl IsA<adw::Dialog>) {
        let d = d.clone().upcast::<adw::Dialog>();
        self.dialogs.borrow_mut().retain(|d| d.parent().is_some());
        self.dialogs.borrow_mut().push(d);
    }
    fn close_dialogs(&self) {
        for d in self.dialogs.borrow_mut().drain(..) {
            d.force_close();
        }
    }
    fn cancel_login(&self) {
        if let Some(p) = self.login.borrow_mut().take() {
            p.force_exit();
        }
    }
    fn clear_clipboard(&self) {
        if let Some(p) = self.clipboard.borrow_mut().take() {
            let c = self.win.clipboard();
            if c.content().as_ref() == Some(&p) {
                let _ = c.set_content(gdk::ContentProvider::NONE);
            }
        }
    }
    fn clear_detail(&self) {
        self.request.set(self.request.get() + 1);
        self.current.borrow_mut().take();
        while let Some(c) = self.detail_box.first_child() {
            self.detail_box.remove(&c);
        }
        self.update_item_actions();
    }
    fn empty_detail(&self) {
        self.clear_detail();
        self.detail_page.set_title("Details");
        self.detail_box.append(
            &adw::StatusPage::builder()
                .title("Choose an item")
                .description("Select an item to view its details.")
                .icon_name("dialog-password-symbolic")
                .build(),
        );
    }
    fn update_item_actions(&self) {
        let current = self.current.borrow();
        let editable = self.backend.can_edit();
        let login = current.as_ref().is_some_and(|d| d.item.kind == "login");
        for (name, enabled) in [
            ("edit", editable && login),
            ("trash", editable && current.is_some()),
            ("new-login", editable && self.vault.borrow().is_some()),
        ] {
            if let Some(a) = self
                .win
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(enabled);
            }
        }
        self.edit_button.set_visible(editable && login);
        self.trash_button.set_visible(editable && current.is_some());
        self.new_button.set_visible(editable);
    }
    /// Drop everything that could reveal vault contents.
    fn wipe(&self) {
        self.epoch.set(self.epoch.get() + 1);
        self.close_dialogs();
        self.clear_clipboard();
        self.empty_detail();
        self.items.borrow_mut().clear();
        self.search.set_text("");
        Self::clear_list(&self.vault_list);
        Self::clear_list(&self.item_list);
    }
    fn signed_out(&self) {
        self.sign_label
            .set_label("Sign in securely using Proton’s website.");
        self.sign_button.set_sensitive(true);
        self.cancel_button.set_visible(false);
        self.cancel_login();
        self.locked.set(false);
        self.wipe();
        self.vault.borrow_mut().take();
        self.stack.set_visible_child_name("signin");
    }
    /// App lock: clears secrets and item data from the UI and memory.
    /// This is a privacy lock; pass-cli's own session lock (if configured)
    /// is the server-enforced barrier.
    pub fn lock(&self) {
        if self.stack.visible_child_name().as_deref() != Some("vaults") {
            return;
        }
        self.locked.set(true);
        self.wipe();
        self.stack.set_visible_child_name("locked");
    }
    fn unlock(self: &Rc<Self>) {
        if !self.locked.get() {
            return;
        }
        self.locked.set(false);
        self.touch();
        self.refresh();
    }
    fn touch(&self) {
        self.last_activity.set(glib::monotonic_time());
    }
    /// Returns true when the inactivity limit was reached and the app locked.
    fn check_auto_lock(&self, now_us: i64) -> bool {
        let minutes = self.prefs.borrow().auto_lock_minutes;
        if minutes == 0 || self.locked.get() {
            return false;
        }
        if now_us - self.last_activity.get() >= i64::from(minutes) * 60_000_000 {
            self.lock();
            return self.locked.get();
        }
        false
    }
    fn start_auto_lock(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(5, move || {
            let Some(s) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            s.check_auto_lock(glib::monotonic_time());
            glib::ControlFlow::Continue
        });
    }
    fn refresh(self: &Rc<Self>) {
        if self.locked.get() {
            return;
        }
        let s = self.clone();
        glib::spawn_future_local(async move {
            match s.backend.info().await {
                Ok(account) => {
                    s.sign_label.set_label(&account);
                    match s.backend.vaults().await {
                        Ok(v) if !s.locked.get() => s.show_vaults(v),
                        Ok(_) => (),
                        Err(e) => s.notify(e),
                    }
                }
                Err(_) => s.signed_out(),
            }
        });
    }
    fn clear_list(list: &gtk::ListBox) {
        while let Some(c) = list.first_child() {
            list.remove(&c);
        }
    }
    fn show_vaults(self: &Rc<Self>, vaults: Vec<Vault>) {
        self.stack.set_visible_child_name("vaults");
        Self::clear_list(&self.vault_list);
        for v in &vaults {
            let r = row(&v.name, "Vault");
            r.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
            r.set_activatable(true);
            self.vault_list.append(&r);
            let s = Rc::downgrade(self);
            let v = v.clone();
            r.connect_activated(move |_| {
                if let Some(s) = s.upgrade() {
                    s.load_items(v.clone(), true, None);
                }
            });
        }
        let last = self.prefs.borrow().last_vault.clone();
        let pick = vaults.iter().position(|v| v.share_id == last).unwrap_or(0);
        if let Some(v) = vaults.get(pick) {
            if let Some(r) = self.vault_list.row_at_index(pick as i32) {
                self.vault_list.select_row(Some(&r));
            }
            self.load_items(v.clone(), false, None);
        } else {
            self.notify("No vaults available");
        }
    }
    /// `reopen`: item to open after loading (after create/edit). Matched by
    /// ID, or by title when the backend did not report a new ID.
    fn load_items(self: &Rc<Self>, v: Vault, navigate: bool, reopen: Option<Item>) {
        self.empty_detail();
        if reopen.is_none() {
            self.search.set_text("");
        }
        self.items.borrow_mut().clear();
        Self::clear_list(&self.item_list);
        self.item_page.set_title(&v.name);
        self.update_prefs(|p| p.last_vault = v.share_id.clone());
        *self.vault.borrow_mut() = Some(v.clone());
        self.update_item_actions();
        self.epoch.set(self.epoch.get() + 1);
        let epoch = self.epoch.get();
        if navigate {
            self.outer.set_show_content(true);
            self.inner.set_show_content(false);
        }
        let s = self.clone();
        glib::spawn_future_local(async move {
            match s.backend.items(&v.share_id).await {
                Ok(items) if s.epoch.get() == epoch => {
                    *s.items.borrow_mut() = items;
                    s.filter();
                    let reopen = reopen.and_then(|want| {
                        s.items
                            .borrow()
                            .iter()
                            .find(|i| {
                                if want.id.is_empty() {
                                    i.title == want.title
                                } else {
                                    i.id == want.id
                                }
                            })
                            .cloned()
                    });
                    if let Some(i) = reopen {
                        s.open(i);
                    }
                }
                Ok(_) => (),
                Err(e) => s.notify(e),
            }
        });
    }
    fn filter(self: &Rc<Self>) {
        Self::clear_list(&self.item_list);
        let query = self.search.text().to_lowercase();
        for i in self
            .items
            .borrow()
            .iter()
            .filter(|i| i.title.to_lowercase().contains(&query) || i.kind.contains(&query))
        {
            let r = row(&i.title, &i.kind);
            r.set_activatable(true);
            r.add_prefix(&gtk::Image::from_icon_name(match i.kind.as_str() {
                "login" => "dialog-password-symbolic",
                "creditcard" => "document-properties-symbolic",
                "note" => "document-edit-symbolic",
                _ => "text-x-generic-symbolic",
            }));
            self.item_list.append(&r);
            let s = Rc::downgrade(self);
            let i = i.clone();
            r.connect_activated(move |_| {
                if let Some(s) = s.upgrade() {
                    s.open(i.clone());
                }
            });
        }
        if self.item_list.first_child().is_none() {
            let empty = row("No items found", "Try a different search");
            empty.set_activatable(false);
            self.item_list.append(&empty);
        }
    }
    fn open(self: &Rc<Self>, i: Item) {
        self.clear_detail();
        let request = self.request.get();
        self.detail_page.set_title(&i.title);
        self.inner.set_show_content(true);
        self.outer.set_show_content(true);
        let s = self.clone();
        glib::spawn_future_local(async move {
            match s.backend.detail(&i).await {
                Ok(d) if request == s.request.get() => s.show_detail(d),
                Ok(_) => (),
                Err(e) => s.notify(e),
            }
        });
    }
    fn show_detail(self: &Rc<Self>, d: Detail) {
        let title = gtk::Label::new(Some(&d.item.title));
        title.add_css_class("title-1");
        title.set_wrap(true);
        title.set_xalign(0.0);
        self.detail_box.append(&title);
        let label = gtk::Label::new(Some(&d.item.kind));
        label.add_css_class("dim-label");
        label.set_xalign(0.0);
        self.detail_box.append(&label);
        let g = adw::PreferencesGroup::new();
        g.set_title("Item details");
        for field in &d.fields {
            let caption = if field.section.is_empty() {
                field.label.clone()
            } else {
                format!("{} · {}", field.section, field.label)
            };
            let r = row(&caption, field.value.as_deref().unwrap_or(HIDDEN));
            if field.kind == Kind::Totp {
                self.totp_row(&r, field);
            }
            let b = icon_button("edit-copy-symbolic", &format!("Copy {}", r.title()));
            b.set_valign(gtk::Align::Center);
            r.add_suffix(&b);
            let s = Rc::downgrade(self);
            let i = d.item.clone();
            let f = field.clone();
            b.connect_clicked(move |_| {
                if let Some(s) = s.upgrade() {
                    s.copy_field(i.clone(), f.clone());
                }
            });
            if field.kind != Kind::Text {
                let eye = icon_button(
                    "view-reveal-symbolic",
                    &format!("Reveal {} for {REVEAL_SECONDS} seconds", r.title()),
                );
                eye.set_valign(gtk::Align::Center);
                r.add_suffix(&eye);
                let s = Rc::downgrade(self);
                let i = d.item.clone();
                let f = field.clone();
                let weak = r.downgrade();
                let reveal_generation = Rc::new(Cell::new(0));
                eye.connect_clicked(move |_| {
                    if let (Some(s), Some(r)) = (s.upgrade(), weak.upgrade()) {
                        s.reveal(i.clone(), f.clone(), r, reveal_generation.clone());
                    }
                });
            }
            g.add(&r);
        }
        for url in &d.urls {
            let r = row("Website", url);
            g.add(&r);
        }
        if !d.alias.is_empty() {
            g.add(&row("Alias email", &d.alias));
        }
        self.detail_box.append(&g);
        if !d.note.is_empty() {
            let note = adw::PreferencesGroup::new();
            note.set_title("Note");
            let label = gtk::Label::new(Some(&d.note));
            label.set_wrap(true);
            label.set_selectable(true);
            label.set_xalign(0.0);
            note.add(&label);
            self.detail_box.append(&note);
        }
        if !d.attachments.is_empty() {
            let files = adw::PreferencesGroup::new();
            files.set_title("Attachments");
            if self.backend.mock {
                files.set_description(Some("Demo files: downloads save a small fake text file."));
            }
            for a in &d.attachments {
                let subtitle = if a.mime_type.is_empty() {
                    model::human_size(a.size)
                } else {
                    format!("{} · {}", model::human_size(a.size), a.mime_type)
                };
                let r = row(&a.name, &subtitle);
                r.add_prefix(&gtk::Image::from_icon_name("mail-attachment-symbolic"));
                let b = icon_button("folder-download-symbolic", &format!("Download {}", a.name));
                b.set_valign(gtk::Align::Center);
                r.add_suffix(&b);
                let s = Rc::downgrade(self);
                let i = d.item.clone();
                let a = a.clone();
                b.connect_clicked(move |_| {
                    if let Some(s) = s.upgrade() {
                        s.download(i.clone(), a.clone());
                    }
                });
                files.add(&r);
            }
            self.detail_box.append(&files);
        }
        if self.backend.can_edit() {
            let trash = gtk::Button::with_label("Move to Trash…");
            trash.add_css_class("destructive-action");
            trash.set_halign(gtk::Align::Center);
            trash.set_margin_top(8);
            trash.set_action_name(Some("win.trash"));
            self.detail_box.append(&trash);
        }
        *self.current.borrow_mut() = Some(d);
        self.update_item_actions();
    }
    /// One-time code row: title, live countdown for the field's own period.
    fn totp_row(&self, r: &adw::ActionRow, field: &Field) {
        let period = field.period.max(1);
        let base = if field.key == "totp" {
            "One-time code".to_string()
        } else {
            format!("One-time code · {}", field.label)
        };
        r.set_title(&if period == model::DEFAULT_TOTP_PERIOD {
            base
        } else {
            format!("{base} · {period} s")
        });
        let countdown = gtk::Label::new(Some(&format!(
            "{} s",
            model::totp_remaining(unix_now(), period)
        )));
        countdown.add_css_class("dim-label");
        countdown.add_css_class("numeric");
        countdown.set_tooltip_text(Some(&format!("Code changes every {period} seconds")));
        r.add_suffix(&countdown);
        let weak = countdown.downgrade();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(l) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            l.set_label(&format!("{} s", model::totp_remaining(unix_now(), period)));
            glib::ControlFlow::Continue
        });
    }
    fn copy_field(self: &Rc<Self>, i: Item, f: Field) {
        let s = self.clone();
        let epoch = self.epoch.get();
        let request = self.request.get();
        glib::spawn_future_local(async move {
            let value = if let Some(v) = f.value.clone() {
                Ok(Zeroizing::new(v))
            } else {
                s.backend.secret(&i, &f).await
            };
            match value {
                Ok(value) if epoch == s.epoch.get() && request == s.request.get() => {
                    let provider = gdk::ContentProvider::for_value(&value.as_str().to_value());
                    let c = s.win.clipboard();
                    if c.set_content(Some(&provider)).is_ok() {
                        *s.clipboard.borrow_mut() = Some(provider.clone());
                        let secs = s.prefs.borrow().clipboard_seconds;
                        s.notify(&format!("Copied · clipboard clears in {secs} seconds"));
                        let weak = Rc::downgrade(&s);
                        glib::timeout_add_local_once(
                            Duration::from_secs(u64::from(secs)),
                            move || {
                                if c.content().as_ref() == Some(&provider) {
                                    let _ = c.set_content(gdk::ContentProvider::NONE);
                                }
                                if let Some(s) = weak.upgrade() {
                                    if s.clipboard.borrow().as_ref() == Some(&provider) {
                                        s.clipboard.borrow_mut().take();
                                    }
                                }
                            },
                        );
                    }
                }
                Ok(_) => (),
                Err(e) => s.notify(e),
            }
        });
    }
    fn reveal(self: &Rc<Self>, i: Item, f: Field, r: adw::ActionRow, generation: Rc<Cell<u64>>) {
        generation.set(generation.get() + 1);
        let token = generation.get();
        if r.subtitle().as_deref() != Some(HIDDEN) {
            r.set_subtitle(HIDDEN);
            return;
        }
        let s = self.clone();
        let request = self.request.get();
        let weak = r.downgrade();
        glib::spawn_future_local(async move {
            match s.backend.secret(&i, &f).await {
                Ok(v) if request == s.request.get() && token == generation.get() => {
                    if let Some(r) = weak.upgrade() {
                        r.set_subtitle(&v);
                        let weak = r.downgrade();
                        let gen_hide = generation.clone();
                        glib::timeout_add_local_once(
                            Duration::from_secs(REVEAL_SECONDS),
                            move || {
                                if token == gen_hide.get() {
                                    gen_hide.set(token + 1);
                                    if let Some(r) = weak.upgrade() {
                                        r.set_subtitle(HIDDEN);
                                    }
                                }
                            },
                        );
                        if f.kind == Kind::Totp {
                            s.follow_totp(i, f, &r, generation, token, request);
                        }
                    }
                }
                Ok(_) => (),
                Err(e) => s.notify(e),
            }
        });
    }
    /// Refresh a revealed code when its own period rolls over.
    fn follow_totp(
        self: &Rc<Self>,
        i: Item,
        f: Field,
        r: &adw::ActionRow,
        generation: Rc<Cell<u64>>,
        token: u64,
        request: u64,
    ) {
        let weak = r.downgrade();
        let weak_s = Rc::downgrade(self);
        let period = f.period.max(1);
        let mut step = model::totp_step(unix_now(), period);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            if token != generation.get() {
                return glib::ControlFlow::Break;
            }
            let (Some(r), Some(s)) = (weak.upgrade(), weak_s.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if request != s.request.get() {
                return glib::ControlFlow::Break;
            }
            let now = model::totp_step(unix_now(), period);
            if now != step {
                step = now;
                let i = i.clone();
                let f = f.clone();
                let g = generation.clone();
                let weak = r.downgrade();
                glib::spawn_future_local(async move {
                    match s.backend.secret(&i, &f).await {
                        Ok(v) if token == g.get() && request == s.request.get() => {
                            if let Some(r) = weak.upgrade() {
                                r.set_subtitle(&v);
                            }
                        }
                        _ => {
                            if let Some(r) = weak.upgrade() {
                                r.set_subtitle(HIDDEN);
                            }
                        }
                    }
                });
            }
            glib::ControlFlow::Continue
        });
    }
    fn download(self: &Rc<Self>, i: Item, a: Attachment) {
        let s = self.clone();
        let epoch = self.epoch.get();
        glib::spawn_future_local(async move {
            let dest = if let Some(dir) =
                std::env::var_os("PPG_MOCK_DOWNLOAD_DIR").filter(|_| s.backend.mock)
            {
                PathBuf::from(dir).join(&a.name)
            } else {
                let d = gtk::FileDialog::builder()
                    .title("Save Attachment")
                    .initial_name(a.name.as_str())
                    .modal(true)
                    .build();
                match d.save_future(Some(&s.win)).await {
                    Ok(f) => match f.path() {
                        Some(p) => p,
                        None => return s.notify("Choose a local folder"),
                    },
                    Err(_) => return,
                }
            };
            if epoch != s.epoch.get() {
                return;
            }
            match s.backend.download_attachment(&i, &a, &dest).await {
                Ok(()) => s.notify(&format!("Saved {}", a.name)),
                Err(e) => s.notify(e),
            }
        });
    }
    fn focus_search(&self) {
        self.outer.set_show_content(true);
        self.inner.set_show_content(false);
        self.search.grab_focus();
    }
    fn connections(self: &Rc<Self>) {
        let s = Rc::downgrade(self);
        self.search.connect_search_changed(move |_| {
            if let Some(s) = s.upgrade() {
                s.filter();
            }
        });
        let s = Rc::downgrade(self);
        self.sign_button.connect_clicked(move |_| {
            if let Some(s) = s.upgrade() {
                s.start_login();
            }
        });
        let s = Rc::downgrade(self);
        self.cancel_button.connect_clicked(move |_| {
            if let Some(s) = s.upgrade() {
                s.cancel_login();
            }
        });
        let s = Rc::downgrade(self);
        self.extra_switch.connect_active_notify(move |w| {
            if let Some(s) = s.upgrade() {
                let on = w.is_active();
                s.update_prefs(|p| p.extra_password = on);
            }
        });
        // Any input counts as activity for auto-lock (capture phase, never consumed).
        let activity_key = gtk::EventControllerKey::new();
        activity_key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = Rc::downgrade(self);
        activity_key.connect_key_pressed(move |_, _, _, _| {
            if let Some(s) = s.upgrade() {
                s.touch();
            }
            glib::Propagation::Proceed
        });
        self.win.add_controller(activity_key);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = Rc::downgrade(self);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(s) = s.upgrade() {
                s.touch();
            }
        });
        self.win.add_controller(click);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = Rc::downgrade(self);
        scroll.connect_scroll(move |_, _, _| {
            if let Some(s) = s.upgrade() {
                s.touch();
            }
            glib::Propagation::Proceed
        });
        self.win.add_controller(scroll);

        let keys = gtk::EventControllerKey::new();
        let s = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(s) = s.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !gtk::prelude::GtkWindowExt::focus(&s.win).is_some_and(|f| {
                f == s.search.clone().upcast::<gtk::Widget>() || f.is_ancestor(&s.search)
            }) && s.stack.visible_child_name().as_deref() == Some("vaults")
                && !mods.intersects(
                    gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK,
                )
            {
                if let Some(c) = key.to_unicode().filter(|c| c.is_alphanumeric()) {
                    s.focus_search();
                    s.search.set_text(&c.to_string());
                    s.search.set_position(-1);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        self.win.add_controller(keys);
        let s = Rc::downgrade(self);
        self.win.connect_close_request(move |w| {
            if let Some(s) = s.upgrade() {
                s.cancel_login();
                s.clear_clipboard();
                s.clear_detail();
                if std::env::var_os("PPG_SIZE").is_none() {
                    let (width, height) = w.default_size();
                    let maximized = w.is_maximized();
                    s.update_prefs(|p| {
                        p.maximized = maximized;
                        if !maximized && width > 0 && height > 0 {
                            p.width = width;
                            p.height = height;
                        }
                    });
                }
            }
            glib::Propagation::Proceed
        });
    }
    /// Ask for the Pass extra password. The value lives only in a zeroizing
    /// buffer and the entry is cleared immediately.
    async fn ask_extra_password(self: &Rc<Self>, error: Option<&str>) -> Option<Zeroizing<String>> {
        let d = self.extra_password_dialog(error);
        let entry = d
            .extra_child()
            .and_then(|c| c.first_child())
            .and_downcast::<adw::PasswordEntryRow>()?;
        let response = d.choose_future(Some(&self.win)).await;
        let value = Zeroizing::new(entry.text().to_string());
        entry.set_text("");
        (response == "continue" && !value.is_empty()).then_some(value)
    }
    fn extra_password_dialog(self: &Rc<Self>, error: Option<&str>) -> adw::AlertDialog {
        let body = "Your Proton Pass is protected by an extra password. It is passed to pass-cli through a private pipe and is never saved, logged or shown.";
        let d = adw::AlertDialog::builder()
            .heading("Pass Extra Password")
            .body(match error {
                Some(e) => format!("{e}\n\n{body}"),
                None => body.into(),
            })
            .build();
        d.add_responses(&[("cancel", "Cancel"), ("continue", "Continue")]);
        d.set_response_appearance("continue", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("continue"));
        d.set_close_response("cancel");
        d.set_response_enabled("continue", false);
        let entry = adw::PasswordEntryRow::builder()
            .title("Extra password")
            .activates_default(true)
            .build();
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.append(&entry);
        d.set_extra_child(Some(&list));
        let weak = d.downgrade();
        entry.connect_changed(move |e| {
            if let Some(d) = weak.upgrade() {
                d.set_response_enabled("continue", !e.text().is_empty());
            }
        });
        d.set_focus(Some(&entry));
        self.track_dialog(&d);
        d
    }
    fn start_login(self: &Rc<Self>) {
        self.sign_button.set_sensitive(false);
        let s = self.clone();
        glib::spawn_future_local(async move {
            let mut extra = None;
            let mut error = None;
            if s.prefs.borrow().extra_password {
                extra = s.ask_extra_password(None).await;
                if extra.is_none() {
                    s.sign_button.set_sensitive(true);
                    return;
                }
            }
            // Like pass-cli itself: at most three extra-password attempts.
            for _attempt in 0..3 {
                if let Some(e) = error {
                    extra = s.ask_extra_password(Some(e)).await;
                    if extra.is_none() {
                        break;
                    }
                }
                s.cancel_button.set_visible(true);
                s.sign_label.set_label("Complete sign-in in your browser…");
                let parent = s.win.clone();
                let result = s
                    .backend
                    .login(s.login.clone(), extra.take(), move |url| {
                        // Launch via the desktop portal inside Flatpak; do not log the URL.
                        gtk::UriLauncher::new(&url).launch(
                            Some(&parent),
                            gio::Cancellable::NONE,
                            |_result| {},
                        );
                    })
                    .await;
                s.cancel_button.set_visible(false);
                match result {
                    Ok(()) => {
                        s.sign_button.set_sensitive(true);
                        s.touch();
                        return s.refresh();
                    }
                    Err(e)
                        if e == backend::ERR_EXTRA_PASSWORD_REQUIRED
                            || e == backend::ERR_EXTRA_PASSWORD_REJECTED =>
                    {
                        if e == backend::ERR_EXTRA_PASSWORD_REQUIRED {
                            s.extra_switch.set_active(true);
                        }
                        s.sign_label.set_label(e);
                        error = Some(e);
                    }
                    Err(e) => {
                        s.sign_label.set_label(e);
                        s.notify(e);
                        break;
                    }
                }
            }
            s.sign_button.set_sensitive(true);
        });
    }
    fn actions(self: &Rc<Self>, app: &adw::Application) {
        for (name, accels) in [
            ("search", vec!["<Control>f"]),
            ("refresh", vec!["<Control>r"]),
            ("close", vec!["<Control>w"]),
            ("quit", vec!["<Control>q"]),
            ("shortcuts", vec!["<Control>question"]),
            ("preferences", vec!["<Control>comma"]),
            ("lock", vec!["<Control>l"]),
            ("unlock", vec![]),
            ("new-login", vec!["<Control>n"]),
            ("edit", vec!["<Control>e"]),
            ("trash", vec![]),
            ("copy-username", vec!["<Control><Shift>u"]),
            ("copy-password", vec!["<Control><Shift>c"]),
            ("copy-totp", vec!["<Control><Shift>t"]),
            ("logout", vec![]),
        ] {
            let a = gio::SimpleAction::new(name, None);
            let s = Rc::downgrade(self);
            a.connect_activate(move |_, _| {
                if let Some(s) = s.upgrade() {
                    match name {
                        "search" => s.focus_search(),
                        "refresh" => s.refresh(),
                        "close" | "quit" => s.win.close(),
                        "logout" => s.confirm_logout(),
                        "shortcuts" => s.shortcuts(),
                        "preferences" => s.preferences(),
                        "lock" => s.lock(),
                        "unlock" => s.unlock(),
                        "new-login" => s.edit_dialog(None),
                        "edit" => {
                            let d = s.current.borrow().clone();
                            if let Some(d) = d.filter(|d| d.item.kind == "login") {
                                s.edit_dialog(Some(d));
                            }
                        }
                        "trash" => s.confirm_trash(),
                        copy => {
                            let key = match copy {
                                "copy-username" => "username",
                                "copy-totp" => "totp",
                                _ => "password",
                            };
                            let d = s.current.borrow().clone();
                            if let Some(d) = d {
                                if let Some(f) =
                                    d.fields.iter().find(|f| f.key == key).or_else(|| {
                                        if key == "username" {
                                            d.fields.iter().find(|f| f.key == "email")
                                        } else {
                                            None
                                        }
                                    })
                                {
                                    s.copy_field(d.item.clone(), f.clone());
                                }
                            }
                        }
                    }
                }
            });
            self.win.add_action(&a);
            app.set_accels_for_action(&format!("win.{name}"), &accels);
        }
        self.update_item_actions();
    }
    fn preferences(self: &Rc<Self>) {
        let p = self.prefs.borrow().clone();
        let d = adw::PreferencesDialog::new();
        d.set_title("Preferences");
        d.set_width_request(300);
        let page = adw::PreferencesPage::builder()
            .title("General")
            .icon_name("preferences-system-symbolic")
            .build();
        let security = adw::PreferencesGroup::builder()
            .title("Security")
            .description("Preferences never contain passwords or item data.")
            .build();
        let clip_labels: Vec<String> = prefs::CLIPBOARD_CHOICES
            .iter()
            .map(|s| format!("{s} seconds"))
            .collect();
        let clip = adw::ComboRow::builder()
            .title("Clear clipboard after")
            .model(&gtk::StringList::new(
                &clip_labels.iter().map(String::as_str).collect::<Vec<_>>(),
            ))
            .selected(
                prefs::CLIPBOARD_CHOICES
                    .iter()
                    .position(|c| *c == p.clipboard_seconds)
                    .unwrap_or(2) as u32,
            )
            .build();
        let lock_labels: Vec<String> = prefs::AUTO_LOCK_CHOICES
            .iter()
            .map(|m| minutes_label(*m))
            .collect();
        let lock = adw::ComboRow::builder()
            .title("Lock after inactivity")
            .subtitle("Clears revealed values and item details")
            .model(&gtk::StringList::new(
                &lock_labels.iter().map(String::as_str).collect::<Vec<_>>(),
            ))
            .selected(
                prefs::AUTO_LOCK_CHOICES
                    .iter()
                    .position(|c| *c == p.auto_lock_minutes)
                    .unwrap_or(3) as u32,
            )
            .build();
        security.add(&clip);
        security.add(&lock);
        let signin = adw::PreferencesGroup::builder().title("Sign-in").build();
        let extra = adw::SwitchRow::builder()
            .title("Ask for Pass extra password")
            .subtitle("Sent to pass-cli through a private pipe, never stored")
            .active(p.extra_password)
            .build();
        signin.add(&extra);
        let look = adw::PreferencesGroup::builder().title("Appearance").build();
        let scheme = adw::ComboRow::builder()
            .title("Style")
            .model(&gtk::StringList::new(
                &ColorScheme::ALL.map(ColorScheme::label),
            ))
            .selected(
                ColorScheme::ALL
                    .iter()
                    .position(|c| *c == p.color_scheme)
                    .unwrap_or(0) as u32,
            )
            .build();
        look.add(&scheme);
        page.add(&security);
        page.add(&signin);
        page.add(&look);
        d.add(&page);
        let s = Rc::downgrade(self);
        clip.connect_selected_notify(move |r| {
            if let (Some(s), Some(v)) = (
                s.upgrade(),
                prefs::CLIPBOARD_CHOICES.get(r.selected() as usize),
            ) {
                s.update_prefs(|p| p.clipboard_seconds = *v);
            }
        });
        let s = Rc::downgrade(self);
        lock.connect_selected_notify(move |r| {
            if let (Some(s), Some(v)) = (
                s.upgrade(),
                prefs::AUTO_LOCK_CHOICES.get(r.selected() as usize),
            ) {
                s.touch();
                s.update_prefs(|p| p.auto_lock_minutes = *v);
            }
        });
        let s = Rc::downgrade(self);
        extra.connect_active_notify(move |r| {
            if let Some(s) = s.upgrade() {
                // The sign-in page switch writes the preference.
                s.extra_switch.set_active(r.is_active());
            }
        });
        let s = Rc::downgrade(self);
        scheme.connect_selected_notify(move |r| {
            if let (Some(s), Some(c)) = (s.upgrade(), ColorScheme::ALL.get(r.selected() as usize)) {
                apply_color_scheme(*c);
                s.update_prefs(|p| p.color_scheme = *c);
            }
        });
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }
    /// Create (`None`) or edit a login. Mock backend only unless explicitly
    /// enabled; see `Backend::can_edit`.
    fn edit_dialog(self: &Rc<Self>, existing: Option<Detail>) {
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let Some(vault) = self.vault.borrow().clone() else {
            return;
        };
        let old = existing.as_ref().map(model::draft_from).unwrap_or_default();
        let creating = existing.is_none();
        let title = entry_row("Title", &old.title);
        let username = entry_row("Username", &old.username);
        let email = entry_row("Email", &old.email);
        let password = adw::PasswordEntryRow::builder()
            .title(if creating {
                "Password"
            } else {
                "New password (empty keeps current)"
            })
            .build();
        // Real pass-cli updates pass values in argv; never offer it there.
        password.set_visible(creating || self.backend.mock);
        let url = entry_row("Website", &old.url);
        url.set_input_purpose(gtk::InputPurpose::Url);
        email.set_input_purpose(gtk::InputPurpose::Email);
        let fields = adw::PreferencesGroup::new();
        if self.backend.mock {
            fields.set_description(Some(
                "Demo backend: changes stay in memory and are discarded on exit. Do not enter real secrets.",
            ));
        }
        for r in [&title, &username, &email] {
            fields.add(r);
        }
        fields.add(&password);
        fields.add(&url);
        let note_group = adw::PreferencesGroup::builder().title("Note").build();
        let note = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(10)
            .bottom_margin(10)
            .left_margin(12)
            .right_margin(12)
            .height_request(96)
            .build();
        note.buffer().set_text(&old.note);
        note.add_css_class("card");
        note.update_property(&[gtk::accessible::Property::Label("Note")]);
        note_group.add(&note);
        let content = boxed();
        content.set_spacing(18);
        content.set_margin_top(12);
        content.set_margin_bottom(24);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&fields);
        content.append(&note_group);
        let clamp = adw::Clamp::builder()
            .maximum_size(520)
            .child(&content)
            .build();
        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label(if creating { "Create" } else { "Save" });
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&scroll(&clamp)));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&view));
        let d = adw::Dialog::builder()
            .title(if creating { "New Login" } else { "Edit Login" })
            .content_width(480)
            .content_height(620)
            .width_request(300)
            .child(&toasts)
            .build();
        d.set_focus(Some(&title));
        let check = {
            let title = title.clone();
            let save = save.clone();
            move || save.set_sensitive(!title.text().trim().is_empty())
        };
        check();
        title.connect_changed(move |_| check());
        let weak = d.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(d) = weak.upgrade() {
                d.close();
            }
        });
        let s = Rc::downgrade(self);
        let weak = d.downgrade();
        let item = existing.map(|d| d.item);
        save.connect_clicked(move |save| {
            let (Some(s), Some(d)) = (s.upgrade(), weak.upgrade()) else {
                return;
            };
            let buf = note.buffer();
            let pw = Zeroizing::new(password.text().to_string());
            let draft = LoginDraft {
                title: title.text().into(),
                username: username.text().into(),
                email: email.text().into(),
                password: (!pw.is_empty()).then_some(pw),
                url: url.text().into(),
                note: buf.text(&buf.start_iter(), &buf.end_iter(), false).into(),
            };
            if let Err(e) = draft.validate() {
                toasts.add_toast(adw::Toast::new(e));
                return;
            }
            save.set_sensitive(false);
            let (old, item, vault, toasts, save, password) = (
                old.clone(),
                item.clone(),
                vault.clone(),
                toasts.clone(),
                save.clone(),
                password.clone(),
            );
            glib::spawn_future_local(async move {
                let result = match &item {
                    None => s.backend.create_login(&vault.share_id, &draft).await,
                    Some(i) => s.backend.update_login(i, &old, &draft).await,
                };
                drop(draft);
                match result {
                    Ok(saved) => {
                        password.set_text("");
                        d.close();
                        s.notify(if item.is_none() {
                            "Login created"
                        } else {
                            "Changes saved"
                        });
                        s.load_items(vault, false, Some(saved));
                    }
                    Err(e) => {
                        save.set_sensitive(true);
                        toasts.add_toast(adw::Toast::new(e));
                    }
                }
            });
        });
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }
    fn confirm_trash(self: &Rc<Self>) {
        let Some(detail) = self.current.borrow().clone() else {
            return;
        };
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let d = adw::AlertDialog::builder()
            .heading("Move to Trash?")
            .body(format!(
                "“{}” will be moved to the trash. {}",
                detail.item.title,
                if self.backend.mock {
                    "Demo backend: it is only removed from this session."
                } else {
                    "You can restore it from the trash in Proton Pass."
                }
            ))
            .build();
        d.add_responses(&[("cancel", "Cancel"), ("trash", "Move to Trash")]);
        d.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        self.track_dialog(&d);
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await != "trash" {
                return;
            }
            match s.backend.trash(&detail.item).await {
                Ok(()) => {
                    s.notify(&format!("Moved “{}” to trash", detail.item.title));
                    let v = s.vault.borrow().clone();
                    if let Some(v) = v {
                        s.load_items(v, false, None);
                        s.inner.set_show_content(false);
                    }
                }
                Err(e) => s.notify(e),
            }
        });
    }
    fn confirm_logout(self: &Rc<Self>) {
        let d=adw::AlertDialog::builder().heading("Sign out of Proton Pass?").body("This removes the CLI session and clears this app’s clipboard. Your vaults remain on Proton.").build();
        d.add_responses(&[("cancel", "Cancel"), ("logout", "Sign out")]);
        d.set_response_appearance("logout", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await == "logout" {
                match s.backend.logout().await {
                    Ok(()) => s.signed_out(),
                    Err(e) => s.notify(e),
                }
            }
        });
    }
    fn shortcuts(&self) {
        let secs = self.prefs.borrow().clipboard_seconds;
        let lock = minutes_label(self.prefs.borrow().auto_lock_minutes).to_lowercase();
        let d = adw::AlertDialog::builder()
            .heading("Keyboard Shortcuts")
            .body(format!(
                "Ctrl+F   Search\nCtrl+R   Refresh\nCtrl+L   Lock\nCtrl+N   New login\nCtrl+E   Edit login\nCtrl+,   Preferences\nCtrl+W / Ctrl+Q   Close / Quit\nCtrl+?   Keyboard shortcuts\nCtrl+Shift+U   Copy username\nCtrl+Shift+C   Copy password\nCtrl+Shift+T   Copy one-time code\n\nType to search in the current vault.\nRevealed values hide after {REVEAL_SECONDS} seconds; the clipboard clears after {secs} seconds.\nAuto-lock: {lock}."
            ))
            .build();
        d.add_response("close", "Close");
        d.present(Some(&self.win));
    }
    fn find_item(&self, pred: impl Fn(&Item) -> bool) -> Option<Item> {
        self.items.borrow().iter().find(|i| pred(i)).cloned()
    }
    /// Deterministic, mock-only screenshot harness: renders GTK, not a mockup.
    fn capture_setup(self: &Rc<Self>, w: i32, h: i32) {
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_enable_animations(false);
        }
        let s = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            let state = std::env::var("PPG_CAPTURE_STATE").unwrap_or_else(|_| "list".into());
            match state.as_str() {
                "signin" | "extra-password" => s.signed_out(),
                "vaults" => s.outer.set_show_content(false),
                "locked" => {
                    s.outer.set_show_content(true);
                    s.lock();
                }
                _ => {
                    s.outer.set_show_content(true);
                    s.inner.set_show_content(false);
                    if state == "search" {
                        s.focus_search();
                        s.search.set_text("example");
                    }
                    let want_totp = state == "totp";
                    let open = matches!(
                        state.as_str(),
                        "detail" | "copied-toast" | "edit" | "trash" | "attachments" | "totp"
                    ) || (w > 860 && state != "search");
                    if open {
                        let i = s.find_item(|i| {
                            if want_totp {
                                i.title == "Demo TOTP periods"
                            } else {
                                i.title == "Example Mail"
                            }
                        });
                        if let Some(i) = i {
                            s.open(i);
                        }
                    }
                }
            }
            let ss = s.clone();
            glib::timeout_add_local_once(Duration::from_millis(800), move || {
                let d = ss.current.borrow().clone();
                match state.as_str() {
                    "copied-toast" => {
                        if let Some(d) = d {
                            if let Some(f) = d.fields.iter().find(|f| f.key == "password") {
                                ss.copy_field(d.item.clone(), f.clone());
                            }
                        }
                    }
                    "extra-password" => {
                        let dialog = ss.extra_password_dialog(None);
                        if let Some(e) = dialog
                            .extra_child()
                            .and_then(|c| c.first_child())
                            .and_downcast::<adw::PasswordEntryRow>()
                        {
                            e.set_text("FAKE-demo");
                        }
                        dialog.present(Some(&ss.win));
                    }
                    "edit" => ss.edit_dialog(d),
                    "new" => ss.edit_dialog(None),
                    "trash" => ss.confirm_trash(),
                    "preferences" => ss.preferences(),
                    "attachments" => {
                        let adj = ss.detail_scroll.vadjustment();
                        adj.set_value(adj.upper());
                    }
                    "totp" => {
                        // Reveal the 60-second demo code.
                        if let Some(d) = d {
                            if let Some(f) = d.fields.iter().find(|f| f.key == "totp") {
                                let r =
                                    find_row(ss.detail_box.upcast_ref(), "One-time code · 60 s");
                                if let Some(r) = r {
                                    ss.reveal(d.item.clone(), f.clone(), r, Rc::new(Cell::new(0)));
                                }
                            }
                        }
                    }
                    _ => (),
                }
                let sss = ss.clone();
                glib::timeout_add_local_once(Duration::from_millis(400), move || {
                    if state == "attachments" {
                        let adj = sss.detail_scroll.vadjustment();
                        adj.set_value(adj.upper());
                    }
                    sss.capture_resize(w, h, 0);
                });
            });
        });
    }
    fn capture_resize(self: &Rc<Self>, w: i32, h: i32, tries: u8) {
        let content = self.win.content().unwrap();
        if tries < 5 && (content.width() != w || content.height() != h) {
            self.win.set_default_size(
                self.win.default_width() + w - content.width(),
                self.win.default_height() + h - content.height(),
            );
            let s = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                s.capture_resize(w, h, tries + 1);
            });
            return;
        }
        let s = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            let paint = gtk::WidgetPaintable::new(Some(&s.win));
            let snapshot = gtk::Snapshot::new();
            paint.snapshot(&snapshot, s.win.width() as f64, s.win.height() as f64);
            let node = snapshot.to_node().expect("Screenshot render node");
            let origin = content
                .compute_point(&s.win, &gtk::graphene::Point::new(0.0, 0.0))
                .unwrap();
            let rect = gtk::graphene::Rect::new(
                origin.x(),
                origin.y(),
                content.width() as f32,
                content.height() as f32,
            );
            let texture = s.win.renderer().unwrap().render_texture(&node, Some(&rect));
            let path = std::env::var("PPG_CAPTURE").unwrap();
            texture.save_to_png(path).expect("Save screenshot");
            s.close_dialogs();
            s.win.close();
        });
    }
}
fn find_row(w: &gtk::Widget, title: &str) -> Option<adw::ActionRow> {
    if let Some(r) = w.downcast_ref::<adw::ActionRow>() {
        if r.title() == title {
            return Some(r.clone());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(r) = find_row(&child, title) {
            return Some(r);
        }
        c = child.next_sibling();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn wait(ms: u64) {
        glib::timeout_future(Duration::from_millis(ms)).await;
    }
    #[test]
    #[ignore = "requires an isolated display and PPG_BACKEND=mock; runs 34 seconds"]
    fn adaptive_navigation_and_secret_expiry() {
        assert_eq!(std::env::var("PPG_BACKEND").as_deref(), Ok("mock"));
        let config = std::env::temp_dir().join(format!("ppg-test-{}", std::process::id()));
        // SAFETY: set before GTK starts any threads; the test runs single-threaded.
        unsafe { std::env::set_var("PPG_CONFIG_DIR", &config) };
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.isyourbrainfoss.ProtonPassGtk.Test")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let s = Window::new(&app);
        glib::MainContext::default().block_on(async {
            wait(500).await;
            assert!(s.outer.is_collapsed());
            assert!(s.inner.is_collapsed());
            s.win.set_default_size(800, 720);
            wait(250).await;
            assert!(s.outer.is_collapsed());
            assert!(!s.inner.is_collapsed());
            s.win.set_default_size(1280, 800);
            wait(250).await;
            assert!(!s.outer.is_collapsed());
            assert!(!s.inner.is_collapsed());
            s.win.set_default_size(360, 720);
            wait(250).await;
            s.focus_search();
            s.search.set_text("example");
            s.filter();
            assert!(s.item_list.row_at_index(0).is_some());
            assert!(s.item_list.row_at_index(1).is_none());
            let i = s.find_item(|i| i.title == "Example Mail").unwrap();
            s.open(i);
            wait(100).await;
            let d = s.current.borrow().clone().unwrap();
            assert_eq!(d.attachments.len(), 2);
            let f = d
                .fields
                .iter()
                .find(|f| f.key == "password")
                .unwrap()
                .clone();
            assert_eq!(f.value, None);
            let r = find_row(s.detail_box.upcast_ref(), "password").unwrap();
            s.copy_field(d.item.clone(), f.clone());
            wait(100).await;
            assert!(
                s.win
                    .clipboard()
                    .read_text_future()
                    .await
                    .unwrap()
                    .is_some()
            );
            s.win.clipboard().set_text("unrelated clipboard content");
            s.clear_clipboard();
            assert_eq!(
                s.win.clipboard().read_text_future().await.unwrap().unwrap(),
                "unrelated clipboard content"
            );
            s.copy_field(d.item.clone(), f.clone());
            s.reveal(d.item.clone(), f, r.clone(), Rc::new(Cell::new(0)));
            wait(100).await;
            assert_ne!(r.subtitle().as_deref(), Some(HIDDEN));
            wait(31_000).await;
            assert_eq!(r.subtitle().as_deref(), Some(HIDDEN));
            assert!(s.win.clipboard().content().is_none());

            // Mock attachment download writes only a fake demo file.
            let dl = config.join("downloads");
            std::fs::create_dir_all(&dl).unwrap();
            s.backend
                .download_attachment(&d.item, &d.attachments[0], &dl.join("a.pdf"))
                .await
                .unwrap();
            assert!(
                std::fs::read_to_string(dl.join("a.pdf"))
                    .unwrap()
                    .starts_with("FAKE demo attachment")
            );

            // Nonstandard TOTP periods are labelled and counted per period.
            s.search.set_text("");
            let t = s.find_item(|i| i.title == "Demo TOTP periods").unwrap();
            s.open(t);
            wait(100).await;
            assert!(find_row(s.detail_box.upcast_ref(), "One-time code · 60 s").is_some());
            assert!(
                find_row(
                    s.detail_box.upcast_ref(),
                    "One-time code · Legacy 2FA · 15 s"
                )
                .is_some()
            );
            assert!(find_row(s.detail_box.upcast_ref(), "One-time code · Backup 2FA").is_some());

            // Mock create → edit → trash, reopened in the UI.
            let vault = s.vault.borrow().clone().unwrap();
            let draft = LoginDraft {
                title: "Demo created login".into(),
                username: "demo".into(),
                password: Some(Zeroizing::new("FAKE-new-password".into())),
                ..Default::default()
            };
            let created = s
                .backend
                .create_login(&vault.share_id, &draft)
                .await
                .unwrap();
            s.load_items(vault.clone(), false, Some(created.clone()));
            wait(200).await;
            let cur = s.current.borrow().clone().unwrap();
            assert_eq!(cur.item.title, "Demo created login");
            assert!(s.edit_button.is_visible() && s.trash_button.is_visible());
            let mut renamed = model::draft_from(&cur);
            renamed.title = "Demo renamed login".into();
            let saved = s
                .backend
                .update_login(&cur.item, &model::draft_from(&cur), &renamed)
                .await
                .unwrap();
            s.load_items(vault.clone(), false, Some(saved.clone()));
            wait(200).await;
            assert_eq!(
                s.current.borrow().as_ref().unwrap().item.title,
                "Demo renamed login"
            );
            s.backend.trash(&saved).await.unwrap();
            s.load_items(vault.clone(), false, None);
            wait(200).await;
            assert!(s.find_item(|i| i.id == saved.id).is_none());

            // Preferences persist without secrets.
            s.update_prefs(|p| p.clipboard_seconds = 10);
            let file = config.join("settings.ini");
            let saved_prefs = Prefs::load(&file);
            assert_eq!(saved_prefs.clipboard_seconds, 10);
            assert_eq!(saved_prefs.last_vault, vault.share_id);
            assert!(!std::fs::read_to_string(&file).unwrap().contains("FAKE"));

            // Lock clears everything; unlock reloads.
            let i = s.find_item(|i| i.title == "Example Mail").unwrap();
            s.open(i);
            wait(100).await;
            s.preferences();
            s.lock();
            assert_eq!(s.stack.visible_child_name().as_deref(), Some("locked"));
            assert!(s.current.borrow().is_none());
            assert!(s.items.borrow().is_empty());
            assert!(s.vault_list.first_child().is_none());
            assert!(s.item_list.first_child().is_none());
            assert!(s.win.clipboard().content().is_none());
            s.refresh(); // must not reload while locked
            wait(200).await;
            assert!(s.items.borrow().is_empty());
            s.unlock();
            wait(300).await;
            assert_eq!(s.stack.visible_child_name().as_deref(), Some("vaults"));
            assert!(!s.items.borrow().is_empty());

            // Auto-lock after the configured inactivity.
            s.update_prefs(|p| p.auto_lock_minutes = 1);
            s.touch();
            let last = s.last_activity.get();
            assert!(!s.check_auto_lock(last + 59_000_000));
            assert!(s.check_auto_lock(last + 60_000_000));
            assert_eq!(s.stack.visible_child_name().as_deref(), Some("locked"));
            s.unlock();
            wait(300).await;
            s.update_prefs(|p| p.auto_lock_minutes = 0);
            assert!(!s.check_auto_lock(i64::MAX / 2));

            s.backend.logout().await.unwrap();
            s.signed_out();
            assert_eq!(s.stack.visible_child_name().as_deref(), Some("signin"));
            assert!(s.current.borrow().is_none());
            assert!(s.items.borrow().is_empty());
            s.win.close();
        });
        let _ = std::fs::remove_dir_all(&config);
    }
}
