// SPDX-License-Identifier: GPL-3.0-or-later
use proton_pass_gtk::{
    commands,
    model::{self, Kind},
};
fn fixture(s: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{s}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
#[test]
fn vault_list() {
    let v = model::vaults(&fixture("vault_list")).unwrap();
    assert_eq!(
        v.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
        ["Personal", "Work"]
    );
    assert_eq!(v[0].share_id, "shr_1111");
}
#[test]
fn vault_list_bad() {
    for s in ["not json", r#"{"nope":1}"#, "null"] {
        assert!(model::vaults(s).is_err());
    }
}
#[test]
fn vault_list_skips_invalid() {
    assert!(
        model::vaults(r#"{"vaults":[{}, {"share_id":""}]}"#)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn item_list() {
    let v = model::items(&fixture("item_list")).unwrap();
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].title, "Example Mail");
    assert_eq!(v[1].kind, "creditcard");
    assert_eq!(v[2].kind, "sshkey");
    assert_eq!(v[2].title, "Untitled");
    assert_eq!(v[2].state, "trashed");
}
#[test]
fn item_list_bad() {
    assert!(model::items("{}").is_err());
}
#[test]
fn variants() {
    for (a, b) in [
        ("Login", "login"),
        ("credit_card", "creditcard"),
        ("credit-card", "creditcard"),
        ("SshKey", "sshkey"),
        ("wifi", "wifi"),
    ] {
        assert_eq!(model::normalize(a), b);
    }
}
#[test]
fn login_fields() {
    let d = model::detail(&fixture("item_view_login")).unwrap();
    assert_eq!(d.item.title, "Example Mail");
    assert_eq!(d.note, "Demo note");
    assert_eq!(d.urls, ["https://mail.example.com"]);
    for key in [
        "email",
        "username",
        "password",
        "totp",
        "Recovery key",
        "Account ID",
        "Admin 2FA",
        "Renewal",
    ] {
        assert!(d.fields.iter().any(|f| f.key == key), "{key}");
    }
}
#[test]
fn login_drops_secrets() {
    secrets("item_view_login");
}
#[test]
fn card_drops_secrets() {
    secrets("item_view_card");
}
#[test]
fn custom_drops_secrets() {
    secrets("item_view_custom");
}
fn secrets(f: &str) {
    let d = model::detail(&fixture(f)).unwrap();
    let dump = format!("{d:?}");
    for marker in ["FAKE-", "FAKEFAKE", "4111FAKE", "otpauth://"] {
        assert!(!dump.contains(marker));
    }
    for f in d.fields {
        if f.kind != Kind::Text {
            assert!(f.value.is_none());
        }
    }
}
#[test]
fn card() {
    let d = model::detail(&fixture("item_view_card")).unwrap();
    assert_eq!(d.item.kind, "creditcard");
    assert_eq!(d.attachments, 1);
    for k in ["number", "verification_number", "pin"] {
        assert_eq!(
            d.fields.iter().find(|f| f.key == k).unwrap().kind,
            Kind::Hidden
        );
    }
}
#[test]
fn note() {
    let d = model::detail(&fixture("item_view_note")).unwrap();
    assert_eq!(d.item.kind, "note");
    assert_eq!(d.note, "Line one\nLine two");
    assert!(d.fields.is_empty());
}
#[test]
fn custom_sections() {
    let d = model::detail(&fixture("item_view_custom")).unwrap();
    assert_eq!(d.fields[0].key, "Admin.user");
    assert_eq!(d.fields[1].key, "Admin.pass");
    assert_eq!(d.fields[0].section, "Admin");
    assert_eq!(d.fields[1].kind, Kind::Hidden);
}
#[test]
fn totp() {
    assert_eq!(model::totp(&fixture("totp"), "totp").unwrap(), "152470");
}
#[test]
fn totp_fallback() {
    assert_eq!(model::totp(r#"{"x":"123456"}"#, "totp").unwrap(), "123456");
}
#[test]
fn totp_missing() {
    assert!(model::totp(&fixture("totp"), "missing").is_err());
}
#[test]
fn totp_bad() {
    assert!(model::totp("[]", "totp").is_err());
}
#[test]
fn info() {
    assert_eq!(
        model::info(&fixture("info")).unwrap(),
        "demo.user@example.com"
    );
}
#[test]
fn info_bad() {
    assert!(model::info("[]").is_err());
}
#[test]
fn errors_never_echo_payload() {
    for s in ["FAKE-private-payload", r#"{"item":"FAKE-secret"}"#] {
        assert!(!model::detail(s).unwrap_err().contains("FAKE"));
    }
}
#[test]
fn wifi() {
    let d = synthetic("Wifi", r#"{"ssid":"Demo","password":"FAKE-secret"}"#);
    assert_eq!(d.fields[1].kind, Kind::Hidden);
    assert!(d.fields[1].value.is_none());
}
#[test]
fn ssh() {
    let d = synthetic(
        "SshKey",
        r#"{"public_key":"public","private_key":"FAKE-secret"}"#,
    );
    assert_eq!(d.fields[1].kind, Kind::Hidden);
}
#[test]
fn identity() {
    let d = synthetic(
        "Identity",
        r#"{"full_name":"Demo","street_address":"Demo St","passport_number":"FAKE-secret"}"#,
    );
    assert!(d.fields.iter().any(|f| f.key == "address"));
    assert_eq!(
        d.fields
            .iter()
            .find(|f| f.key == "passport_number")
            .unwrap()
            .value,
        None
    );
}
#[test]
fn alias() {
    let d = synthetic("Alias", "null");
    assert_eq!(d.item.kind, "alias");
}
#[test]
fn unknown_type() {
    assert_eq!(synthetic("FutureType", "{}").item.kind, "futuretype");
}
fn synthetic(tag: &str, payload: &str) -> model::Detail {
    model::detail(&format!(
        r#"{{"item":{{"id":"i","content":{{"title":"Demo","content":{{"{tag}":{payload}}}}}}}}}"#
    ))
    .unwrap()
}
#[test]
fn no_show_secrets() {
    for a in [
        commands::info(),
        commands::vaults(),
        commands::items("s"),
        commands::view("s", "i"),
        commands::field("s", "i", "password"),
        commands::totp("s", "i", "totp"),
    ] {
        assert!(!a.iter().any(|s| s == "--show-secrets"));
    }
}
#[test]
fn info_json() {
    assert_eq!(commands::info(), ["info", "--output", "json"]);
}
#[test]
fn active_only() {
    assert!(
        commands::items("s")
            .windows(2)
            .any(|v| v == ["--filter-state", "active"])
    );
}
#[test]
fn field_argv() {
    assert_eq!(
        commands::field("s", "i", "password"),
        [
            "item",
            "view",
            "--share-id",
            "s",
            "--item-id",
            "i",
            "--field",
            "password"
        ]
    );
}
#[test]
fn totp_argv() {
    assert!(commands::totp("s", "i", "totp").ends_with(&["--output".into(), "json".into()]));
}
#[test]
fn ids_not_shell_expanded() {
    assert!(commands::view("a;rm -rf /", "$(id)").contains(&"$(id)".into()));
}
#[test]
fn login_url() {
    assert_eq!(
        commands::login_url("Visit https://account.proton.me/desktop/login?x=1").unwrap(),
        "https://account.proton.me/desktop/login?x=1"
    );
}
#[test]
fn reject_foreign_login_url() {
    for s in [
        "http://account.proton.me/desktop/login?x=1",
        "https://evil.test/desktop/login",
        "https://account.proton.me.evil.test/desktop/login?x=1",
        "https://account.proton.me/desktop/login.evil",
    ] {
        assert!(commands::login_url(s).is_none());
    }
}
