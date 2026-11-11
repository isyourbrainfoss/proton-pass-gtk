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
    assert_eq!(d.attachments.len(), 1);
    assert_eq!(d.attachments[0].id, "att_1");
    assert_eq!(d.attachments[0].name, "demo-receipt.pdf");
    assert_eq!(d.attachments[0].size, 49152);
    assert_eq!(model::human_size(d.attachments[0].size), "48.0 KB");
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
#[test]
fn totp_periods() {
    use model::totp_period as p;
    assert_eq!(p("otpauth://totp/x?secret=FAKE&period=60"), 60);
    assert_eq!(p("otpauth://totp/x?period=15&secret=FAKE"), 15);
    assert_eq!(p("OTPAUTH://TOTP/x?PERIOD=45"), 45);
    assert_eq!(p("otpauth://totp/x?secret=FAKE"), 30);
    assert_eq!(p("otpauth://totp/x?secret=FAKE&period=0"), 30);
    assert_eq!(p("otpauth://totp/x?secret=FAKE&period=999"), 30);
    assert_eq!(p("otpauth://totp/x?secret=FAKE&period=abc"), 30);
    assert_eq!(p("otpauth://totp/x?secret=FAKE#period=60"), 30);
    assert_eq!(p("otpauth://hotp/x?period=60"), 30);
    assert_eq!(p("FAKEFAKEFAKEFAKE"), 30);
    assert_eq!(p(""), 30);
}
#[test]
fn totp_countdown() {
    use model::{totp_remaining as r, totp_step as s};
    assert_eq!(r(0, 30), 30);
    assert_eq!(r(29, 30), 1);
    assert_eq!(r(30, 30), 30);
    assert_eq!(r(59, 60), 1);
    assert_eq!(r(60, 60), 60);
    assert_eq!(r(1_000_000_007, 15), 15 - (1_000_000_007 % 15));
    assert_eq!(s(59, 60), 0);
    assert_eq!(s(60, 60), 1);
    assert_eq!(s(14, 15), 0);
    assert_eq!(s(15, 15), 1);
    assert_eq!(r(-1, 30), 1);
    assert_eq!(r(10, 0), 1); // guarded, never divides by zero
}
#[test]
fn totp_fields_keep_only_period() {
    let d = model::detail(&fixture("item_view_totp_periods")).unwrap();
    let t: Vec<_> = d.fields.iter().filter(|f| f.kind == Kind::Totp).collect();
    assert_eq!(t.len(), 3);
    assert_eq!((t[0].key.as_str(), t[0].period), ("totp", 60));
    assert_eq!((t[1].label.as_str(), t[1].period), ("Legacy 2FA", 15));
    assert_eq!((t[2].label.as_str(), t[2].period), ("Backup 2FA", 30));
    let dump = format!("{d:?}");
    assert!(!dump.contains("FAKEFAKE") && !dump.contains("otpauth://"));
    let login = model::detail(&fixture("item_view_login")).unwrap();
    assert!(
        login
            .fields
            .iter()
            .filter(|f| f.kind == Kind::Totp)
            .all(|f| f.period == 30)
    );
}
#[test]
fn login_template_and_changes() {
    use model::LoginDraft;
    let d = LoginDraft {
        title: " Demo ".into(),
        username: "user".into(),
        password: Some(zeroize::Zeroizing::new("FAKE-pw".into())),
        url: "https://demo.example.invalid".into(),
        ..Default::default()
    };
    let t: serde_json::Value = serde_json::from_str(&d.template()).unwrap();
    assert_eq!(t["title"], "Demo");
    assert_eq!(t["username"], "user");
    assert_eq!(t["email"], serde_json::Value::Null);
    assert_eq!(t["password"], "FAKE-pw");
    assert_eq!(t["urls"][0], "https://demo.example.invalid");
    assert!(t["extra_fields"].as_array().unwrap().is_empty());
    let mut e = d.clone();
    e.title = "Renamed".into();
    e.password = Some(zeroize::Zeroizing::new("FAKE-other".into()));
    let changed = e.changed_fields(&d);
    assert_eq!(changed, [("title", "Renamed".to_string())]);
    assert!(LoginDraft::default().validate().is_err());
    let mut bad = d.clone();
    bad.url = "nonsense".into();
    assert!(bad.validate().is_err());
    let detail = model::detail(&fixture("item_view_login")).unwrap();
    let prefill = model::draft_from(&detail);
    assert_eq!(prefill.title, "Example Mail");
    assert_eq!(prefill.username, "demo_user");
    assert_eq!(prefill.email, "demo.user@example.com");
    assert_eq!(prefill.url, "https://mail.example.com");
    assert!(prefill.password.is_none());
}
#[test]
fn edit_commands_carry_no_secret_values() {
    assert_eq!(
        commands::create_login("S"),
        [
            "item",
            "create",
            "login",
            "--share-id",
            "S",
            "--from-template",
            "-"
        ]
    );
    assert_eq!(
        commands::update("S", "I", &[("title", "T".into())]),
        [
            "item",
            "update",
            "--share-id",
            "S",
            "--item-id",
            "I",
            "--field",
            "title=T"
        ]
    );
    assert_eq!(
        commands::trash("S", "I"),
        ["item", "trash", "--share-id", "S", "--item-id", "I"]
    );
    assert_eq!(
        commands::attachment_download("S", "I", "A", "/tmp/x")
            .last()
            .unwrap(),
        "/tmp/x"
    );
}
#[test]
fn prefs_roundtrip_and_clamping() {
    use proton_pass_gtk::prefs::{ColorScheme, Prefs};
    let d = Prefs::default();
    assert_eq!((d.clipboard_seconds, d.auto_lock_minutes), (30, 5));
    assert_eq!(Prefs::parse(&d.serialize()), d);
    let p = Prefs::parse(
        "clipboard-seconds=1000\nauto-lock-minutes=7\nwidth=10\nheight=99999\nmaximized=true\n\
         last-vault=shr_2222\ncolor-scheme=dark\nextra-password=true\njunk\n# c=1\nunknown=1",
    );
    assert_eq!(p.clipboard_seconds, 90);
    assert_eq!(p.auto_lock_minutes, 5);
    assert_eq!((p.width, p.height, p.maximized), (320, 8192, true));
    assert_eq!(p.last_vault, "shr_2222");
    assert_eq!(p.color_scheme, ColorScheme::Dark);
    assert!(p.extra_password);
    assert_eq!(
        Prefs::parse("clipboard-seconds=-5\ncolor-scheme=pink\nlast-vault=a b").clipboard_seconds,
        30
    );
    assert_eq!(Prefs::parse("last-vault=a b").last_vault, "");
    assert_eq!(Prefs::parse("auto-lock-minutes=0").auto_lock_minutes, 0);
    let dir = std::env::temp_dir().join(format!("ppg-prefs-{}", std::process::id()));
    let path = dir.join("settings.ini");
    p.save(&path).unwrap();
    assert_eq!(Prefs::load(&path), p);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(Prefs::load(&dir.join("missing.ini")), Prefs::default());
    std::fs::remove_dir_all(dir).unwrap();
}
