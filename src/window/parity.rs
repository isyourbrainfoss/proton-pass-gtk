// SPDX-License-Identifier: GPL-3.0-or-later
//! Feature-parity UI: password generator, all item types, trash, vault
//! management, sharing, invitations, moving items, export and the search
//! index. Every write goes through `Backend`, which keeps real-account
//! changes behind `PPG_ENABLE_REAL_EDITS=1`.
use super::*;
use proton_pass_gtk::{
    generator::{self, Strength},
    model::{Invite, ItemDraft},
};
use zeroize::Zeroize;

/// Late-bound reload callback for list dialogs (dropped when they close).
type Reload = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
/// Session-only generator history length (cleared on lock and sign-out).
const HISTORY: usize = 5;

pub(super) fn sheet(
    title: &str,
    content: &impl IsA<gtk::Widget>,
    width: i32,
    height: i32,
) -> (adw::Dialog, adw::HeaderBar, adw::ToastOverlay) {
    let header = adw::HeaderBar::new();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    let clamp = adw::Clamp::builder()
        .maximum_size(560)
        .child(content)
        .build();
    view.set_content(Some(&scroll(&clamp)));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));
    let d = adw::Dialog::builder()
        .title(title)
        .content_width(width)
        .content_height(height)
        .width_request(300)
        .child(&toasts)
        .build();
    (d, header, toasts)
}
fn page_box() -> gtk::Box {
    let b = boxed();
    b.set_spacing(18);
    b.set_margin_top(12);
    b.set_margin_bottom(24);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b
}
fn boxed_list() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.add_css_class("boxed-list");
    l.set_selection_mode(gtk::SelectionMode::None);
    l
}
fn role_model() -> gtk::StringList {
    gtk::StringList::new(&model::ROLES.map(|(_, l)| l))
}

impl Window {
    // ---------- Password generator ----------

    /// `target`: editor field that “Use Password” fills in.
    pub(super) fn generator_dialog(self: &Rc<Self>, target: Option<adw::PasswordEntryRow>) {
        let p = self.prefs.borrow().clone();
        let content = page_box();
        let value = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Char)
            .selectable(false)
            .justify(gtk::Justification::Center)
            .css_classes(["title-2", "monospace"])
            .margin_top(12)
            .build();
        value.update_property(&[gtk::accessible::Property::Label("Generated password")]);
        let strength = gtk::Label::builder()
            .css_classes(["caption-heading"])
            .build();
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        actions.set_halign(gtk::Align::Center);
        let regenerate = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Generate another (Ctrl+R)")
            .css_classes(["circular"])
            .build();
        regenerate.update_property(&[gtk::accessible::Property::Label("Generate another")]);
        let copy = gtk::Button::builder()
            .label("Copy")
            .css_classes(["pill", "suggested-action"])
            .build();
        actions.append(&regenerate);
        actions.append(&copy);
        let use_button = gtk::Button::builder()
            .label("Use Password")
            .css_classes(["pill"])
            .visible(target.is_some())
            .build();
        actions.append(&use_button);
        content.append(&value);
        content.append(&strength);
        content.append(&actions);

        let options = adw::PreferencesGroup::builder().title("Options").build();
        let mode = adw::ComboRow::builder()
            .title("Type")
            .model(&gtk::StringList::new(&["Random password", "Passphrase"]))
            .selected(u32::from(p.gen_passphrase))
            .build();
        let length = adw::SpinRow::with_range(
            f64::from(generator::MIN_LENGTH),
            f64::from(generator::MAX_LENGTH),
            1.0,
        );
        length.set_title("Length");
        length.set_value(f64::from(p.gen_length));
        let upper = adw::SwitchRow::builder()
            .title("Capital letters")
            .active(p.gen_uppercase)
            .build();
        let numbers = adw::SwitchRow::builder()
            .title("Numbers")
            .active(p.gen_numbers)
            .build();
        let symbols = adw::SwitchRow::builder()
            .title("Symbols")
            .active(p.gen_symbols)
            .build();
        let ambiguous = adw::SwitchRow::builder()
            .title("Avoid similar characters")
            .subtitle("No I, l, 1, O, 0 or o")
            .active(p.gen_avoid_ambiguous)
            .build();
        let words = adw::SpinRow::with_range(
            f64::from(generator::MIN_WORDS),
            f64::from(generator::MAX_WORDS),
            1.0,
        );
        words.set_title("Words");
        words.set_value(f64::from(p.gen_words));
        let separator = adw::ComboRow::builder()
            .title("Separator")
            .model(&gtk::StringList::new(
                &generator::SEPARATORS.map(|(_, l)| l),
            ))
            .selected(
                generator::SEPARATORS
                    .iter()
                    .position(|(k, _)| *k == p.gen_separator)
                    .unwrap_or(0) as u32,
            )
            .build();
        let capitalise = adw::SwitchRow::builder()
            .title("Capitalize words")
            .active(p.gen_capitalise)
            .build();
        let word_numbers = adw::SwitchRow::builder()
            .title("Include numbers")
            .active(p.gen_word_numbers)
            .build();
        options.add(&mode);
        for r in [
            &length.clone().upcast::<gtk::Widget>(),
            upper.upcast_ref(),
            numbers.upcast_ref(),
            symbols.upcast_ref(),
            ambiguous.upcast_ref(),
            words.upcast_ref(),
            separator.upcast_ref(),
            capitalise.upcast_ref(),
            word_numbers.upcast_ref(),
        ] {
            options.add(r);
        }
        content.append(&options);
        let history = adw::PreferencesGroup::builder()
            .title("Recent")
            .description("This session only. Cleared when the app locks or closes.")
            .build();
        content.append(&history);
        if self.backend.mock {
            let note = gtk::Label::builder()
                .label("Demo backend: random passwords use the system random generator; passphrases use a tiny fake word list.")
                .wrap(true)
                .css_classes(["dim-label", "caption"])
                .build();
            content.append(&note);
        }

        let (d, _header, toasts) = sheet("Password Generator", &content, 460, 680);
        let current: Rc<RefCell<Zeroizing<String>>> =
            Rc::new(RefCell::new(Zeroizing::new(String::new())));
        let history_rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();
        let render_history = {
            let s = Rc::downgrade(self);
            let history = history.clone();
            let rows = history_rows.clone();
            move || {
                let Some(s) = s.upgrade() else { return };
                for r in rows.borrow_mut().drain(..) {
                    history.remove(&r);
                }
                let h = s.gen_history.borrow();
                history.set_visible(!h.is_empty());
                for (n, v) in h.iter().enumerate() {
                    let masked: String = v.chars().take(3).chain("•••••".chars()).collect();
                    let r = row(&masked, &format!("{} characters", v.chars().count()));
                    let b = icon_button("edit-copy-symbolic", "Copy this earlier password");
                    b.set_valign(gtk::Align::Center);
                    let weak = Rc::downgrade(&s);
                    b.connect_clicked(move |_| {
                        if let Some(s) = weak.upgrade() {
                            let v = s.gen_history.borrow().get(n).cloned();
                            if let Some(v) = v {
                                s.put_clipboard(&v);
                            }
                        }
                    });
                    r.add_suffix(&b);
                    history.add(&r);
                    rows.borrow_mut().push(r);
                }
            }
        };
        render_history();
        let sync_visibility = {
            let (
                length,
                upper,
                numbers,
                symbols,
                ambiguous,
                words,
                separator,
                capitalise,
                word_numbers,
            ) = (
                length.clone(),
                upper.clone(),
                numbers.clone(),
                symbols.clone(),
                ambiguous.clone(),
                words.clone(),
                separator.clone(),
                capitalise.clone(),
                word_numbers.clone(),
            );
            move |phrase: bool| {
                for w in [
                    length.upcast_ref::<gtk::Widget>(),
                    upper.upcast_ref(),
                    numbers.upcast_ref(),
                    symbols.upcast_ref(),
                    ambiguous.upcast_ref(),
                ] {
                    w.set_visible(!phrase);
                }
                for w in [
                    words.upcast_ref::<gtk::Widget>(),
                    separator.upcast_ref(),
                    capitalise.upcast_ref(),
                    word_numbers.upcast_ref(),
                ] {
                    w.set_visible(phrase);
                }
            }
        };
        sync_visibility(p.gen_passphrase);
        let generate: Rc<dyn Fn()> = {
            let s = Rc::downgrade(self);
            let (value, strength, current, toasts) = (
                value.clone(),
                strength.clone(),
                current.clone(),
                toasts.clone(),
            );
            let busy = Rc::new(Cell::new(0u64));
            Rc::new(move || {
                let Some(s) = s.upgrade() else { return };
                let p = s.prefs.borrow().clone();
                let token = busy.get() + 1;
                busy.set(token);
                let (bits, label_strength) = if p.gen_passphrase {
                    let b = generator::passphrase_entropy_bits(&p.passphrase_options());
                    (
                        b,
                        Strength::from_bits(if s.backend.mock { b.min(40.0) } else { b }),
                    )
                } else {
                    let b = generator::random_entropy_bits(&p.random_options());
                    (b, Strength::from_bits(b))
                };
                for c in ["error", "warning", "success"] {
                    strength.remove_css_class(c);
                }
                strength.add_css_class(label_strength.css_class());
                strength.set_label(&format!(
                    "{} · about {:.0} bits",
                    label_strength.label(),
                    bits
                ));
                let (value, current, toasts, busy) =
                    (value.clone(), current.clone(), toasts.clone(), busy.clone());
                glib::spawn_future_local(async move {
                    let result = if p.gen_passphrase {
                        s.backend.passphrase(&p.passphrase_options()).await
                    } else {
                        generator::random(&p.random_options())
                    };
                    if busy.get() != token {
                        return;
                    }
                    match result {
                        Ok(v) => {
                            value.set_label(&v);
                            *current.borrow_mut() = v;
                        }
                        Err(e) => toasts.add_toast(adw::Toast::new(e)),
                    }
                });
            })
        };
        // Option changes are saved as defaults and produce a new value.
        macro_rules! on_change {
            ($w:expr, $connect:ident, $get:expr, $set:expr) => {{
                let s = Rc::downgrade(self);
                let g = generate.clone();
                $w.$connect(move |w| {
                    if let Some(s) = s.upgrade() {
                        let v = $get(w);
                        s.update_prefs(|p| $set(p, v));
                        g();
                    }
                });
            }};
        }
        {
            let s = Rc::downgrade(self);
            let g = generate.clone();
            let sync = sync_visibility.clone();
            mode.connect_selected_notify(move |w| {
                if let Some(s) = s.upgrade() {
                    let phrase = w.selected() == 1;
                    s.update_prefs(|p| p.gen_passphrase = phrase);
                    sync(phrase);
                    g();
                }
            });
        }
        on_change!(
            length,
            connect_value_notify,
            |w: &adw::SpinRow| w.value() as u32,
            |p: &mut Prefs, v| p.gen_length = v
        );
        on_change!(
            words,
            connect_value_notify,
            |w: &adw::SpinRow| w.value() as u32,
            |p: &mut Prefs, v| p.gen_words = v
        );
        on_change!(
            upper,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_uppercase = v
        );
        on_change!(
            numbers,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_numbers = v
        );
        on_change!(
            symbols,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_symbols = v
        );
        on_change!(
            ambiguous,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_avoid_ambiguous = v
        );
        on_change!(
            capitalise,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_capitalise = v
        );
        on_change!(
            word_numbers,
            connect_active_notify,
            |w: &adw::SwitchRow| w.is_active(),
            |p: &mut Prefs, v| p.gen_word_numbers = v
        );
        on_change!(
            separator,
            connect_selected_notify,
            |w: &adw::ComboRow| w.selected() as usize,
            |p: &mut Prefs, v: usize| {
                if let Some((k, _)) = generator::SEPARATORS.get(v) {
                    p.gen_separator = (*k).into();
                }
            }
        );
        let remember = {
            let s = Rc::downgrade(self);
            let current = current.clone();
            let render = render_history.clone();
            move || {
                let Some(s) = s.upgrade() else { return };
                let v = current.borrow().clone();
                if v.is_empty() {
                    return;
                }
                let mut h = s.gen_history.borrow_mut();
                h.retain(|x| x.as_str() != v.as_str());
                h.insert(0, v);
                h.truncate(HISTORY);
                drop(h);
                render();
            }
        };
        {
            let g = generate.clone();
            regenerate.connect_clicked(move |_| g());
        }
        {
            let s = Rc::downgrade(self);
            let current = current.clone();
            let remember = remember.clone();
            copy.connect_clicked(move |_| {
                if let Some(s) = s.upgrade() {
                    let v = current.borrow().clone();
                    if !v.is_empty() {
                        s.put_clipboard(&v);
                        remember();
                    }
                }
            });
        }
        {
            let weak = d.downgrade();
            let current = current.clone();
            use_button.connect_clicked(move |_| {
                if let Some(t) = &target {
                    t.set_text(&current.borrow());
                }
                remember();
                if let Some(d) = weak.upgrade() {
                    d.close();
                }
            });
        }
        let keys = gtk::ShortcutController::new();
        let g = generate.clone();
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>r"),
            Some(gtk::CallbackAction::new(move |_, _| {
                g();
                glib::Propagation::Stop
            })),
        ));
        d.add_controller(keys);
        d.connect_closed(move |_| {
            current.borrow_mut().zeroize();
            value.set_label("");
        });
        generate();
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }

    // ---------- Create / edit any item type ----------

    pub(super) fn new_item_menu() -> gio::Menu {
        let m = gio::Menu::new();
        for kind in model::CREATABLE {
            let label = match kind {
                "alias" => "Hide-my-email Alias",
                k => model::kind_label(k),
            };
            m.append(Some(label), Some(&format!("win.new-item::{kind}")));
        }
        m
    }

    pub(super) fn item_editor(self: &Rc<Self>, kind: &str, existing: Option<Detail>) {
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let Some(vault) = self.vault.borrow().clone() else {
            return self.notify("Choose a vault first");
        };
        let creating = existing.is_none();
        let kind = existing
            .as_ref()
            .map_or(kind.to_owned(), |d| d.item.kind.clone());
        let old = existing
            .as_ref()
            .map(model::item_draft_from)
            .unwrap_or_else(|| ItemDraft {
                kind: kind.clone(),
                ..Default::default()
            });
        let content = page_box();
        let fields = adw::PreferencesGroup::new();
        if self.backend.mock {
            fields.set_description(Some(
                "Demo backend: changes stay in memory and are discarded on exit. Do not enter real secrets.",
            ));
        } else if !creating {
            fields.set_description(Some(
                "Hidden values cannot be changed here: pass-cli only accepts new values on its command line, where other programs could read them.",
            ));
        }
        let title = entry_row(
            if kind == "alias" && creating {
                "Alias prefix"
            } else {
                "Title"
            },
            &old.title,
        );
        fields.add(&title);
        let mut inputs: Vec<(&'static str, gtk::Widget)> = vec![];
        let mut key_type = None;
        match (kind.as_str(), creating) {
            ("alias", true) => {
                title.set_input_purpose(gtk::InputPurpose::Email);
                let hint = row(
                    "How aliases work",
                    "Proton creates prefix.random@domain and forwards mail to your mailbox. The suffix and mailbox are chosen by pass-cli.",
                );
                fields.add(&hint);
            }
            ("sshkey", true) => {
                let kt = adw::ComboRow::builder()
                    .title("Key type")
                    .subtitle(
                        "Generated by pass-cli; the private key never passes through this app",
                    )
                    .model(&gtk::StringList::new(&model::SSH_KEY_TYPES))
                    .build();
                fields.add(&kt);
                key_type = Some(kt);
            }
            _ => (),
        }
        for spec in model::field_specs(&kind) {
            let w: gtk::Widget = if spec.hidden {
                let p = adw::PasswordEntryRow::builder()
                    .title(if creating {
                        spec.label.to_owned()
                    } else {
                        format!("{} (empty keeps current)", spec.label)
                    })
                    .build();
                // Real updates would put the value in argv; never offer it.
                p.set_visible(creating || self.backend.mock);
                if spec.key == "password" {
                    let b = icon_button("dialog-password-symbolic", "Generate password (Ctrl+G)");
                    b.set_valign(gtk::Align::Center);
                    b.add_css_class("flat");
                    let s = Rc::downgrade(self);
                    let target = p.clone();
                    b.connect_clicked(move |_| {
                        if let Some(s) = s.upgrade() {
                            s.generator_dialog(Some(target.clone()));
                        }
                    });
                    p.add_suffix(&b);
                }
                p.upcast()
            } else {
                let e = entry_row(spec.label, old.get(spec.key));
                if spec.key.contains("email") {
                    e.set_input_purpose(gtk::InputPurpose::Email);
                } else if spec.key.contains("phone") {
                    e.set_input_purpose(gtk::InputPurpose::Phone);
                }
                e.upcast()
            };
            fields.add(&w);
            inputs.push((spec.key, w));
        }
        let url = entry_row("Website", &old.url);
        url.set_input_purpose(gtk::InputPurpose::Url);
        url.set_visible(kind == "login");
        fields.add(&url);
        content.append(&fields);
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
        note_group.set_visible(!(kind == "alias" && creating));
        content.append(&note_group);
        let heading = format!(
            "{} {}",
            if creating { "New" } else { "Edit" },
            if kind == "alias" {
                "Alias"
            } else {
                model::kind_label(&kind)
            }
        );
        let (d, header, toasts) = sheet(&heading, &content, 480, 640);
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label(if creating { "Create" } else { "Save" });
        save.add_css_class("suggested-action");
        header.pack_start(&cancel);
        header.pack_end(&save);
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
        let inputs = Rc::new(inputs);
        let clear_inputs = {
            let inputs = inputs.clone();
            move || {
                for (_, w) in inputs.iter() {
                    if let Some(p) = w.downcast_ref::<adw::PasswordEntryRow>() {
                        p.set_text("");
                    }
                }
            }
        };
        let clear = clear_inputs.clone();
        d.connect_closed(move |_| clear());
        save.connect_clicked(move |save| {
            let (Some(s), Some(d)) = (s.upgrade(), weak.upgrade()) else {
                return;
            };
            let buf = note.buffer();
            let mut draft = ItemDraft {
                kind: kind.clone(),
                title: title.text().into(),
                note: buf.text(&buf.start_iter(), &buf.end_iter(), false).into(),
                url: url.text().into(),
                values: vec![],
                extra: String::new(),
            };
            if kind == "alias" && item.is_none() {
                draft.extra = draft.title.trim().to_owned();
            }
            if let Some(kt) = &key_type {
                draft.extra = model::SSH_KEY_TYPES
                    .get(kt.selected() as usize)
                    .copied()
                    .unwrap_or("ed25519")
                    .into();
            }
            for (key, w) in inputs.iter() {
                let text = if let Some(p) = w.downcast_ref::<adw::PasswordEntryRow>() {
                    Zeroizing::new(p.text().to_string())
                } else if let Some(e) = w.downcast_ref::<adw::EntryRow>() {
                    Zeroizing::new(e.text().to_string())
                } else {
                    continue;
                };
                if !text.is_empty() {
                    draft.values.push(((*key).to_owned(), text));
                }
            }
            if let Err(e) = draft.validate() {
                toasts.add_toast(adw::Toast::new(e));
                return;
            }
            save.set_sensitive(false);
            let (old, item, vault, toasts, save, clear) = (
                old.clone(),
                item.clone(),
                vault.clone(),
                toasts.clone(),
                save.clone(),
                clear_inputs.clone(),
            );
            glib::spawn_future_local(async move {
                let result = match &item {
                    None => s.backend.create_item(&vault.share_id, &draft).await,
                    Some(i) => s.backend.update_item(i, &old, &draft).await,
                };
                let label = if draft.kind == "alias" {
                    "Alias"
                } else {
                    model::kind_label(&draft.kind)
                };
                drop(draft);
                match result {
                    Ok(saved) => {
                        clear();
                        d.close();
                        s.notify(&if item.is_none() {
                            format!("{label} created")
                        } else {
                            "Changes saved".into()
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

    // ---------- Search index ----------

    pub(super) fn index_detail(&self, d: &Detail) {
        self.index.borrow_mut().insert(
            index_key(&d.item),
            (model::search_text(d), model::subtitle_of(d)),
        );
    }
    /// Load item details one at a time (never `--show-secrets`) to search
    /// usernames, websites, notes and aliases. Hidden values are discarded
    /// at the parsing boundary. Demo backend: always on (in memory).
    pub(super) fn build_index(self: &Rc<Self>, epoch: u64) {
        if !self.backend.mock && !self.prefs.borrow().deep_search {
            return;
        }
        let s = self.clone();
        glib::spawn_future_local(async move {
            let items = s.items.borrow().clone();
            let mut since_refresh = 0;
            for i in items {
                if s.epoch.get() != epoch {
                    return;
                }
                if s.index.borrow().contains_key(&index_key(&i)) {
                    continue;
                }
                if let Ok(d) = s.backend.detail(&i).await {
                    if s.epoch.get() != epoch {
                        return;
                    }
                    s.index_detail(&d);
                    since_refresh += 1;
                }
                if since_refresh >= 25 {
                    since_refresh = 0;
                    s.refilter_keep_selection();
                }
            }
            if s.epoch.get() == epoch {
                s.refilter_keep_selection();
            }
        });
    }
    fn refilter_keep_selection(self: &Rc<Self>) {
        let selected = self.item_list.selected_row().map(|r| r.index());
        self.filter();
        if let Some(r) = selected.and_then(|i| self.item_list.row_at_index(i)) {
            self.item_list.select_row(Some(&r));
        }
    }

    // ---------- Trash ----------

    pub(super) fn trash_dialog(self: &Rc<Self>) {
        let content = page_box();
        let group = adw::PreferencesGroup::builder()
            .title("Trash")
            .description(if self.backend.mock {
                "Demo backend: items you move to the trash appear here."
            } else {
                "Items in the trash of all your vaults. Restore them or delete them permanently."
            })
            .build();
        content.append(&group);
        let (d, _, toasts) = sheet("Trash", &content, 480, 560);
        let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::default();
        let reload: Reload = Rc::default();
        let f: Rc<dyn Fn()> = {
            let s = Rc::downgrade(self);
            let (group, toasts, rows, reload) =
                (group.clone(), toasts.clone(), rows.clone(), reload.clone());
            Rc::new(move || {
                let Some(s) = s.upgrade() else { return };
                let (group, toasts, rows, reload) =
                    (group.clone(), toasts.clone(), rows.clone(), reload.clone());
                glib::spawn_future_local(async move {
                    let vaults = s.vaults.borrow().clone();
                    let result = s.backend.trashed(&vaults).await;
                    for r in rows.borrow_mut().drain(..) {
                        group.remove(&r);
                    }
                    let items = match result {
                        Ok(i) => i,
                        Err(e) => return toasts.add_toast(adw::Toast::new(e)),
                    };
                    if items.is_empty() {
                        let empty = adw::StatusPage::builder()
                            .icon_name("user-trash-symbolic")
                            .title("Trash is empty")
                            .description("Items you move to the trash appear here.")
                            .build();
                        empty.add_css_class("compact");
                        group.add(&empty);
                        rows.borrow_mut().push(empty.upcast());
                        return;
                    }
                    for i in items {
                        let vault = vaults
                            .iter()
                            .find(|v| v.share_id == i.share_id)
                            .map_or("", |v| v.name.as_str());
                        let r = row(
                            &i.title,
                            &format!("{} · {vault}", model::kind_label(&i.kind)),
                        );
                        r.add_prefix(&gtk::Image::from_icon_name(model::kind_icon(&i.kind)));
                        let restore =
                            icon_button("edit-undo-symbolic", &format!("Restore {}", i.title));
                        restore.set_valign(gtk::Align::Center);
                        restore.add_css_class("flat");
                        let delete = icon_button(
                            "edit-delete-symbolic",
                            &format!("Delete {} permanently", i.title),
                        );
                        delete.set_valign(gtk::Align::Center);
                        delete.add_css_class("flat");
                        delete.add_css_class("error");
                        r.add_suffix(&restore);
                        r.add_suffix(&delete);
                        let editable = s.backend.can_edit();
                        restore.set_sensitive(editable);
                        delete.set_sensitive(editable);
                        {
                            let (s, i, toasts, reload) =
                                (s.clone(), i.clone(), toasts.clone(), reload.clone());
                            restore.connect_clicked(move |_| {
                                let (s, i, toasts, reload) =
                                    (s.clone(), i.clone(), toasts.clone(), reload.clone());
                                glib::spawn_future_local(async move {
                                    match s.backend.untrash(&i).await {
                                        Ok(()) => {
                                            toasts.add_toast(adw::Toast::new(&format!(
                                                "Restored “{}”",
                                                i.title
                                            )));
                                            s.after_vault_change(None);
                                            if let Some(f) = reload.borrow().clone() {
                                                f();
                                            }
                                        }
                                        Err(e) => toasts.add_toast(adw::Toast::new(e)),
                                    }
                                });
                            });
                        }
                        {
                            let (s, i, toasts, reload) =
                                (s.clone(), i.clone(), toasts.clone(), reload.clone());
                            delete.connect_clicked(move |_| {
                                let (s, i, toasts, reload) =
                                    (s.clone(), i.clone(), toasts.clone(), reload.clone());
                                glib::spawn_future_local(async move {
                                    let a = adw::AlertDialog::builder()
                                        .heading("Delete Permanently?")
                                        .body(format!(
                                            "“{}” will be deleted forever. This cannot be undone.",
                                            i.title
                                        ))
                                        .build();
                                    a.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
                                    a.set_response_appearance(
                                        "delete",
                                        adw::ResponseAppearance::Destructive,
                                    );
                                    a.set_default_response(Some("cancel"));
                                    a.set_close_response("cancel");
                                    s.track_dialog(&a);
                                    if a.choose_future(Some(&s.win)).await != "delete" {
                                        return;
                                    }
                                    match s.backend.delete_forever(&i).await {
                                        Ok(()) => {
                                            toasts.add_toast(adw::Toast::new(&format!(
                                                "Deleted “{}”",
                                                i.title
                                            )));
                                            if let Some(f) = reload.borrow().clone() {
                                                f();
                                            }
                                        }
                                        Err(e) => toasts.add_toast(adw::Toast::new(e)),
                                    }
                                });
                            });
                        }
                        group.add(&r);
                        rows.borrow_mut().push(r.upcast());
                    }
                });
            })
        };
        *reload.borrow_mut() = Some(f.clone());
        // Break the reference cycle when the dialog closes.
        d.connect_closed(move |_| {
            reload.borrow_mut().take();
        });
        f();
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }

    /// Reload vaults and the current vault after a change elsewhere.
    pub(super) fn after_vault_change(self: &Rc<Self>, select: Option<Vault>) {
        if let Some(v) = select {
            self.update_prefs(|p| p.last_vault = v.share_id.clone());
        }
        self.refresh();
    }

    // ---------- Vaults ----------

    pub(super) fn vault_name_dialog(self: &Rc<Self>, rename: Option<Vault>) {
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let d = adw::AlertDialog::builder()
            .heading(if rename.is_some() {
                "Rename Vault"
            } else {
                "New Vault"
            })
            .body(if rename.is_some() {
                "Choose a new name. Members of shared vaults see the new name too."
            } else {
                "Vaults keep items separate and can be shared with other people."
            })
            .build();
        d.add_responses(&[
            ("cancel", "Cancel"),
            ("ok", if rename.is_some() { "Rename" } else { "Create" }),
        ]);
        d.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("ok"));
        d.set_close_response("cancel");
        let entry = entry_row("Name", rename.as_ref().map_or("", |v| v.name.as_str()));
        entry.set_activates_default(true);
        let list = boxed_list();
        list.append(&entry);
        d.set_extra_child(Some(&list));
        d.set_response_enabled("ok", rename.is_some());
        let weak = d.downgrade();
        entry.connect_changed(move |e| {
            if let Some(d) = weak.upgrade() {
                d.set_response_enabled("ok", !e.text().trim().is_empty());
            }
        });
        d.set_focus(Some(&entry));
        self.track_dialog(&d);
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await != "ok" {
                return;
            }
            let name = entry.text().to_string();
            let result = match &rename {
                Some(v) => s.backend.rename_vault(v, &name).await,
                None => s.backend.create_vault(&name).await,
            };
            match result {
                Ok(v) => {
                    s.notify(&if rename.is_some() {
                        format!("Renamed to “{}”", v.name)
                    } else {
                        format!("Created “{}”", v.name)
                    });
                    s.after_vault_change(Some(v));
                }
                Err(e) => s.notify(e),
            }
        });
    }
    pub(super) fn confirm_vault_delete(self: &Rc<Self>) {
        let Some(v) = self.vault.borrow().clone() else {
            return;
        };
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let d = adw::AlertDialog::builder()
            .heading("Delete Vault?")
            .body(format!(
                "“{}” and every item in it will be deleted permanently, also for anyone it is shared with. Type the vault name to confirm.",
                v.name
            ))
            .build();
        d.add_responses(&[("cancel", "Cancel"), ("delete", "Delete Vault")]);
        d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        d.set_response_enabled("delete", false);
        let entry = entry_row("Vault name", "");
        let list = boxed_list();
        list.append(&entry);
        d.set_extra_child(Some(&list));
        let weak = d.downgrade();
        let want = v.name.clone();
        entry.connect_changed(move |e| {
            if let Some(d) = weak.upgrade() {
                d.set_response_enabled("delete", e.text() == want);
            }
        });
        self.track_dialog(&d);
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await != "delete" {
                return;
            }
            match s.backend.delete_vault(&v).await {
                Ok(()) => {
                    s.notify(&format!("Deleted “{}”", v.name));
                    s.update_prefs(|p| p.last_vault.clear());
                    s.after_vault_change(None);
                }
                Err(e) => s.notify(e),
            }
        });
    }

    // ---------- Sharing ----------

    /// Members of the current vault, or of `item` when given.
    pub(super) fn members_dialog(self: &Rc<Self>, item: Option<Item>) {
        let Some(vault) = self.vault.borrow().clone() else {
            return;
        };
        let share = item
            .as_ref()
            .map_or(vault.share_id.clone(), |i| i.share_id.clone());
        let content = page_box();
        let invite = adw::PreferencesGroup::builder()
            .title("Invite")
            .description(if item.is_some() {
                "Share only this item. The person needs a Proton account."
            } else {
                "Share the whole vault. The person needs a Proton account."
            })
            .build();
        let email = entry_row("Email address", "");
        email.set_input_purpose(gtk::InputPurpose::Email);
        let role = adw::ComboRow::builder()
            .title("Role")
            .model(&role_model())
            .build();
        let send = gtk::Button::builder()
            .label("Send Invitation")
            .css_classes(["pill", "suggested-action"])
            .halign(gtk::Align::Center)
            .margin_top(12)
            .sensitive(false)
            .build();
        invite.add(&email);
        invite.add(&role);
        let send_box = boxed();
        send_box.append(&send);
        invite.add(&send_box);
        let members = adw::PreferencesGroup::builder()
            .title("People with access")
            .build();
        content.append(&invite);
        content.append(&members);
        let help = gtk::Label::builder()
            .label("Viewers can see items. Editors can also change them. Managers can also invite people. Public Secure Links are not available in pass-cli.")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        content.append(&help);
        let title = match &item {
            Some(i) => format!("Share “{}”", i.title),
            None => format!("Share “{}”", vault.name),
        };
        let (d, _, toasts) = sheet(&title, &content, 480, 620);
        let editable = self.backend.can_edit();
        email.set_sensitive(editable);
        role.set_sensitive(editable);
        if !editable {
            invite.set_description(Some(backend::ERR_EDITING_DISABLED));
        }
        {
            let send = send.clone();
            email.connect_changed(move |e| send.set_sensitive(e.text().contains('@')));
        }
        let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::default();
        let reload: Reload = Rc::default();
        let f: Rc<dyn Fn()> = {
            let s = Rc::downgrade(self);
            let (members, toasts, rows, reload, share, item) = (
                members.clone(),
                toasts.clone(),
                rows.clone(),
                reload.clone(),
                share.clone(),
                item.clone(),
            );
            Rc::new(move || {
                let Some(s) = s.upgrade() else { return };
                let (members, toasts, rows, reload, share, item) = (
                    members.clone(),
                    toasts.clone(),
                    rows.clone(),
                    reload.clone(),
                    share.clone(),
                    item.clone(),
                );
                glib::spawn_future_local(async move {
                    let result = s.backend.members(&share, item.as_ref()).await;
                    for r in rows.borrow_mut().drain(..) {
                        members.remove(&r);
                    }
                    let list = match result {
                        Ok(l) => l,
                        Err(e) => return toasts.add_toast(adw::Toast::new(e)),
                    };
                    for m in list {
                        let title = if m.name.is_empty() {
                            m.email.clone()
                        } else {
                            m.name.clone()
                        };
                        let subtitle = if m.name.is_empty() {
                            String::new()
                        } else {
                            m.email.clone()
                        };
                        let r = row(&title, &subtitle);
                        // Avatar initials come from the first letters of the
                        // first two words; drop punctuation like "(demo)".
                        let initials: String = title
                            .chars()
                            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
                            .collect();
                        r.add_prefix(&adw::Avatar::new(32, Some(initials.trim()), true));
                        if m.is_owner() {
                            let l = gtk::Label::new(Some("Owner"));
                            l.add_css_class("dim-label");
                            r.add_suffix(&l);
                        } else {
                            let roles =
                                gtk::DropDown::new(Some(role_model()), gtk::Expression::NONE);
                            roles.set_valign(gtk::Align::Center);
                            roles.set_selected(
                                model::ROLES
                                    .iter()
                                    .position(|(_, l)| l.eq_ignore_ascii_case(&m.role))
                                    .unwrap_or(0) as u32,
                            );
                            roles.update_property(&[gtk::accessible::Property::Label(&format!(
                                "Role of {title}"
                            ))]);
                            roles.set_sensitive(editable && item.is_none());
                            let remove =
                                icon_button("list-remove-symbolic", &format!("Remove {title}"));
                            remove.set_valign(gtk::Align::Center);
                            remove.add_css_class("flat");
                            remove.set_sensitive(editable && item.is_none());
                            r.add_suffix(&roles);
                            r.add_suffix(&remove);
                            {
                                let (s, m, share, toasts) =
                                    (s.clone(), m.clone(), share.clone(), toasts.clone());
                                roles.connect_selected_notify(move |dd| {
                                    let Some((role, label)) =
                                        model::ROLES.get(dd.selected() as usize)
                                    else {
                                        return;
                                    };
                                    if label.eq_ignore_ascii_case(&m.role) {
                                        return;
                                    }
                                    let (s, m, share, toasts) =
                                        (s.clone(), m.clone(), share.clone(), toasts.clone());
                                    glib::spawn_future_local(async move {
                                        match s.backend.set_member_role(&share, &m, role).await {
                                            Ok(()) => toasts.add_toast(adw::Toast::new(&format!(
                                                "{} is now {}",
                                                m.email,
                                                label.to_lowercase()
                                            ))),
                                            Err(e) => toasts.add_toast(adw::Toast::new(e)),
                                        }
                                    });
                                });
                            }
                            {
                                let (s, m, share, toasts, reload) = (
                                    s.clone(),
                                    m.clone(),
                                    share.clone(),
                                    toasts.clone(),
                                    reload.clone(),
                                );
                                remove.connect_clicked(move |_| {
                                    let (s, m, share, toasts, reload) = (
                                        s.clone(),
                                        m.clone(),
                                        share.clone(),
                                        toasts.clone(),
                                        reload.clone(),
                                    );
                                    glib::spawn_future_local(async move {
                                        let a = adw::AlertDialog::builder()
                                            .heading("Remove Access?")
                                            .body(format!(
                                                "{} will no longer see this vault.",
                                                m.email
                                            ))
                                            .build();
                                        a.add_responses(&[
                                            ("cancel", "Cancel"),
                                            ("remove", "Remove"),
                                        ]);
                                        a.set_response_appearance(
                                            "remove",
                                            adw::ResponseAppearance::Destructive,
                                        );
                                        a.set_close_response("cancel");
                                        s.track_dialog(&a);
                                        if a.choose_future(Some(&s.win)).await != "remove" {
                                            return;
                                        }
                                        match s.backend.remove_member(&share, &m).await {
                                            Ok(()) => {
                                                toasts.add_toast(adw::Toast::new(&format!(
                                                    "Removed {}",
                                                    m.email
                                                )));
                                                if let Some(f) = reload.borrow().clone() {
                                                    f();
                                                }
                                            }
                                            Err(e) => toasts.add_toast(adw::Toast::new(e)),
                                        }
                                    });
                                });
                            }
                        }
                        members.add(&r);
                        rows.borrow_mut().push(r.upcast());
                    }
                });
            })
        };
        *reload.borrow_mut() = Some(f.clone());
        {
            let s = Rc::downgrade(self);
            let (toasts, reload) = (toasts.clone(), reload.clone());
            send.connect_clicked(move |b| {
                let Some(s) = s.upgrade() else { return };
                let (addr, role_key) = (
                    email.text().to_string(),
                    model::ROLES
                        .get(role.selected() as usize)
                        .map_or("viewer", |(k, _)| k),
                );
                b.set_sensitive(false);
                let (toasts, reload, share, item, email, b) = (
                    toasts.clone(),
                    reload.clone(),
                    share.clone(),
                    item.clone(),
                    email.clone(),
                    b.clone(),
                );
                glib::spawn_future_local(async move {
                    match s
                        .backend
                        .share(&share, item.as_ref(), &addr, role_key)
                        .await
                    {
                        Ok(()) => {
                            email.set_text("");
                            toasts.add_toast(adw::Toast::new(&format!(
                                "Invitation sent to {}",
                                addr.trim()
                            )));
                            if let Some(f) = reload.borrow().clone() {
                                f();
                            }
                        }
                        Err(e) => {
                            b.set_sensitive(true);
                            toasts.add_toast(adw::Toast::new(e));
                        }
                    }
                });
            });
        }
        {
            let reload = reload.clone();
            d.connect_closed(move |_| {
                reload.borrow_mut().take();
            });
        }
        f();
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }

    pub(super) fn invites_dialog(self: &Rc<Self>) {
        let content = page_box();
        let group = adw::PreferencesGroup::builder()
            .title("Pending invitations")
            .description("People who invited you to a vault or an item.")
            .build();
        content.append(&group);
        let (d, _, toasts) = sheet("Invitations", &content, 460, 480);
        let s = self.clone();
        glib::spawn_future_local(async move {
            let list: Vec<Invite> = match s.backend.invites().await {
                Ok(l) => l,
                Err(e) => return toasts.add_toast(adw::Toast::new(e)),
            };
            if list.is_empty() {
                let empty = adw::StatusPage::builder()
                    .icon_name("mail-inbox-symbolic")
                    .title("No invitations")
                    .build();
                empty.add_css_class("compact");
                group.add(&empty);
            }
            for inv in list {
                let title = if inv.name.is_empty() {
                    format!("Shared {}", inv.kind)
                } else {
                    inv.name.clone()
                };
                let r = row(&title, &format!("From {}", inv.inviter));
                r.add_prefix(&gtk::Image::from_icon_name("mail-unread-symbolic"));
                let accept = gtk::Button::builder()
                    .label("Accept")
                    .valign(gtk::Align::Center)
                    .css_classes(["suggested-action"])
                    .build();
                let reject = icon_button(
                    "window-close-symbolic",
                    &format!("Decline invitation from {}", inv.inviter),
                );
                reject.set_valign(gtk::Align::Center);
                reject.add_css_class("flat");
                r.add_suffix(&reject);
                r.add_suffix(&accept);
                for (b, yes) in [
                    (accept.clone().upcast::<gtk::Button>(), true),
                    (reject.clone(), false),
                ] {
                    let (s, inv, toasts, r, group) = (
                        s.clone(),
                        inv.clone(),
                        toasts.clone(),
                        r.clone(),
                        group.clone(),
                    );
                    b.connect_clicked(move |_| {
                        let (s, inv, toasts, r, group) = (
                            s.clone(),
                            inv.clone(),
                            toasts.clone(),
                            r.clone(),
                            group.clone(),
                        );
                        glib::spawn_future_local(async move {
                            match s.backend.respond_invite(&inv, yes).await {
                                Ok(()) => {
                                    group.remove(&r);
                                    toasts.add_toast(adw::Toast::new(if yes {
                                        "Invitation accepted"
                                    } else {
                                        "Invitation declined"
                                    }));
                                    if yes {
                                        s.after_vault_change(None);
                                    }
                                }
                                Err(e) => toasts.add_toast(adw::Toast::new(e)),
                            }
                        });
                    });
                }
                group.add(&r);
            }
        });
        self.track_dialog(&d);
        d.present(Some(&self.win));
    }

    // ---------- Move ----------

    pub(super) fn move_dialog(self: &Rc<Self>) {
        let Some(detail) = self.current.borrow().clone() else {
            return;
        };
        if !self.backend.can_edit() {
            return self.notify(backend::ERR_EDITING_DISABLED);
        }
        let targets: Vec<Vault> = self
            .vaults
            .borrow()
            .iter()
            .filter(|v| v.share_id != detail.item.share_id)
            .cloned()
            .collect();
        if targets.is_empty() {
            return self.notify("Create another vault first");
        }
        let d = adw::AlertDialog::builder()
            .heading("Move to Another Vault")
            .body(format!(
                "Choose where to move “{}”. People with access to the new vault will see it.",
                detail.item.title
            ))
            .build();
        d.add_responses(&[("cancel", "Cancel"), ("move", "Move")]);
        d.set_response_appearance("move", adw::ResponseAppearance::Suggested);
        d.set_close_response("cancel");
        let names: Vec<&str> = targets.iter().map(|v| v.name.as_str()).collect();
        let combo = adw::ComboRow::builder()
            .title("Vault")
            .model(&gtk::StringList::new(&names))
            .build();
        let list = boxed_list();
        list.append(&combo);
        d.set_extra_child(Some(&list));
        self.track_dialog(&d);
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await != "move" {
                return;
            }
            let Some(to) = targets.get(combo.selected() as usize).cloned() else {
                return;
            };
            match s.backend.move_item(&detail.item, &to).await {
                Ok(moved) => {
                    s.notify(&format!("Moved “{}” to {}", detail.item.title, to.name));
                    if let Some(r) = s
                        .vaults
                        .borrow()
                        .iter()
                        .position(|v| v.share_id == to.share_id)
                        .and_then(|p| s.vault_list.row_at_index(p as i32))
                    {
                        s.vault_list.select_row(Some(&r));
                    }
                    s.load_items(to, false, Some(moved));
                }
                Err(e) => s.notify(e),
            }
        });
    }

    // ---------- Export ----------

    pub(super) fn export_flow(self: &Rc<Self>) {
        let d = adw::AlertDialog::builder()
            .heading("Export All Vaults?")
            .body("pass-cli writes an offline HTML archive that contains every password, note and code in readable form. Store it somewhere safe and delete it when you no longer need it.\n\nPassword-protected archives need a terminal: pass-cli export archive --encrypt")
            .build();
        d.add_responses(&[("cancel", "Cancel"), ("export", "Choose File…")]);
        d.set_response_appearance("export", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let attachments = adw::SwitchRow::builder()
            .title("Include attachments")
            .subtitle("Makes the file larger")
            .build();
        let list = boxed_list();
        list.append(&attachments);
        d.set_extra_child(Some(&list));
        self.track_dialog(&d);
        let s = self.clone();
        glib::spawn_future_local(async move {
            if d.choose_future(Some(&s.win)).await != "export" {
                return;
            }
            let epoch = s.epoch.get();
            let dest = if let Some(dir) =
                std::env::var_os("PPG_MOCK_DOWNLOAD_DIR").filter(|_| s.backend.mock)
            {
                PathBuf::from(dir).join("proton-pass-archive.html")
            } else {
                let fd = gtk::FileDialog::builder()
                    .title("Save Export")
                    .initial_name("proton-pass-archive.html")
                    .modal(true)
                    .build();
                match fd.save_future(Some(&s.win)).await {
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
            s.notify("Exporting…");
            match s.backend.export(&dest, attachments.is_active()).await {
                Ok(()) => s.notify("Export saved"),
                Err(e) => s.notify(e),
            }
        });
    }
}
