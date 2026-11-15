// SPDX-License-Identifier: GPL-3.0-or-later
//! Feature-parity additions: generator, item drafts/templates, search
//! index, sharing/invite parsing, new CLI argument builders, preferences.
use proton_pass_gtk::{commands, generator, model, prefs::Prefs};
use zeroize::Zeroizing;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn generator_respects_options_and_classes() {
    let o = generator::RandomOptions {
        length: 32,
        uppercase: true,
        numbers: true,
        symbols: false,
        avoid_ambiguous: true,
    };
    for _ in 0..50 {
        let p = generator::random(&o).unwrap();
        assert_eq!(p.chars().count(), 32);
        assert!(p.chars().any(|c| c.is_ascii_uppercase()));
        assert!(p.chars().any(|c| c.is_ascii_lowercase()));
        assert!(p.chars().any(|c| c.is_ascii_digit()));
        assert!(!p.chars().any(|c| generator::SYMBOLS.contains(c)));
        assert!(!p.chars().any(|c| generator::AMBIGUOUS.contains(c)));
    }
    // Length is clamped to the supported range.
    let short = generator::RandomOptions {
        length: 1,
        ..Default::default()
    };
    assert_eq!(
        generator::random(&short).unwrap().len(),
        generator::MIN_LENGTH as usize
    );
    let long = generator::RandomOptions {
        length: 999,
        ..Default::default()
    };
    assert_eq!(
        generator::random(&long).unwrap().len(),
        generator::MAX_LENGTH as usize
    );
}
#[test]
fn generator_is_unbiased_and_fails_closed() {
    // Byte 255 is outside the rejection limit for a 26-letter alphabet and
    // must be skipped, never folded into the range.
    let lower = generator::RandomOptions {
        length: 4,
        uppercase: false,
        numbers: false,
        symbols: false,
        avoid_ambiguous: false,
    };
    let mut bytes = [255u8, 0, 255, 1, 2, 25].into_iter();
    let p = generator::random_with(&lower, || bytes.next()).unwrap();
    assert_eq!(p.as_str(), "abcz");
    // An exhausted source yields None instead of a weak value.
    let mut few = [0u8, 1].into_iter();
    assert!(generator::random_with(&lower, || few.next()).is_none());
}
#[test]
fn generator_entropy_and_strength() {
    let d = generator::RandomOptions::default();
    let bits = generator::random_entropy_bits(&d);
    assert!(bits > 100.0, "{bits}");
    assert_eq!(
        generator::Strength::from_bits(bits),
        generator::Strength::VeryStrong
    );
    let weak = generator::RandomOptions {
        length: 6,
        uppercase: false,
        numbers: false,
        symbols: false,
        avoid_ambiguous: false,
    };
    assert_eq!(
        generator::Strength::from_bits(generator::random_entropy_bits(&weak)),
        generator::Strength::Weak
    );
    let phrase = generator::PassphraseOptions::default();
    assert!(generator::passphrase_entropy_bits(&phrase) > 64.0);
    let demo = generator::demo_passphrase(&phrase).unwrap();
    assert_eq!(demo.split('-').count(), 5);
}
#[test]
fn passkeys_keep_only_metadata() {
    let d = model::detail(&fixture("item_view_login")).unwrap();
    assert_eq!(d.passkeys.len(), 1);
    assert_eq!(d.passkeys[0].rp_name, "Example Mail");
    assert_eq!(d.passkeys[0].user_name, "demo_user");
    assert_eq!(d.passkeys[0].domain, "mail.example.com");
    assert!(!format!("{d:?}").contains("FAKE-passkey-private-material"));
}
#[test]
fn new_item_types_parse() {
    let id = model::detail(&fixture("item_view_identity")).unwrap();
    assert_eq!(id.item.kind, "identity");
    assert!(
        id.fields
            .iter()
            .any(|f| f.key == "address" && f.value.as_deref() == Some("Demoveien 1"))
    );
    let pass = id
        .fields
        .iter()
        .find(|f| f.key == "passport_number")
        .unwrap();
    assert_eq!(pass.value, None);
    let ssh = model::detail(&fixture("item_view_sshkey")).unwrap();
    assert_eq!(ssh.item.kind, "sshkey");
    assert!(
        ssh.fields
            .iter()
            .any(|f| f.key == "private_key" && f.value.is_none())
    );
    let wifi = model::detail(&fixture("item_view_wifi")).unwrap();
    assert_eq!(model::subtitle_of(&wifi), "Demo-Guest");
    let alias = model::detail(&fixture("item_view_alias")).unwrap();
    assert_eq!(alias.item.kind, "alias");
    assert_eq!(
        model::subtitle_of(&alias),
        "newsletter.demo123@passinbox.invalid"
    );
}
#[test]
fn search_index_never_contains_hidden_values() {
    let d = model::detail(&fixture("item_view_login")).unwrap();
    let t = model::search_text(&d);
    for want in [
        "demo_user",
        "mail.example.com",
        "demo note",
        "12345",
        "example mail",
    ] {
        assert!(t.contains(want), "{want} missing from {t}");
    }
    for secret in [
        "fake-test-password-123",
        "fake-hidden-value",
        "fakefake",
        "passkey-private",
    ] {
        assert!(!t.contains(secret), "{secret} leaked into the index");
    }
    let item = &d.item;
    assert!(model::matches(item, Some(&t), "DEMO_user"));
    assert!(model::matches(item, Some(&t), "mail demo_user"));
    assert!(!model::matches(item, None, "demo_user"));
    assert!(model::matches(item, None, "login"));
    assert!(!model::matches(item, Some(&t), "nothing-like-this"));
}
#[test]
fn drafts_build_stdin_templates_per_type() {
    let card = model::ItemDraft {
        kind: "creditcard".into(),
        title: "Demo".into(),
        values: vec![
            ("number".into(), Zeroizing::new("FAKE-4111".into())),
            (
                "verification_number".into(),
                Zeroizing::new("FAKE-1".into()),
            ),
            ("expiration_date".into(), Zeroizing::new("2030-12".into())),
        ],
        ..Default::default()
    };
    card.validate().unwrap();
    let v: serde_json::Value = serde_json::from_str(&card.template()).unwrap();
    assert_eq!(v["number"], "FAKE-4111");
    assert_eq!(v["cvv"], "FAKE-1"); // pass-cli's template key
    assert_eq!(v["expiration_date"], "2030-12");
    assert_eq!(v["extra_fields"], serde_json::json!([]));
    let mut bad = card.clone();
    bad.values
        .push(("expiration_date".into(), Zeroizing::new("12/30".into())));
    bad.values.remove(2);
    assert!(bad.validate().is_err());

    let wifi = model::ItemDraft {
        kind: "wifi".into(),
        title: "Net".into(),
        ..Default::default()
    };
    assert!(wifi.validate().is_err(), "SSID is required");
    let id = model::ItemDraft {
        kind: "identity".into(),
        title: "Me".into(),
        values: vec![("address".into(), Zeroizing::new("Demoveien 1".into()))],
        ..Default::default()
    };
    let v: serde_json::Value = serde_json::from_str(&id.template()).unwrap();
    assert_eq!(v["street_address"], "Demoveien 1");
    let custom = model::ItemDraft {
        kind: "custom".into(),
        title: "Thing".into(),
        values: vec![("custom_hidden".into(), Zeroizing::new("FAKE-x".into()))],
        ..Default::default()
    };
    let v: serde_json::Value = serde_json::from_str(&custom.template()).unwrap();
    assert_eq!(v["extra_fields"][0]["kind"], "hidden");
    assert_eq!(v["sections"], serde_json::json!([]));
    let alias = model::ItemDraft {
        kind: "alias".into(),
        extra: "shop.news".into(),
        ..Default::default()
    };
    alias.validate().unwrap();
    for bad in ["", "-x", "a b", ".x", "æøå"] {
        let a = model::ItemDraft {
            kind: "alias".into(),
            extra: bad.into(),
            ..Default::default()
        };
        assert!(a.validate().is_err(), "{bad}");
    }
}
#[test]
fn updates_never_send_hidden_values() {
    let old = model::ItemDraft {
        kind: "wifi".into(),
        title: "Net".into(),
        values: vec![("ssid".into(), Zeroizing::new("Old".into()))],
        ..Default::default()
    };
    let mut new = old.clone();
    new.values = vec![
        ("ssid".into(), Zeroizing::new("New".into())),
        ("password".into(), Zeroizing::new("FAKE-new".into())),
    ];
    let changed = new.changed_fields(&old);
    assert_eq!(changed, vec![("ssid".to_string(), "New".to_string())]);
    assert!(new.has_secret_change());
    let login_old = model::ItemDraft {
        kind: "login".into(),
        title: "A".into(),
        ..Default::default()
    };
    let mut login_new = login_old.clone();
    login_new.url = "https://example.com".into();
    assert_eq!(
        login_new.changed_fields(&login_old),
        vec![("urls".to_string(), "https://example.com".to_string())]
    );
}
#[test]
fn members_and_invites_parse() {
    let m = model::members(
        r#"[{"member_share_id":"m1","email":"owner@example.invalid","name":"O","is_group_share":false,"role":"Owner","target_type":"Vault"},
            {"member_share_id":"m2","email":"x@example.invalid","name":"","is_group_share":false,"role":{"Custom":{"name":"Auditor","permission":1}},"target_type":"Vault"},
            {"member_share_id":"","email":"skip@example.invalid"}]"#,
    )
    .unwrap();
    assert_eq!(m.len(), 2);
    assert!(m[0].is_owner());
    assert_eq!(m[1].role, "Auditor");
    let i = model::invites(r#"{"invites":[{"token":"t1","invite_type":"vault","inviter":"a@example.invalid","name":"Family"},{"token":"t2","invite_type":"item","inviter":"b@example.invalid"}]}"#).unwrap();
    assert_eq!(i.len(), 2);
    assert_eq!(i[0].name, "Family");
    assert_eq!(i[1].name, "");
    assert_eq!(
        model::generated_password(r#"{"password":"FAKE-gen"}"#)
            .unwrap()
            .as_str(),
        "FAKE-gen"
    );
    assert!(model::generated_password("{}").is_err());
    assert_eq!(
        model::created_alias(r#"{"id":"itm_x","alias":"a.b@passinbox.invalid"}"#).unwrap(),
        ("itm_x".to_string(), "a.b@passinbox.invalid".to_string())
    );
}
#[test]
fn new_commands_carry_no_secret_values() {
    assert_eq!(
        commands::create_item("credit-card", "S"),
        [
            "item",
            "create",
            "credit-card",
            "--share-id",
            "S",
            "--from-template",
            "-"
        ]
    );
    assert_eq!(
        commands::trashed_items("S")[4..6],
        ["--filter-state", "trashed"]
    );
    assert_eq!(
        commands::untrash("S", "I"),
        ["item", "untrash", "--share-id", "S", "--item-id", "I"]
    );
    assert_eq!(commands::delete("S", "I")[..2], ["item", "delete"]);
    assert_eq!(
        commands::vault_share("S", "a@b.c", "editor"),
        [
            "vault",
            "share",
            "--share-id",
            "S",
            "--role",
            "editor",
            "a@b.c"
        ]
    );
    assert_eq!(
        commands::invite_respond(false, "T"),
        ["invite", "reject", "T"]
    );
    let e = commands::export_archive("/tmp/x.html", true);
    assert!(e.contains(&"--include-attachments".to_string()));
    assert!(
        !e.iter()
            .any(|a| a.contains("encrypt") || a.contains("password"))
    );
    let p = commands::generate_passphrase(6, "spaces", false, true);
    assert_eq!(
        p,
        [
            "password",
            "generate",
            "passphrase",
            "--count",
            "6",
            "--separator",
            "spaces",
            "--capitalise",
            "false",
            "--numbers",
            "true",
            "--output",
            "json"
        ]
    );
    assert_eq!(
        commands::generate_ssh_key("S", "T", "ed25519")
            .last()
            .unwrap(),
        "ed25519"
    );
    assert_eq!(
        commands::move_item("A", "I", "B"),
        [
            "item",
            "move",
            "--from-share-id",
            "A",
            "--item-id",
            "I",
            "--to-share-id",
            "B"
        ]
    );
}
#[test]
fn generator_and_search_preferences_round_trip() {
    let mut p = Prefs {
        deep_search: true,
        gen_passphrase: true,
        gen_length: 33,
        gen_symbols: false,
        gen_words: 7,
        gen_separator: "spaces".into(),
        ..Default::default()
    };
    assert_eq!(Prefs::parse(&p.serialize()), p);
    let clamped =
        Prefs::parse("generator-length=2\ngenerator-words=99\ngenerator-separator=evil\n");
    assert_eq!(clamped.gen_length, generator::MIN_LENGTH);
    assert_eq!(clamped.gen_words, generator::MAX_WORDS);
    assert_eq!(clamped.gen_separator, "hyphens");
    p.gen_passphrase = false;
    assert!(p.serialize().contains("generator-mode=random"));
    assert!(!Prefs::default().deep_search);
}
