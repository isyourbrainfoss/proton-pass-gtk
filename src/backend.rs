// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    commands,
    generator::{self, PassphraseOptions},
    model::{
        self, Attachment, Detail, Field, Invite, Item, ItemDraft, Kind, LoginDraft, Member, Vault,
    },
};
use futures_util::lock::Mutex;
use gtk::{gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::Write,
    os::fd::{BorrowedFd, FromRawFd, OwnedFd},
    path::Path,
    rc::Rc,
};
use zeroize::Zeroizing;

/// Login needs the Pass extra password (mock, or a real failure while the
/// user has not supplied one).
pub const ERR_EXTRA_PASSWORD_REQUIRED: &str =
    "This account uses a Pass extra password. Enter it to finish signing in.";
pub const ERR_EXTRA_PASSWORD_REJECTED: &str =
    "Sign-in failed. Check your Pass extra password and try again.";
pub const ERR_EDITING_DISABLED: &str =
    "Editing is only enabled for the demo backend until it has been validated with a real account.";
pub const ERR_PASSWORD_ARGV: &str = "Changing a password is not supported here: pass-cli only accepts new values on the command line. Use another Proton Pass client.";
/// File descriptor number the extra password is passed on.
const EXTRA_PASSWORD_FD: i32 = 3;

#[derive(Clone, Default)]
struct MockEntry {
    item: Item,
    detail: Option<Detail>,
    /// Fake values for hidden fields edited in the demo.
    secrets: HashMap<String, Zeroizing<String>>,
}
#[derive(Default)]
struct MockState {
    shares: HashMap<String, Vec<MockEntry>>,
    next_id: u32,
    vaults: Option<Vec<Vault>>,
    members: HashMap<String, Vec<Member>>,
    invites: Option<Vec<Invite>>,
}
pub const ERR_SECRET_ARGV: &str = "Changing hidden values is not supported here: pass-cli only accepts new values on the command line. Use another Proton Pass client.";
fn validate_email(email: &str) -> Result<(), &'static str> {
    let e = email.trim();
    let ok = e.len() <= 254
        && !e.starts_with('-')
        && !e.chars().any(char::is_whitespace)
        && e.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.'));
    if ok {
        Ok(())
    } else {
        Err("Enter a valid email address")
    }
}
fn validate_name(name: &str) -> Result<&str, &'static str> {
    let n = name.trim();
    if n.is_empty() {
        return Err("A name is required");
    }
    if n.starts_with('-') || n.chars().count() > 100 || n.chars().any(char::is_control) {
        return Err("Use a shorter name without control characters or a leading dash");
    }
    Ok(n)
}

#[derive(Clone)]
pub struct Backend {
    pub mock: bool,
    binary: String,
    signed_in: Rc<Cell<bool>>,
    queue: Rc<Mutex<()>>,
    mock_needs_extra: bool,
    real_edits: bool,
    state: Rc<RefCell<MockState>>,
}
impl Default for Backend {
    fn default() -> Self {
        let mut b = Self::new(
            std::env::var("PPG_BACKEND").as_deref() == Ok("mock"),
            std::env::var("PPG_PASS_CLI").unwrap_or_else(|_| "pass-cli".into()),
        );
        b.signed_in
            .set(std::env::var_os("PPG_MOCK_SIGNED_OUT").is_none());
        b.mock_needs_extra = std::env::var_os("PPG_MOCK_EXTRA_PASSWORD").is_some();
        b.real_edits = std::env::var("PPG_ENABLE_REAL_EDITS").as_deref() == Ok("1");
        b
    }
}
fn launcher() -> gio::SubprocessLauncher {
    let l = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDOUT_PIPE
            | gio::SubprocessFlags::STDERR_SILENCE
            | gio::SubprocessFlags::STDIN_PIPE,
    );
    l.setenv("PROTON_PASS_LINUX_KEYRING", "dbus", true);
    l.setenv("PASS_LOG_LEVEL", "off", true);
    // Never forward a secret that happens to be in this app's environment.
    l.unsetenv("PROTON_PASS_EXTRA_PASSWORD");
    l.unsetenv("PROTON_PASS_EXTRA_PASSWORD_FILE");
    l
}
fn spawn_with(
    l: &gio::SubprocessLauncher,
    bin: &str,
    args: &[String],
) -> Result<gio::Subprocess, &'static str> {
    let mut argv = vec![std::ffi::OsStr::new(bin)];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    l.spawn(&argv)
        .map_err(|_| "Could not start pass-cli. Install the CLI or use the Flatpak build.")
}
fn spawn(bin: &str, args: &[String]) -> Result<gio::Subprocess, &'static str> {
    spawn_with(&launcher(), bin, args)
}
/// Put `secret` in a close-on-exec pipe and hand only its read end to the
/// child as fd 3, so the value never appears in argv or the environment.
/// pass-cli reads `PROTON_PASS_EXTRA_PASSWORD_FILE=/dev/fd/3` to EOF.
pub fn secret_pipe_launcher(secret: &str) -> Result<gio::SubprocessLauncher, &'static str> {
    const ERR: &str = "Could not prepare the extra password for pass-cli";
    if secret.len() >= 16 * 1024 {
        return Err("The extra password is too long");
    }
    let (r, w) = glib::unix_open_pipe(1 /* FD_CLOEXEC */).map_err(|_| ERR)?;
    // SAFETY: both fds were just created by g_unix_open_pipe and are owned here.
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(r), OwnedFd::from_raw_fd(w)) };
    let mut w = std::fs::File::from(w);
    // Far below the pipe buffer size, so this cannot block.
    w.write_all(secret.as_bytes()).map_err(|_| ERR)?;
    drop(w); // EOF for the reader
    let l = launcher();
    // SAFETY: only used as the numeric target descriptor for dup2 in the child.
    l.take_fd(r, unsafe { BorrowedFd::borrow_raw(EXTRA_PASSWORD_FD) });
    l.setenv(
        "PROTON_PASS_EXTRA_PASSWORD_FILE",
        format!("/dev/fd/{EXTRA_PASSWORD_FD}"),
        true,
    );
    Ok(l)
}
fn fake_code(field: &Field, now: i64) -> String {
    // Clearly fake: derived from the time step and field name, not a seed.
    let salt: i64 = field.key.bytes().map(i64::from).sum();
    let step = model::totp_step(now, field.period.max(1));
    format!(
        "{:06}",
        (step * 7919 + salt * 104_729).rem_euclid(1_000_000)
    )
}
impl Backend {
    pub fn new(mock: bool, binary: impl Into<String>) -> Self {
        Self {
            mock,
            binary: binary.into(),
            signed_in: Rc::new(Cell::new(true)),
            queue: Rc::new(Mutex::new(())),
            mock_needs_extra: false,
            real_edits: false,
            state: Rc::new(RefCell::new(MockState::default())),
        }
    }
    /// Mock only: make sign-in require an extra password (any value except
    /// `wrong` is accepted).
    pub fn set_mock_extra_password(&mut self, required: bool) {
        self.mock_needs_extra = required;
    }
    /// Real-account writes stay off unless `PPG_ENABLE_REAL_EDITS=1`.
    pub fn set_real_edits(&mut self, enabled: bool) {
        self.real_edits = enabled;
    }
    pub fn can_edit(&self) -> bool {
        self.mock || self.real_edits
    }

    /// Serialize subprocesses: pass-cli shares mutable local session state.
    async fn run(&self, args: Vec<String>) -> Result<Zeroizing<String>, &'static str> {
        self.run_input(args, None).await
    }
    async fn run_input(
        &self,
        args: Vec<String>,
        stdin: Option<Zeroizing<String>>,
    ) -> Result<Zeroizing<String>, &'static str> {
        let _guard = self.queue.lock().await;
        let p = spawn(&self.binary, &args)?;
        let result = futures_util::future::select(
            Box::pin(p.communicate_utf8_future(stdin.as_ref().map(|s| s.as_str().into()))),
            Box::pin(glib::timeout_future_seconds(45)),
        )
        .await;
        drop(stdin);
        let (out, _) = match result {
            futures_util::future::Either::Left((result, _)) => {
                result.map_err(|_| "pass-cli communication failed")?
            }
            futures_util::future::Either::Right(_) => {
                p.force_exit();
                return Err("pass-cli timed out. Check your connection and Secret Service.");
            }
        };
        let out = Zeroizing::new(out.map(String::from).unwrap_or_default());
        if !p.is_successful() {
            return Err(
                "pass-cli could not complete the request. Check sign-in, the session lock and Secret Service availability.",
            );
        }
        Ok(out)
    }
    pub async fn info(&self) -> Result<String, &'static str> {
        if self.mock {
            return if self.signed_in.get() {
                Ok("Demo account · no real data".into())
            } else {
                Err("Not signed in")
            };
        }
        model::info(&self.run(commands::info()).await?)
    }
    pub async fn vaults(&self) -> Result<Vec<Vault>, &'static str> {
        if self.mock {
            let mut st = self.state.borrow_mut();
            if st.vaults.is_none() {
                st.vaults = Some(model::vaults(include_str!(
                    "../tests/fixtures/vault_list.json"
                ))?);
            }
            return Ok(st.vaults.clone().unwrap_or_default());
        }
        model::vaults(&self.run(commands::vaults()).await?)
    }
    fn seed(share: &str) -> Result<Vec<MockEntry>, &'static str> {
        let mut a = vec![];
        for fixture in [
            include_str!("../tests/fixtures/item_view_login.json"),
            include_str!("../tests/fixtures/item_view_card.json"),
            include_str!("../tests/fixtures/item_view_note.json"),
            include_str!("../tests/fixtures/item_view_custom.json"),
            include_str!("../tests/fixtures/item_view_totp_periods.json"),
            include_str!("../tests/fixtures/item_view_identity.json"),
            include_str!("../tests/fixtures/item_view_sshkey.json"),
            include_str!("../tests/fixtures/item_view_wifi.json"),
            include_str!("../tests/fixtures/item_view_alias.json"),
        ] {
            let mut d = model::detail(fixture)?;
            d.item.share_id = share.into();
            d.item.state = "active".into();
            if d.item.id == "itm_01" {
                d.attachments = vec![
                    Attachment {
                        id: "att_demo_1".into(),
                        name: "demo-receipt.pdf".into(),
                        size: 48 * 1024,
                        mime_type: "application/pdf".into(),
                    },
                    Attachment {
                        id: "att_demo_2".into(),
                        name: "demo-diagram.png".into(),
                        size: 215_040,
                        mime_type: "image/png".into(),
                    },
                ];
            }
            a.push(MockEntry {
                item: d.item.clone(),
                detail: Some(d),
                secrets: HashMap::new(),
            });
        }
        // Two demo items already in the trash.
        for (id, title, user) in [
            ("trash_1", "Old demo forum", "former_demo_user"),
            ("trash_2", "Expired demo shop", "shopper@example.invalid"),
        ] {
            let item = Item {
                id: id.into(),
                share_id: share.into(),
                title: title.into(),
                kind: "login".into(),
                state: "trashed".into(),
            };
            let draft = LoginDraft {
                title: title.into(),
                username: user.into(),
                ..Default::default()
            };
            a.push(MockEntry {
                detail: Some(mock_detail(&item, &ItemDraft::from(&draft))),
                item,
                secrets: HashMap::new(),
            });
        }
        Ok(a)
    }
    fn with_mock<T>(
        &self,
        share: &str,
        f: impl FnOnce(&mut Vec<MockEntry>, &mut u32) -> T,
    ) -> Result<T, &'static str> {
        let mut st = self.state.borrow_mut();
        let st = &mut *st;
        if !st.shares.contains_key(share) {
            st.shares.insert(share.into(), Self::seed(share)?);
        }
        Ok(f(st.shares.get_mut(share).unwrap(), &mut st.next_id))
    }
    pub async fn items(&self, share: &str) -> Result<Vec<Item>, &'static str> {
        if self.mock {
            let mut a = self.with_mock(share, |e, _| {
                e.iter()
                    .filter(|e| e.item.state == "active")
                    .map(|e| e.item.clone())
                    .collect::<Vec<_>>()
            })?;
            a.sort_by_key(|i| i.title.to_lowercase());
            return Ok(a);
        }
        Ok(model::items(&self.run(commands::items(share)).await?)?
            .into_iter()
            .filter(|i| i.state == "active" || i.state.is_empty())
            .collect())
    }
    pub async fn detail(&self, i: &Item) -> Result<Detail, &'static str> {
        if self.mock {
            let found = self.with_mock(&i.share_id, |e, _| {
                e.iter()
                    .find(|e| e.item.id == i.id)
                    .and_then(|e| e.detail.clone())
            })?;
            let mut d = match found {
                Some(d) => d,
                None => {
                    let variant = match i.kind.as_str() {
                        "identity" => {
                            serde_json::json!({"Identity":{"full_name":"Demo Person","email":"demo@example.invalid","passport_number":"FAKE-never-real"}})
                        }
                        "sshkey" => {
                            serde_json::json!({"SshKey":{"public_key":"FAKE PUBLIC KEY · not valid","private_key":"FAKE PRIVATE KEY · not valid"}})
                        }
                        "wifi" => {
                            serde_json::json!({"Wifi":{"ssid":"Demo network","password":"FAKE-not-a-password"}})
                        }
                        _ => serde_json::json!("Alias"),
                    };
                    model::detail(&serde_json::json!({"item":{"content":{"title":i.title,"note":"Clearly fake demonstration data. Never use these credentials.","content":variant},"alias_email": if i.kind=="alias" {"demo@alias.invalid"} else {""}}}).to_string())?
                }
            };
            d.item = i.clone();
            return Ok(d);
        }
        model::detail(&self.run(commands::view(&i.share_id, &i.id)).await?)
    }
    pub async fn secret(&self, i: &Item, field: &Field) -> Result<Zeroizing<String>, &'static str> {
        let totp = field.kind == Kind::Totp;
        if self.mock {
            if totp {
                return Ok(Zeroizing::new(fake_code(
                    field,
                    glib::real_time() / 1_000_000,
                )));
            }
            let edited = self.with_mock(&i.share_id, |e, _| {
                e.iter()
                    .find(|e| e.item.id == i.id)
                    .and_then(|e| e.secrets.get(&field.key).cloned())
            })?;
            return Ok(edited.unwrap_or_else(|| Zeroizing::new("FAKE-demo-password-only".into())));
        }
        if totp {
            return Ok(Zeroizing::new(model::totp(
                &self
                    .run(commands::totp(&i.share_id, &i.id, &field.key))
                    .await?,
                &field.key,
            )?));
        }
        let mut s = self
            .run(commands::field(&i.share_id, &i.id, &field.key))
            .await?;
        if s.ends_with('\n') {
            s.pop();
        }
        Ok(s)
    }
    pub async fn create_login(
        &self,
        share: &str,
        draft: &LoginDraft,
    ) -> Result<Item, &'static str> {
        draft.validate()?;
        self.create_item(share, &ItemDraft::from(draft)).await
    }
    pub async fn update_login(
        &self,
        i: &Item,
        old: &LoginDraft,
        new: &LoginDraft,
    ) -> Result<Item, &'static str> {
        new.validate()?;
        self.update_item(i, &ItemDraft::from(old), &ItemDraft::from(new))
            .await
    }
    /// Moves to the trash (reversible in Proton Pass), never a hard delete.
    pub async fn trash(&self, i: &Item) -> Result<(), &'static str> {
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| {
                for e in e.iter_mut().filter(|e| e.item.id == i.id) {
                    e.item.state = "trashed".into();
                }
            });
        }
        if !self.real_edits {
            return Err(ERR_EDITING_DISABLED);
        }
        self.run(commands::trash(&i.share_id, &i.id)).await?;
        Ok(())
    }
    /// Mock: writes a small, clearly fake text file to `dest`.
    pub async fn download_attachment(
        &self,
        i: &Item,
        a: &Attachment,
        dest: &Path,
    ) -> Result<(), &'static str> {
        if self.mock {
            let body = format!(
                "FAKE demo attachment \"{}\" from the Proton Pass GTK mock backend.\n\
                 It contains no real data and was not downloaded from Proton.\n",
                a.name
            );
            return std::fs::write(dest, body).map_err(|_| "Could not save the demo file");
        }
        let dest = dest.to_str().ok_or("Unsupported file name")?;
        self.run(commands::attachment_download(
            &i.share_id,
            &i.id,
            &a.id,
            dest,
        ))
        .await?;
        Ok(())
    }
    pub async fn logout(&self) -> Result<(), &'static str> {
        if self.mock {
            self.signed_in.set(false);
            return Ok(());
        }
        self.run(vec!["logout".into(), "--force".into()]).await?;
        Ok(())
    }
    /// The URL callback must not log or persist the transient login link.
    /// `extra_password` is consumed and zeroized; it is only ever given to
    /// pass-cli through an inherited pipe.
    pub async fn login(
        &self,
        active: Rc<RefCell<Option<gio::Subprocess>>>,
        extra_password: Option<Zeroizing<String>>,
        on_url: impl Fn(String),
    ) -> Result<(), &'static str> {
        if self.mock {
            if self.mock_needs_extra {
                match extra_password.as_deref().map(String::as_str) {
                    None | Some("") => return Err(ERR_EXTRA_PASSWORD_REQUIRED),
                    Some("wrong") => return Err(ERR_EXTRA_PASSWORD_REJECTED),
                    Some(_) => (),
                }
            }
            drop(extra_password);
            self.signed_in.set(true);
            return Ok(());
        }
        let had_extra = extra_password.is_some();
        let _guard = self.queue.lock().await;
        let l = match &extra_password {
            Some(p) => secret_pipe_launcher(p)?,
            None => launcher(),
        };
        drop(extra_password);
        let p = spawn_with(&l, &self.binary, &["login".into()])?;
        drop(l); // closes the parent's copy of the pipe
        *active.borrow_mut() = Some(p.clone());
        if let Some(stdin) = p.stdin_pipe() {
            let _ = stdin.close(gio::Cancellable::NONE);
        }
        let stream = gio::DataInputStream::new(&p.stdout_pipe().ok_or("No login output")?);
        let mut opened = false;
        loop {
            let line = stream
                .read_line_future(glib::Priority::DEFAULT)
                .await
                .map_err(|_| "Sign-in cancelled")?;
            let Some(line) = line else { break };
            let line = Zeroizing::new(String::from_utf8_lossy(&line).into_owned());
            if !opened {
                if let Some(url) = commands::login_url(&line) {
                    opened = true;
                    on_url(url);
                }
            }
        }
        let result = p.wait_future().await;
        active.borrow_mut().take();
        if result.is_err() || !p.is_successful() {
            return Err(if had_extra {
                ERR_EXTRA_PASSWORD_REJECTED
            } else {
                "Sign-in did not complete. If your account uses a Pass extra password, turn on “Ask for extra password” and try again."
            });
        }
        Ok(())
    }
}
impl Backend {
    fn require_edits(&self) -> Result<(), &'static str> {
        if self.real_edits {
            Ok(())
        } else {
            Err(ERR_EDITING_DISABLED)
        }
    }
    /// Create an item of any supported type. Secrets travel on stdin only.
    pub async fn create_item(&self, share: &str, draft: &ItemDraft) -> Result<Item, &'static str> {
        draft.validate()?;
        if self.mock {
            return self.with_mock(share, |e, next| {
                *next += 1;
                let (title, alias) = if draft.kind == "alias" {
                    let a = format!("{}.demo{next:03}@passinbox.invalid", draft.extra.trim());
                    (a.clone(), a)
                } else {
                    (draft.title.trim().to_owned(), String::new())
                };
                let item = Item {
                    id: format!("demo_new_{next}"),
                    share_id: share.into(),
                    title,
                    kind: draft.kind.clone(),
                    state: "active".into(),
                };
                let mut detail = mock_detail(&item, draft);
                detail.alias = alias;
                if draft.kind == "sshkey" {
                    detail.fields.push(Field {
                        key: "public_key".into(),
                        label: "public key".into(),
                        kind: Kind::Text,
                        value: Some(format!(
                            "ssh-{} AAAAFAKEdemo{next}notarealkey demo@example.invalid",
                            if draft.extra.starts_with("rsa") {
                                "rsa"
                            } else {
                                "ed25519"
                            }
                        )),
                        section: String::new(),
                        period: 0,
                    });
                    detail.fields.push(Field {
                        key: "private_key".into(),
                        label: "private key".into(),
                        kind: Kind::Hidden,
                        value: None,
                        section: String::new(),
                        period: 0,
                    });
                }
                let mut entry = MockEntry {
                    item: item.clone(),
                    detail: Some(detail),
                    secrets: HashMap::new(),
                };
                for spec in model::field_specs(&draft.kind).iter().filter(|s| s.hidden) {
                    let v = draft.get(spec.key);
                    if !v.is_empty() {
                        let key = if draft.kind == "custom" {
                            spec.label
                        } else {
                            spec.key
                        };
                        entry
                            .secrets
                            .insert(key.into(), Zeroizing::new(v.to_owned()));
                    }
                }
                e.push(entry);
                item
            });
        }
        self.require_edits()?;
        let title = draft.title.trim().to_owned();
        let out = match draft.kind.as_str() {
            "alias" => {
                let out = self
                    .run(commands::create_alias(share, draft.extra.trim()))
                    .await?;
                let (id, alias) = model::created_alias(&out)?;
                return Ok(Item {
                    id,
                    share_id: share.into(),
                    title: alias,
                    kind: "alias".into(),
                    state: "active".into(),
                });
            }
            "sshkey" => {
                let key_type = if draft.extra.is_empty() {
                    "ed25519"
                } else {
                    &draft.extra
                };
                self.run(commands::generate_ssh_key(share, &title, key_type))
                    .await?
            }
            kind => {
                let sub =
                    model::create_subcommand(kind).ok_or("This item type cannot be created")?;
                self.run_input(commands::create_item(sub, share), Some(draft.template()))
                    .await?
            }
        };
        // pass-cli prints the new item ID on success (`println!("{res}")`).
        let id = out.trim();
        let id = if !id.is_empty() && !id.contains(char::is_whitespace) {
            id.to_owned()
        } else {
            String::new()
        };
        Ok(Item {
            id,
            share_id: share.into(),
            title,
            kind: draft.kind.clone(),
            state: "active".into(),
        })
    }
    /// Update an item. Real accounts: only non-secret fields, via argv.
    pub async fn update_item(
        &self,
        i: &Item,
        old: &ItemDraft,
        new: &ItemDraft,
    ) -> Result<Item, &'static str> {
        new.validate()?;
        let mut item = i.clone();
        item.title = new.title.trim().into();
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| {
                let Some(entry) = e.iter_mut().find(|e| e.item.id == i.id) else {
                    return Err("The item no longer exists");
                };
                let mut d = mock_detail(&item, new);
                if let Some(old) = &entry.detail {
                    let edited: Vec<String> = d.fields.iter().map(|f| f.key.clone()).collect();
                    let spec_keys: Vec<&str> = model::field_specs(&new.kind)
                        .iter()
                        .map(|s| if new.kind == "custom" { s.label } else { s.key })
                        .collect();
                    // Keep fields the editor does not touch (TOTP, sections, files)
                    // and hidden values left empty (= unchanged).
                    let kept: Vec<Field> = old
                        .fields
                        .iter()
                        .filter(|f| {
                            let editable_text = f.kind == Kind::Text
                                && spec_keys.contains(&f.key.as_str())
                                && (f.section.is_empty() || new.kind == "custom");
                            !edited.contains(&f.key) && !editable_text
                        })
                        .cloned()
                        .collect();
                    d.fields.extend(kept);
                    d.attachments = old.attachments.clone();
                    d.passkeys = old.passkeys.clone();
                    d.alias = old.alias.clone();
                }
                for spec in model::field_specs(&new.kind).iter().filter(|s| s.hidden) {
                    let v = new.get(spec.key);
                    if !v.is_empty() {
                        let key = if new.kind == "custom" {
                            spec.label
                        } else {
                            spec.key
                        };
                        entry
                            .secrets
                            .insert(key.into(), Zeroizing::new(v.to_owned()));
                    }
                }
                entry.item = item.clone();
                entry.detail = Some(d);
                Ok(item.clone())
            })?;
        }
        self.require_edits()?;
        if new.has_secret_change() {
            return Err(if new.kind == "login" {
                ERR_PASSWORD_ARGV
            } else {
                ERR_SECRET_ARGV
            });
        }
        let fields = new.changed_fields(old);
        if !fields.is_empty() {
            let pairs: Vec<(&str, String)> = fields
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            self.run(commands::update(&i.share_id, &i.id, &pairs))
                .await?;
        }
        Ok(item)
    }
    /// Trashed items of all given vaults.
    pub async fn trashed(&self, vaults: &[Vault]) -> Result<Vec<Item>, &'static str> {
        let mut out = vec![];
        for v in vaults {
            if self.mock {
                out.extend(self.with_mock(&v.share_id, |e, _| {
                    e.iter()
                        .filter(|e| e.item.state == "trashed")
                        .map(|e| e.item.clone())
                        .collect::<Vec<_>>()
                })?);
            } else {
                out.extend(
                    model::items(&self.run(commands::trashed_items(&v.share_id)).await?)?
                        .into_iter()
                        .filter(|i| i.state == "trashed"),
                );
            }
        }
        out.sort_by_key(|i| i.title.to_lowercase());
        Ok(out)
    }
    pub async fn untrash(&self, i: &Item) -> Result<(), &'static str> {
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| {
                for e in e.iter_mut().filter(|e| e.item.id == i.id) {
                    e.item.state = "active".into();
                }
            });
        }
        self.require_edits()?;
        self.run(commands::untrash(&i.share_id, &i.id)).await?;
        Ok(())
    }
    /// Permanently delete an item that is already in the trash.
    pub async fn delete_forever(&self, i: &Item) -> Result<(), &'static str> {
        if i.state != "trashed" {
            return Err("Only items in the trash can be deleted permanently");
        }
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| e.retain(|e| e.item.id != i.id));
        }
        self.require_edits()?;
        self.run(commands::delete(&i.share_id, &i.id)).await?;
        Ok(())
    }
    pub async fn move_item(&self, i: &Item, to: &Vault) -> Result<Item, &'static str> {
        if i.share_id == to.share_id {
            return Err("The item is already in this vault");
        }
        let mut moved = i.clone();
        moved.share_id = to.share_id.clone();
        if self.mock {
            let entry = self.with_mock(&i.share_id, |e, _| {
                let pos = e.iter().position(|e| e.item.id == i.id)?;
                Some(e.remove(pos))
            })?;
            let mut entry = entry.ok_or("The item no longer exists")?;
            entry.item.share_id = to.share_id.clone();
            if let Some(d) = &mut entry.detail {
                d.item.share_id = to.share_id.clone();
            }
            self.with_mock(&to.share_id, |e, _| e.push(entry))?;
            return Ok(moved);
        }
        self.require_edits()?;
        self.run(commands::move_item(&i.share_id, &i.id, &to.share_id))
            .await?;
        // The CLI may assign a new item ID in the destination vault.
        moved.id = String::new();
        Ok(moved)
    }
    pub async fn create_vault(&self, name: &str) -> Result<Vault, &'static str> {
        let name = validate_name(name)?.to_owned();
        if self.mock {
            self.vaults().await?;
            let mut st = self.state.borrow_mut();
            st.next_id += 1;
            let v = Vault {
                name,
                share_id: format!("shr_demo_{}", st.next_id),
            };
            st.shares.insert(v.share_id.clone(), vec![]);
            st.vaults.get_or_insert_with(Vec::new).push(v.clone());
            return Ok(v);
        }
        self.require_edits()?;
        let out = self.run(commands::vault_create(&name)).await?;
        Ok(Vault {
            name,
            share_id: out.trim().to_owned(),
        })
    }
    pub async fn rename_vault(&self, v: &Vault, name: &str) -> Result<Vault, &'static str> {
        let name = validate_name(name)?.to_owned();
        let renamed = Vault {
            name: name.clone(),
            share_id: v.share_id.clone(),
        };
        if self.mock {
            self.vaults().await?;
            let mut st = self.state.borrow_mut();
            for x in st
                .vaults
                .iter_mut()
                .flatten()
                .filter(|x| x.share_id == v.share_id)
            {
                x.name = name.clone();
            }
            return Ok(renamed);
        }
        self.require_edits()?;
        self.run(commands::vault_rename(&v.share_id, &name)).await?;
        Ok(renamed)
    }
    /// Deletes the vault and every item in it. The UI asks for the vault name.
    pub async fn delete_vault(&self, v: &Vault) -> Result<(), &'static str> {
        if self.mock {
            self.vaults().await?;
            let mut st = self.state.borrow_mut();
            let vaults = st.vaults.get_or_insert_with(Vec::new);
            if vaults.len() <= 1 {
                return Err("You cannot delete your last vault");
            }
            vaults.retain(|x| x.share_id != v.share_id);
            st.shares.remove(&v.share_id);
            return Ok(());
        }
        self.require_edits()?;
        self.run(commands::vault_delete(&v.share_id)).await?;
        Ok(())
    }
    fn mock_members(&self, key: &str) -> Vec<Member> {
        self.state
            .borrow_mut()
            .members
            .entry(key.into())
            .or_insert_with(|| {
                let mut m = vec![Member {
                    share_id: format!("{key}_owner"),
                    email: "demo.user@example.com".into(),
                    name: "You (demo)".into(),
                    role: "Owner".into(),
                }];
                if key == "shr_2222" {
                    m.push(Member {
                        share_id: "mbr_demo_colleague".into(),
                        email: "colleague@example.invalid".into(),
                        name: "Demo Colleague".into(),
                        role: "Editor".into(),
                    });
                }
                m
            })
            .clone()
    }
    /// Members of a vault, or of a single shared item when `item` is given.
    pub async fn members(
        &self,
        share: &str,
        item: Option<&Item>,
    ) -> Result<Vec<Member>, &'static str> {
        if self.mock {
            let key = item.map_or(share.to_owned(), |i| format!("{share}/{}", i.id));
            return Ok(self.mock_members(&key));
        }
        let args = match item {
            Some(i) => commands::item_members(share, &i.id),
            None => commands::vault_members(share),
        };
        model::members(&self.run(args).await?)
    }
    /// Invite someone to a vault or (with `item`) a single item.
    pub async fn share(
        &self,
        share: &str,
        item: Option<&Item>,
        email: &str,
        role: &str,
    ) -> Result<(), &'static str> {
        validate_email(email)?;
        if !model::ROLES.iter().any(|(r, _)| *r == role) {
            return Err("Unknown role");
        }
        let email = email.trim();
        if self.mock {
            let key = item.map_or(share.to_owned(), |i| format!("{share}/{}", i.id));
            self.mock_members(&key);
            let mut st = self.state.borrow_mut();
            let list = st.members.get_mut(&key).unwrap();
            if list.iter().any(|m| m.email.eq_ignore_ascii_case(email)) {
                return Err("This person already has access");
            }
            st.next_id += 1;
            let id = st.next_id;
            let list = st.members.get_mut(&key).unwrap();
            list.push(Member {
                share_id: format!("mbr_demo_{id}"),
                email: email.into(),
                name: String::new(),
                role: model::ROLES
                    .iter()
                    .find(|(r, _)| *r == role)
                    .map_or("Viewer", |(_, l)| l)
                    .into(),
            });
            return Ok(());
        }
        self.require_edits()?;
        let args = match item {
            Some(i) => commands::item_share(share, &i.id, email, role),
            None => commands::vault_share(share, email, role),
        };
        self.run(args).await?;
        Ok(())
    }
    pub async fn remove_member(&self, share: &str, m: &Member) -> Result<(), &'static str> {
        if m.is_owner() {
            return Err("The owner cannot be removed");
        }
        if self.mock {
            for list in self.state.borrow_mut().members.values_mut() {
                list.retain(|x| x.share_id != m.share_id);
            }
            return Ok(());
        }
        self.require_edits()?;
        self.run(commands::vault_member_remove(share, &m.share_id))
            .await?;
        Ok(())
    }
    pub async fn set_member_role(
        &self,
        share: &str,
        m: &Member,
        role: &str,
    ) -> Result<(), &'static str> {
        if m.is_owner() {
            return Err("The owner’s role cannot be changed");
        }
        let label = model::ROLES
            .iter()
            .find(|(r, _)| *r == role)
            .map(|(_, l)| *l)
            .ok_or("Unknown role")?;
        if self.mock {
            for list in self.state.borrow_mut().members.values_mut() {
                for x in list.iter_mut().filter(|x| x.share_id == m.share_id) {
                    x.role = label.into();
                }
            }
            return Ok(());
        }
        self.require_edits()?;
        self.run(commands::vault_member_role(share, &m.share_id, role))
            .await?;
        Ok(())
    }
    pub async fn invites(&self) -> Result<Vec<Invite>, &'static str> {
        if self.mock {
            let mut st = self.state.borrow_mut();
            return Ok(st
                .invites
                .get_or_insert_with(|| {
                    vec![Invite {
                        token: "inv_demo_1".into(),
                        kind: "vault".into(),
                        inviter: "friend@example.invalid".into(),
                        name: "Demo family vault".into(),
                    }]
                })
                .clone());
        }
        model::invites(&self.run(commands::invites()).await?)
    }
    pub async fn respond_invite(&self, inv: &Invite, accept: bool) -> Result<(), &'static str> {
        if self.mock {
            self.invites().await?;
            let mut st = self.state.borrow_mut();
            if let Some(list) = st.invites.as_mut() {
                list.retain(|i| i.token != inv.token);
            }
            if accept && inv.kind == "vault" {
                st.next_id += 1;
                let v = Vault {
                    name: if inv.name.is_empty() {
                        "Shared vault".into()
                    } else {
                        inv.name.clone()
                    },
                    share_id: format!("shr_demo_{}", st.next_id),
                };
                st.vaults.get_or_insert_with(Vec::new).push(v);
            }
            return Ok(());
        }
        self.require_edits()?;
        self.run(commands::invite_respond(accept, &inv.token))
            .await?;
        Ok(())
    }
    /// Unencrypted HTML archive of all vaults. Mock: a small fake file.
    pub async fn export(&self, dest: &Path, attachments: bool) -> Result<(), &'static str> {
        if self.mock {
            let body = "<!doctype html><title>FAKE Proton Pass demo export</title>\
                <p>FAKE demo export from the Proton Pass GTK mock backend. It contains no real data.</p>\n";
            return std::fs::write(dest, body).map_err(|_| "Could not save the demo export");
        }
        let dest = dest.to_str().ok_or("Unsupported file name")?;
        self.run(commands::export_archive(dest, attachments))
            .await?;
        Ok(())
    }
    /// Passphrase from pass-cli's official generator (offline, no session).
    /// The demo backend uses a tiny fake word list instead.
    pub async fn passphrase(
        &self,
        o: &PassphraseOptions,
    ) -> Result<Zeroizing<String>, &'static str> {
        if self.mock {
            return generator::demo_passphrase(o);
        }
        let out = self
            .run(commands::generate_passphrase(
                o.words.clamp(generator::MIN_WORDS, generator::MAX_WORDS),
                &o.separator,
                o.capitalise,
                o.numbers,
            ))
            .await?;
        model::generated_password(&out)
    }
}
/// Demo detail for a draft: text values are shown, hidden values become
/// masked fields whose fake value lives in `MockEntry::secrets`.
fn mock_detail(item: &Item, d: &ItemDraft) -> Detail {
    let mut fields = vec![];
    for spec in model::field_specs(&d.kind) {
        let v = d.get(spec.key).trim();
        let always = d.kind == "login" && spec.key == "password";
        if v.is_empty() && !always {
            continue;
        }
        let custom = d.kind == "custom";
        fields.push(Field {
            key: if custom {
                spec.label.into()
            } else {
                spec.key.into()
            },
            label: if custom {
                spec.label.into()
            } else {
                spec.key.replace('_', " ")
            },
            kind: if spec.key == "totp" {
                Kind::Totp
            } else if spec.hidden {
                Kind::Hidden
            } else {
                Kind::Text
            },
            value: (!spec.hidden).then(|| v.to_owned()),
            section: if custom {
                "Custom fields".into()
            } else {
                String::new()
            },
            period: if spec.key == "totp" {
                model::totp_period(v)
            } else {
                0
            },
        });
    }
    Detail {
        item: item.clone(),
        fields,
        note: d.note.clone(),
        urls: if d.url.trim().is_empty() || d.kind != "login" {
            vec![]
        } else {
            vec![d.url.trim().into()]
        },
        alias: String::new(),
        attachments: vec![],
        passkeys: vec![],
    }
}
