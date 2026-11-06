// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(feature = "ui")]
use gtk::glib;
use proton_pass_gtk::backend::Backend;
use std::{cell::RefCell, rc::Rc};
#[test]
fn async_subprocess_contract() {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let b = Backend::new(
                    false,
                    format!("{}/tests/fake-pass-cli.py", env!("CARGO_MANIFEST_DIR")),
                );
                assert_eq!(b.info().await.unwrap(), "demo.user@example.com");
                let vaults = b.vaults().await.unwrap();
                let items = b.items(&vaults[0].share_id).await.unwrap();
                assert_eq!(items.len(), 2); // trashed fixture is filtered out
                let detail = b.detail(&items[0]).await.unwrap();
                assert_eq!(detail.item.title, "Example Mail");
                let secret = b.secret(&items[0], "password", false).await.unwrap();
                assert_eq!(secret.as_str(), "FAKE-single-field-secret");
                assert_eq!(
                    b.secret(&items[0], "totp", true).await.unwrap().as_str(),
                    "152470"
                );
                let active = Rc::new(RefCell::new(None));
                let url = RefCell::new(None);
                b.login(active.clone(), |u| *url.borrow_mut() = Some(u))
                    .await
                    .unwrap();
                assert!(url.borrow().as_ref().unwrap().contains("FAKE-test-only"));
                assert!(active.borrow().is_none());
                b.logout().await.unwrap();
            })
        })
        .unwrap();
}
#[test]
fn mock_all_item_types_and_logout() {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let b = Backend::new(true, "unused");
                assert!(b.info().await.is_ok());
                let v = b.vaults().await.unwrap();
                let items = b.items(&v[0].share_id).await.unwrap();
                assert_eq!(items.len(), 8);
                for i in &items {
                    assert_eq!(b.detail(i).await.unwrap().item, *i);
                }
                b.logout().await.unwrap();
                assert!(b.info().await.is_err());
                b.login(Rc::new(RefCell::new(None)), |_| {
                    panic!("Mock must not open browser")
                })
                .await
                .unwrap();
                assert!(b.info().await.is_ok());
            })
        })
        .unwrap();
}
