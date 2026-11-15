// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    commands,
    model::{self, Attachment, Detail, Field, Item, Kind, LoginDraft, Vault},
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
            return model::vaults(include_str!("../tests/fixtures/vault_list.json"));
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
        ] {
            let mut d = model::detail(fixture)?;
            d.item.share_id = share.into();
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
        for (kind, title) in [
            ("identity", "Demo identity"),
            ("sshkey", "Demo SSH key"),
            ("wifi", "Demo Wi-Fi"),
            ("alias", "Demo alias"),
        ] {
            a.push(MockEntry {
                item: Item {
                    id: kind.into(),
                    share_id: share.into(),
                    title: title.into(),
                    kind: kind.into(),
                    state: "active".into(),
                },
                ..Default::default()
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
                e.iter().map(|e| e.item.clone()).collect::<Vec<_>>()
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
        if self.mock {
            return self.with_mock(share, |e, next| {
                *next += 1;
                let item = Item {
                    id: format!("demo_new_{next}"),
                    share_id: share.into(),
                    title: draft.title.trim().into(),
                    kind: "login".into(),
                    state: "active".into(),
                };
                let mut entry = MockEntry {
                    item: item.clone(),
                    detail: Some(mock_login_detail(&item, draft)),
                    secrets: HashMap::new(),
                };
                if let Some(p) = &draft.password {
                    entry.secrets.insert("password".into(), p.clone());
                }
                e.push(entry);
                item
            });
        }
        if !self.real_edits {
            return Err(ERR_EDITING_DISABLED);
        }
        self.run_input(commands::create_login(share), Some(draft.template()))
            .await?;
        // pass-cli does not document the create output; reload to find it.
        Ok(Item {
            id: String::new(),
            share_id: share.into(),
            title: draft.title.trim().into(),
            kind: "login".into(),
            state: "active".into(),
        })
    }
    pub async fn update_login(
        &self,
        i: &Item,
        old: &LoginDraft,
        new: &LoginDraft,
    ) -> Result<Item, &'static str> {
        new.validate()?;
        let mut item = i.clone();
        item.title = new.title.trim().into();
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| {
                let Some(entry) = e.iter_mut().find(|e| e.item.id == i.id) else {
                    return Err("The item no longer exists");
                };
                let mut d = mock_login_detail(&item, new);
                if let Some(old) = &entry.detail {
                    // Keep fields the editor does not touch (TOTP, custom, files).
                    d.fields.extend(
                        old.fields
                            .iter()
                            .filter(|f| {
                                !matches!(f.key.as_str(), "username" | "email" | "password")
                                    || !f.section.is_empty()
                            })
                            .cloned(),
                    );
                    d.attachments = old.attachments.clone();
                }
                entry.item = item.clone();
                entry.detail = Some(d);
                if let Some(p) = &new.password {
                    entry.secrets.insert("password".into(), p.clone());
                }
                Ok(item.clone())
            })?;
        }
        if !self.real_edits {
            return Err(ERR_EDITING_DISABLED);
        }
        if new.password.is_some() {
            return Err(ERR_PASSWORD_ARGV);
        }
        let fields = new.changed_fields(old);
        if !fields.is_empty() {
            self.run(commands::update(&i.share_id, &i.id, &fields))
                .await?;
        }
        Ok(item)
    }
    /// Moves to the trash (reversible in Proton Pass), never a hard delete.
    pub async fn trash(&self, i: &Item) -> Result<(), &'static str> {
        if self.mock {
            return self.with_mock(&i.share_id, |e, _| e.retain(|e| e.item.id != i.id));
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
fn mock_login_detail(item: &Item, d: &LoginDraft) -> Detail {
    let mut fields = vec![];
    for (key, v) in [("email", &d.email), ("username", &d.username)] {
        if !v.trim().is_empty() {
            fields.push(Field {
                key: key.into(),
                label: key.into(),
                kind: Kind::Text,
                value: Some(v.trim().into()),
                section: String::new(),
                period: 0,
            });
        }
    }
    fields.push(Field {
        key: "password".into(),
        label: "password".into(),
        kind: Kind::Hidden,
        value: None,
        section: String::new(),
        period: 0,
    });
    Detail {
        item: item.clone(),
        fields,
        note: d.note.clone(),
        urls: if d.url.trim().is_empty() {
            vec![]
        } else {
            vec![d.url.trim().into()]
        },
        alias: String::new(),
        attachments: vec![],
    }
}
