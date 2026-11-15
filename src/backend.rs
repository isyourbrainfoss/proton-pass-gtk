// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    commands,
    model::{self, Detail, Item, Vault},
};
use futures_util::lock::Mutex;
use gtk::{gio, glib, prelude::*};
use std::{cell::Cell, rc::Rc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Backend {
    pub mock: bool,
    binary: String,
    signed_in: Rc<Cell<bool>>,
    queue: Rc<Mutex<()>>,
}
impl Default for Backend {
    fn default() -> Self {
        Self {
            mock: std::env::var("PPG_BACKEND").as_deref() == Ok("mock"),
            binary: std::env::var("PPG_PASS_CLI").unwrap_or_else(|_| "pass-cli".into()),
            signed_in: Rc::new(Cell::new(std::env::var_os("PPG_MOCK_SIGNED_OUT").is_none())),
            queue: Rc::new(Mutex::new(())),
        }
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
    l
}
fn spawn(bin: &str, args: &[String]) -> Result<gio::Subprocess, &'static str> {
    let mut argv = vec![std::ffi::OsStr::new(bin)];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    launcher()
        .spawn(&argv)
        .map_err(|_| "Could not start pass-cli. Install the CLI or use the Flatpak build.")
}
impl Backend {
    pub fn new(mock: bool, binary: impl Into<String>) -> Self {
        Self {
            mock,
            binary: binary.into(),
            signed_in: Rc::new(Cell::new(true)),
            queue: Rc::new(Mutex::new(())),
        }
    }

    /// Serialize subprocesses: pass-cli shares mutable local session state.
    async fn run(&self, args: Vec<String>) -> Result<Zeroizing<String>, &'static str> {
        let _guard = self.queue.lock().await;
        let p = spawn(&self.binary, &args)?;
        let result = futures_util::future::select(
            Box::pin(p.communicate_utf8_future(None)),
            Box::pin(glib::timeout_future_seconds(45)),
        )
        .await;
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
                "pass-cli could not complete the request. Check sign-in, extra password and Secret Service availability.",
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
    pub async fn items(&self, share: &str) -> Result<Vec<Item>, &'static str> {
        if self.mock {
            let mut a = vec![];
            for fixture in [
                include_str!("../tests/fixtures/item_view_login.json"),
                include_str!("../tests/fixtures/item_view_card.json"),
                include_str!("../tests/fixtures/item_view_note.json"),
                include_str!("../tests/fixtures/item_view_custom.json"),
            ] {
                let mut i = model::detail(fixture)?.item;
                i.share_id = share.into();
                a.push(i);
            }
            for (kind, title) in [
                ("identity", "Demo identity"),
                ("sshkey", "Demo SSH key"),
                ("wifi", "Demo Wi-Fi"),
                ("alias", "Demo alias"),
            ] {
                a.push(Item {
                    id: kind.into(),
                    share_id: share.into(),
                    title: title.into(),
                    kind: kind.into(),
                    state: "active".into(),
                });
            }
            a.sort_by(|a, b| a.title.cmp(&b.title));
            return Ok(a);
        }
        Ok(model::items(&self.run(commands::items(share)).await?)?
            .into_iter()
            .filter(|i| i.state == "active" || i.state.is_empty())
            .collect())
    }
    pub async fn detail(&self, i: &Item) -> Result<Detail, &'static str> {
        if self.mock {
            let fixture = match i.kind.as_str() {
                "login" => include_str!("../tests/fixtures/item_view_login.json"),
                "creditcard" => include_str!("../tests/fixtures/item_view_card.json"),
                "note" => include_str!("../tests/fixtures/item_view_note.json"),
                "custom" => include_str!("../tests/fixtures/item_view_custom.json"),
                _ => "",
            };
            let mut d = if fixture.is_empty() {
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
            } else {
                model::detail(fixture)?
            };
            d.item = i.clone();
            return Ok(d);
        }
        model::detail(&self.run(commands::view(&i.share_id, &i.id)).await?)
    }
    pub async fn secret(
        &self,
        i: &Item,
        field: &str,
        totp: bool,
    ) -> Result<Zeroizing<String>, &'static str> {
        if self.mock {
            return Ok(Zeroizing::new(
                if totp {
                    "123456"
                } else {
                    "FAKE-demo-password-only"
                }
                .into(),
            ));
        }
        if totp {
            return Ok(Zeroizing::new(model::totp(
                &self.run(commands::totp(&i.share_id, &i.id, field)).await?,
                field,
            )?));
        }
        let mut s = self.run(commands::field(&i.share_id, &i.id, field)).await?;
        if s.ends_with('\n') {
            s.pop();
        }
        Ok(s)
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
    pub async fn login(
        &self,
        active: Rc<std::cell::RefCell<Option<gio::Subprocess>>>,
        on_url: impl Fn(String),
    ) -> Result<(), &'static str> {
        if self.mock {
            self.signed_in.set(true);
            return Ok(());
        }
        let _guard = self.queue.lock().await;
        let p = spawn(&self.binary, &["login".into()])?;
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
            return Err(
                "Sign-in did not complete. For a Pass extra password, sign in with pass-cli in a terminal first.",
            );
        }
        Ok(())
    }
}
