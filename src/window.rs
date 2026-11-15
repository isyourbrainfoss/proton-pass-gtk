// SPDX-License-Identifier: GPL-3.0-or-later
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use proton_pass_gtk::{
    backend::Backend,
    model::{Detail, Field, Item, Kind, Vault},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

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
    item_page: adw::NavigationPage,
    detail_page: adw::NavigationPage,
    sign_label: gtk::Label,
    sign_button: gtk::Button,
    cancel_button: gtk::Button,
    items: RefCell<Vec<Item>>,
    current: RefCell<Option<Detail>>,
    epoch: Cell<u64>,
    request: Cell<u64>,
    clipboard: RefCell<Option<gdk::ContentProvider>>,
    login: Rc<RefCell<Option<gio::Subprocess>>>,
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
impl Window {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let backend = Backend::default();
        let size = std::env::var("PPG_SIZE").unwrap_or_else(|_| "1280x800".into());
        let (w, h) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?)))
            .unwrap_or((1280, 800));
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title("Proton Pass GTK")
            .default_width(w)
            .default_height(h)
            .width_request(320)
            .height_request(400)
            .build();
        let vault_list = gtk::ListBox::new();
        vault_list.add_css_class("navigation-sidebar");
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
        let ib = boxed();
        ib.append(&search);
        ib.append(&scroll(&item_list));
        let (items, _) = toolbar(&ib, "Items");
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
        let (details, detail_header) = toolbar(&scroll(&clamp), "Details");
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
        let sign_label = gtk::Label::new(Some("Sign in securely using Proton’s website."));
        sign_label.set_wrap(true);
        let sign_button = gtk::Button::with_label("Sign in with Proton");
        sign_button.add_css_class("suggested-action");
        sign_button.add_css_class("pill");
        sign_button.set_halign(gtk::Align::Center);
        let cancel_button = gtk::Button::with_label("Cancel sign-in");
        cancel_button.set_halign(gtk::Align::Center);
        cancel_button.set_visible(false);
        let sign_child = boxed();
        sign_child.set_spacing(16);
        sign_child.append(&sign_label);
        sign_child.append(&sign_button);
        sign_child.append(&cancel_button);
        let signin=adw::StatusPage::builder().title("Your passwords, everywhere").description("An unofficial, read-only Proton Pass client.\nAuthentication is handled by the official pass-cli.").icon_name("dialog-password-symbolic").child(&sign_child).build();
        let (signin, _) = toolbar(&signin, "Proton Pass GTK");
        let stack = gtk::Stack::new();
        stack.add_named(&outer, Some("vaults"));
        stack.add_named(&signin, Some("signin"));
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
        menu.append(Some("Refresh"), Some("win.refresh"));
        menu.append(Some("Dark mode"), Some("win.dark"));
        menu.append(Some("Keyboard shortcuts"), Some("win.shortcuts"));
        menu.append(Some("Sign out…"), Some("win.logout"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main menu")
            .build();
        vault_header.pack_end(&menu_button);
        let copy = icon_button("edit-copy-symbolic", "Copy password (Ctrl+Shift+C)");
        copy.set_action_name(Some("win.copy-password"));
        detail_header.pack_end(&copy);
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
            item_page,
            detail_page,
            sign_label,
            sign_button,
            cancel_button,
            items: RefCell::new(vec![]),
            current: RefCell::new(None),
            epoch: Cell::new(0),
            request: Cell::new(0),
            clipboard: RefCell::new(None),
            login: Rc::new(RefCell::new(None)),
        });
        s.actions(app);
        s.connections();
        match std::env::var("PPG_COLOR_SCHEME").as_deref() {
            Ok("dark") => {
                adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark)
            }
            Ok("light") => {
                adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight)
            }
            _ => (),
        }
        s.empty_detail();
        s.refresh();
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
    }
    fn empty_detail(&self) {
        self.clear_detail();
        self.detail_box.append(
            &adw::StatusPage::builder()
                .title("Choose an item")
                .description("Select an item to view its details.")
                .icon_name("dialog-password-symbolic")
                .build(),
        );
    }
    fn signed_out(&self) {
        self.sign_label
            .set_label("Sign in securely using Proton’s website.");
        self.sign_button.set_sensitive(true);
        self.cancel_button.set_visible(false);
        self.epoch.set(self.epoch.get() + 1);
        self.cancel_login();
        self.clear_clipboard();
        self.empty_detail();
        self.items.borrow_mut().clear();
        Self::clear_list(&self.vault_list);
        Self::clear_list(&self.item_list);
        self.stack.set_visible_child_name("signin");
    }
    fn refresh(self: &Rc<Self>) {
        let s = self.clone();
        glib::spawn_future_local(async move {
            match s.backend.info().await {
                Ok(account) => {
                    s.sign_label.set_label(&account);
                    match s.backend.vaults().await {
                        Ok(v) => s.show_vaults(v),
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
                    s.load_items(v.clone(), true);
                }
            });
        }
        if let Some(v) = vaults.first() {
            self.load_items(v.clone(), false);
        } else {
            self.notify("No vaults available");
        }
    }
    fn load_items(self: &Rc<Self>, v: Vault, navigate: bool) {
        self.empty_detail();
        self.search.set_text("");
        self.items.borrow_mut().clear();
        Self::clear_list(&self.item_list);
        self.item_page.set_title(&v.name);
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
            self.item_list
                .append(&row("No items found", "Try a different search"));
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
            let r = row(&caption, field.value.as_deref().unwrap_or("••••••••"));
            if field.kind == Kind::Totp {
                r.set_title("One-time code");
                let countdown = gtk::Label::new(None);
                countdown.add_css_class("dim-label");
                r.add_suffix(&countdown);
                let weak = countdown.downgrade();
                glib::timeout_add_local(Duration::from_secs(1), move || {
                    let Some(l) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    l.set_label(&format!("{} s", 30 - (glib::real_time() / 1_000_000) % 30));
                    glib::ControlFlow::Continue
                });
            }
            let b = icon_button("edit-copy-symbolic", &format!("Copy {}", field.label));
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
                    &format!("Reveal {} for 30 seconds", field.label),
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
        if d.attachments > 0 {
            self.detail_box.append(&gtk::Label::new(Some(&format!(
                "{} attachment(s) · download not supported",
                d.attachments
            ))));
        }
        *self.current.borrow_mut() = Some(d);
    }
    fn copy_field(self: &Rc<Self>, i: Item, f: Field) {
        let s = self.clone();
        let epoch = self.epoch.get();
        let request = self.request.get();
        glib::spawn_future_local(async move {
            let value = if let Some(v) = f.value {
                Ok(zeroize::Zeroizing::new(v))
            } else {
                s.backend.secret(&i, &f.key, f.kind == Kind::Totp).await
            };
            match value {
                Ok(value) if epoch == s.epoch.get() && request == s.request.get() => {
                    let provider = gdk::ContentProvider::for_value(&value.as_str().to_value());
                    let c = s.win.clipboard();
                    if c.set_content(Some(&provider)).is_ok() {
                        *s.clipboard.borrow_mut() = Some(provider.clone());
                        s.notify("Copied · clipboard clears in 30 seconds");
                        let weak = Rc::downgrade(&s);
                        glib::timeout_add_local_once(Duration::from_secs(30), move || {
                            if c.content().as_ref() == Some(&provider) {
                                let _ = c.set_content(gdk::ContentProvider::NONE);
                            }
                            if let Some(s) = weak.upgrade() {
                                if s.clipboard.borrow().as_ref() == Some(&provider) {
                                    s.clipboard.borrow_mut().take();
                                }
                            }
                        });
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
        if r.subtitle().as_deref() != Some("••••••••") {
            r.set_subtitle("••••••••");
            return;
        }
        let s = self.clone();
        let request = self.request.get();
        let weak = r.downgrade();
        glib::spawn_future_local(async move {
            match s.backend.secret(&i, &f.key, f.kind == Kind::Totp).await {
                Ok(v) if request == s.request.get() && token == generation.get() => {
                    if let Some(r) = weak.upgrade() {
                        r.set_subtitle(&v);
                        let weak = r.downgrade();
                        let gen_hide = generation.clone();
                        glib::timeout_add_local_once(Duration::from_secs(30), move || {
                            if token == gen_hide.get() {
                                gen_hide.set(token + 1);
                                if let Some(r) = weak.upgrade() {
                                    r.set_subtitle("••••••••");
                                }
                            }
                        });
                        if f.kind == Kind::Totp {
                            let weak = r.downgrade();
                            let weak_s = Rc::downgrade(&s);
                            let mut period = (glib::real_time() / 1_000_000) / 30;
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
                                let now = (glib::real_time() / 1_000_000) / 30;
                                if now != period {
                                    period = now;
                                    let i = i.clone();
                                    let f = f.clone();
                                    let g = generation.clone();
                                    let weak = r.downgrade();
                                    glib::spawn_future_local(async move {
                                        match s.backend.secret(&i, &f.key, true).await {
                                            Ok(v)
                                                if token == g.get()
                                                    && request == s.request.get() =>
                                            {
                                                if let Some(r) = weak.upgrade() {
                                                    r.set_subtitle(&v);
                                                }
                                            }
                                            _ => {
                                                if let Some(r) = weak.upgrade() {
                                                    r.set_subtitle("••••••••");
                                                }
                                            }
                                        }
                                    });
                                }
                                glib::ControlFlow::Continue
                            });
                        }
                    }
                }
                Ok(_) => (),
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
        self.win.connect_close_request(move |_| {
            if let Some(s) = s.upgrade() {
                s.cancel_login();
                s.clear_clipboard();
                s.clear_detail();
            }
            glib::Propagation::Proceed
        });
    }
    fn start_login(self: &Rc<Self>) {
        self.sign_button.set_sensitive(false);
        self.cancel_button.set_visible(true);
        self.sign_label
            .set_label("Complete sign-in in your browser…");
        let s = self.clone();
        glib::spawn_future_local(async move {
            let parent = s.win.clone();
            let result = s
                .backend
                .login(s.login.clone(), move |url| {
                    // Launch via the desktop portal inside Flatpak; do not log the URL.
                    gtk::UriLauncher::new(&url).launch(
                        Some(&parent),
                        gio::Cancellable::NONE,
                        |_result| {},
                    );
                })
                .await;
            s.sign_button.set_sensitive(true);
            s.cancel_button.set_visible(false);
            match result {
                Ok(()) => s.refresh(),
                Err(e) => {
                    s.sign_label.set_label(e);
                    s.notify(e);
                }
            }
        });
    }
    fn actions(self: &Rc<Self>, app: &adw::Application) {
        for (name, accels) in [
            ("search", vec!["<Control>f"]),
            ("refresh", vec!["<Control>r"]),
            ("close", vec!["<Control>w"]),
            ("quit", vec!["<Control>q"]),
            ("shortcuts", vec!["<Control>question"]),
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
        let a = gio::SimpleAction::new_stateful("dark", None, &false.to_variant());
        a.connect_activate(|a, _| {
            let dark = !a.state().and_then(|v| v.get::<bool>()).unwrap_or(false);
            a.set_state(&dark.to_variant());
            adw::StyleManager::default().set_color_scheme(if dark {
                adw::ColorScheme::ForceDark
            } else {
                adw::ColorScheme::ForceLight
            });
        });
        self.win.add_action(&a);
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
        let d=adw::AlertDialog::builder().heading("Keyboard shortcuts").body("Ctrl+F   Search\nCtrl+R   Refresh\nCtrl+W / Ctrl+Q   Close / Quit\nCtrl+?   Keyboard shortcuts\nCtrl+Shift+U   Copy username\nCtrl+Shift+C   Copy password\nCtrl+Shift+T   Copy one-time code\n\nType to search in the current vault.\nSecrets hide and clipboard clears after 30 seconds.").build();
        d.add_response("close", "Close");
        d.present(Some(&self.win));
    }
    /// Deterministic, mock-only screenshot harness: renders GTK, not a mockup.
    fn capture_setup(self: &Rc<Self>, w: i32, h: i32) {
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_enable_animations(false);
        }
        let s = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            let state = std::env::var("PPG_CAPTURE_STATE").unwrap_or_else(|_| "list".into());
            if state == "signin" {
                s.signed_out();
            } else if state == "vaults" {
                s.outer.set_show_content(false);
            } else {
                s.outer.set_show_content(true);
                s.inner.set_show_content(false);
                if state == "search" {
                    s.focus_search();
                    s.search.set_text("example");
                }
                if state == "detail" || state == "copied-toast" || w > 860 {
                    let i = s.items.borrow().iter().find(|i| i.kind == "login").cloned();
                    if let Some(i) = i {
                        s.open(i);
                    }
                }
            }
            let ss = s.clone();
            glib::timeout_add_local_once(Duration::from_millis(800), move || {
                if state == "copied-toast" {
                    let d = ss.current.borrow().clone();
                    if let Some(d) = d {
                        if let Some(f) = d.fields.iter().find(|f| f.key == "password") {
                            ss.copy_field(d.item.clone(), f.clone());
                        }
                    }
                }
                ss.capture_resize(w, h, 0);
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
            s.win.close();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn password_row(w: &gtk::Widget) -> Option<adw::ActionRow> {
        if let Some(r) = w.downcast_ref::<adw::ActionRow>() {
            if r.title() == "password" {
                return Some(r.clone());
            }
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            if let Some(r) = password_row(&child) {
                return Some(r);
            }
            c = child.next_sibling();
        }
        None
    }
    #[test]
    #[ignore = "requires an isolated display and PPG_BACKEND=mock; runs 34 seconds"]
    fn adaptive_navigation_and_secret_expiry() {
        assert_eq!(std::env::var("PPG_BACKEND").as_deref(), Ok("mock"));
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.isyourbrainfoss.ProtonPassGtk.Test")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let s = Window::new(&app);
        glib::MainContext::default().block_on(async {
            glib::timeout_future(Duration::from_millis(500)).await;
            assert!(s.outer.is_collapsed());
            assert!(s.inner.is_collapsed());
            s.win.set_default_size(800, 720);
            glib::timeout_future(Duration::from_millis(250)).await;
            assert!(s.outer.is_collapsed());
            assert!(!s.inner.is_collapsed());
            s.win.set_default_size(1280, 800);
            glib::timeout_future(Duration::from_millis(250)).await;
            assert!(!s.outer.is_collapsed());
            assert!(!s.inner.is_collapsed());
            s.win.set_default_size(360, 720);
            glib::timeout_future(Duration::from_millis(250)).await;
            s.focus_search();
            s.search.set_text("example");
            s.filter();
            assert!(s.item_list.row_at_index(0).is_some());
            assert!(s.item_list.row_at_index(1).is_none());
            let i = s
                .items
                .borrow()
                .iter()
                .find(|i| i.kind == "login")
                .unwrap()
                .clone();
            s.open(i);
            glib::timeout_future(Duration::from_millis(100)).await;
            let d = s.current.borrow().clone().unwrap();
            let f = d
                .fields
                .iter()
                .find(|f| f.key == "password")
                .unwrap()
                .clone();
            assert_eq!(f.value, None);
            let r = password_row(s.detail_box.upcast_ref()).unwrap();
            s.copy_field(d.item.clone(), f.clone());
            glib::timeout_future(Duration::from_millis(100)).await;
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
            s.reveal(d.item, f, r.clone(), Rc::new(Cell::new(0)));
            glib::timeout_future(Duration::from_millis(100)).await;
            assert_ne!(r.subtitle().as_deref(), Some("••••••••"));
            glib::timeout_future(Duration::from_secs(31)).await;
            assert_eq!(r.subtitle().as_deref(), Some("••••••••"));
            assert!(s.win.clipboard().content().is_none());
            s.backend.logout().await.unwrap();
            s.signed_out();
            assert_eq!(s.stack.visible_child_name().as_deref(), Some("signin"));
            assert!(s.current.borrow().is_none());
            assert!(s.items.borrow().is_empty());
            s.win.close();
        });
    }
}
