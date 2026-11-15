// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(feature = "ui")]
use gtk::glib;
use proton_pass_gtk::{
    backend::{self, Backend},
    model::{Kind, LoginDraft},
};
use std::{cell::RefCell, rc::Rc};
use zeroize::Zeroizing;

fn run(f: impl std::future::Future<Output = ()>) {
    let context = glib::MainContext::new();
    context.with_thread_default(|| context.block_on(f)).unwrap();
}
fn fake_cli() -> Backend {
    Backend::new(
        false,
        format!("{}/tests/fake-pass-cli.py", env!("CARGO_MANIFEST_DIR")),
    )
}
fn secret(s: &str) -> Option<Zeroizing<String>> {
    Some(Zeroizing::new(s.into()))
}
#[test]
fn async_subprocess_contract() {
    run(async {
        let b = fake_cli();
        assert_eq!(b.info().await.unwrap(), "demo.user@example.com");
        let vaults = b.vaults().await.unwrap();
        let items = b.items(&vaults[0].share_id).await.unwrap();
        assert_eq!(items.len(), 2); // trashed fixture is filtered out
        let detail = b.detail(&items[0]).await.unwrap();
        assert_eq!(detail.item.title, "Example Mail");
        let pw = detail.fields.iter().find(|f| f.key == "password").unwrap();
        let secret = b.secret(&items[0], pw).await.unwrap();
        assert_eq!(secret.as_str(), "FAKE-single-field-secret");
        let totp = detail.fields.iter().find(|f| f.key == "totp").unwrap();
        assert_eq!(b.secret(&items[0], totp).await.unwrap().as_str(), "152470");
        let active = Rc::new(RefCell::new(None));
        let url = RefCell::new(None);
        b.login(active.clone(), None, |u| *url.borrow_mut() = Some(u))
            .await
            .unwrap();
        assert!(url.borrow().as_ref().unwrap().contains("FAKE-test-only"));
        assert!(active.borrow().is_none());
        b.logout().await.unwrap();
    });
}
#[test]
fn extra_password_is_passed_through_inherited_pipe() {
    run(async {
        let b = fake_cli();
        let active = Rc::new(RefCell::new(None));
        // The fake CLI asserts the value is absent from argv and environment
        // and reads it from /dev/fd/N.
        b.login(active.clone(), secret("FAKE-extra-password"), |_| {})
            .await
            .unwrap();
        assert_eq!(
            b.login(active.clone(), secret("FAKE-wrong"), |_| {}).await,
            Err(backend::ERR_EXTRA_PASSWORD_REJECTED)
        );
    });
}
#[test]
fn real_edits_are_gated_and_keep_secrets_off_argv() {
    run(async {
        let mut b = fake_cli();
        let item = b.items("shr_1111").await.unwrap().remove(0);
        let draft = LoginDraft {
            title: "Demo template".into(),
            password: secret("FAKE-template-password"),
            url: "https://demo.example.invalid".into(),
            ..Default::default()
        };
        assert_eq!(
            b.create_login("shr_1111", &draft).await,
            Err(backend::ERR_EDITING_DISABLED)
        );
        assert_eq!(b.trash(&item).await, Err(backend::ERR_EDITING_DISABLED));
        assert!(!b.can_edit());
        b.set_real_edits(true);
        assert!(b.can_edit());
        b.create_login("shr_1111", &draft).await.unwrap();
        let old = LoginDraft {
            title: "Example Mail".into(),
            ..Default::default()
        };
        let mut new = old.clone();
        new.title = "Renamed demo".into();
        b.update_login(&item, &old, &new).await.unwrap();
        new.password = secret("FAKE-template-password");
        assert_eq!(
            b.update_login(&item, &old, &new).await,
            Err(backend::ERR_PASSWORD_ARGV)
        );
        b.trash(&item).await.unwrap();
    });
}
#[test]
fn mock_all_item_types_and_logout() {
    run(async {
        let b = Backend::new(true, "unused");
        assert!(b.info().await.is_ok());
        let v = b.vaults().await.unwrap();
        let items = b.items(&v[0].share_id).await.unwrap();
        assert_eq!(items.len(), 9);
        for i in &items {
            assert_eq!(b.detail(i).await.unwrap().item, *i);
        }
        b.logout().await.unwrap();
        assert!(b.info().await.is_err());
        b.login(Rc::new(RefCell::new(None)), None, |_| {
            panic!("Mock must not open browser")
        })
        .await
        .unwrap();
        assert!(b.info().await.is_ok());
    });
}
#[test]
fn mock_extra_password_flow() {
    run(async {
        let mut b = Backend::new(true, "unused");
        b.set_mock_extra_password(true);
        b.logout().await.unwrap();
        let a = Rc::new(RefCell::new(None));
        assert_eq!(
            b.login(a.clone(), None, |_| {}).await,
            Err(backend::ERR_EXTRA_PASSWORD_REQUIRED)
        );
        assert_eq!(
            b.login(a.clone(), secret("wrong"), |_| {}).await,
            Err(backend::ERR_EXTRA_PASSWORD_REJECTED)
        );
        assert!(b.info().await.is_err());
        b.login(a, secret("FAKE-demo"), |_| {}).await.unwrap();
        assert!(b.info().await.is_ok());
    });
}
#[test]
fn mock_create_edit_trash() {
    run(async {
        let b = Backend::new(true, "unused");
        assert!(b.can_edit());
        let draft = LoginDraft {
            title: "  Demo new  ".into(),
            username: "demo".into(),
            password: secret("FAKE-created"),
            url: "https://new.example.invalid".into(),
            ..Default::default()
        };
        assert!(
            b.create_login("shr_1111", &LoginDraft::default())
                .await
                .is_err()
        );
        let i = b.create_login("shr_1111", &draft).await.unwrap();
        assert_eq!(i.title, "Demo new");
        assert!(b.items("shr_1111").await.unwrap().contains(&i));
        assert!(!b.items("shr_2222").await.unwrap().contains(&i));
        let d = b.detail(&i).await.unwrap();
        assert_eq!(d.urls, ["https://new.example.invalid"]);
        let pw = d.fields.iter().find(|f| f.key == "password").unwrap();
        assert_eq!(b.secret(&i, pw).await.unwrap().as_str(), "FAKE-created");
        let old = proton_pass_gtk::model::draft_from(&d);
        let mut new = old.clone();
        new.username = "renamed".into();
        let i = b.update_login(&i, &old, &new).await.unwrap();
        let d = b.detail(&i).await.unwrap();
        let user = d.fields.iter().find(|f| f.key == "username").unwrap();
        assert_eq!(user.value.as_deref(), Some("renamed"));
        // Unchanged password is kept.
        assert_eq!(b.secret(&i, pw).await.unwrap().as_str(), "FAKE-created");
        // Editing keeps TOTP and attachments of fixture logins.
        let mail = b
            .items("shr_1111")
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.title == "Example Mail")
            .unwrap();
        let md = b.detail(&mail).await.unwrap();
        let mut renamed = proton_pass_gtk::model::draft_from(&md);
        renamed.title = "Example Mail 2".into();
        let mail = b
            .update_login(&mail, &proton_pass_gtk::model::draft_from(&md), &renamed)
            .await
            .unwrap();
        let md2 = b.detail(&mail).await.unwrap();
        assert_eq!(md2.attachments.len(), 2);
        assert!(md2.fields.iter().any(|f| f.kind == Kind::Totp));
        assert_eq!(md2.fields.len(), md.fields.len());
        b.trash(&i).await.unwrap();
        assert!(!b.items("shr_1111").await.unwrap().contains(&i));
    });
}
#[test]
fn mock_totp_codes_follow_their_period() {
    run(async {
        let b = Backend::new(true, "unused");
        let i = b
            .items("shr_1111")
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.title == "Demo TOTP periods")
            .unwrap();
        let d = b.detail(&i).await.unwrap();
        let periods: Vec<u32> = d
            .fields
            .iter()
            .filter(|f| f.kind == Kind::Totp)
            .map(|f| f.period)
            .collect();
        assert_eq!(periods, [60, 15, 30]);
        for f in d.fields.iter().filter(|f| f.kind == Kind::Totp) {
            let code = b.secret(&i, f).await.unwrap();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    });
}
