// SPDX-License-Identifier: GPL-3.0-or-later
//! Only metadata is retained. Hidden/TOTP values are discarded at the boundary.
use serde_json::Value;
use zeroize::Zeroize;

struct Json(Value);
impl std::ops::Deref for Json {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl Drop for Json {
    fn drop(&mut self) {
        fn wipe(v: &mut Value) {
            match v {
                Value::String(s) => s.zeroize(),
                Value::Array(a) => a.iter_mut().for_each(wipe),
                Value::Object(o) => o.values_mut().for_each(wipe),
                _ => (),
            }
        }
        wipe(&mut self.0);
    }
}

pub type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vault {
    pub name: String,
    pub share_id: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub share_id: String,
    pub title: String,
    pub kind: String,
    pub state: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Hidden,
    Totp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: Kind,
    pub value: Option<String>,
    pub section: String,
    /// TOTP step in seconds (only meaningful for `Kind::Totp`). Parsed from the
    /// otpauth URI at the boundary; the URI and its seed are never retained.
    pub period: u32,
}
/// Attachment metadata only; contents are never loaded into memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub item: Item,
    pub fields: Vec<Field>,
    pub note: String,
    pub urls: Vec<String>,
    pub alias: String,
    pub attachments: Vec<Attachment>,
    /// Passkey metadata only; the private key material is never retained.
    pub passkeys: Vec<Passkey>,
}
/// Display metadata of a passkey stored in a login.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passkey {
    pub domain: String,
    pub rp_name: String,
    pub user_name: String,
    pub create_time: i64,
}
fn json(s: &str) -> Result<Json> {
    serde_json::from_str(s)
        .map(Json)
        .map_err(|_| "Invalid JSON from pass-cli")
}
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
pub fn info(s: &str) -> Result<String> {
    let v = json(s)?;
    if !v.is_object() {
        return Err("Invalid session metadata");
    }
    Ok(["email", "username", "personal_access_token_name"]
        .iter()
        .map(|k| text(&v[k]))
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| "Proton account".into()))
}
pub fn vaults(s: &str) -> Result<Vec<Vault>> {
    let v = json(s)?;
    let a = v
        .get("vaults")
        .unwrap_or(&v)
        .as_array()
        .ok_or("Missing vaults array")?;
    Ok(a.iter()
        .filter(|v| v["share_id"].as_str().is_some_and(|s| !s.is_empty()))
        .map(|v| Vault {
            name: nonempty(text(&v["name"]), "Untitled vault"),
            share_id: text(&v["share_id"]),
        })
        .collect())
}
fn nonempty(s: String, fallback: &str) -> String {
    if s.is_empty() { fallback.into() } else { s }
}
fn summary(v: &Value) -> Item {
    Item {
        id: text(&v["id"]),
        share_id: text(&v["share_id"]),
        title: nonempty(text(&v["title"]), "Untitled"),
        kind: normalize(&text(&v["item_type"])),
        state: text(&v["state"]).to_lowercase(),
    }
}
pub fn items(s: &str) -> Result<Vec<Item>> {
    let v = json(s)?;
    let a = v
        .get("items")
        .unwrap_or(&v)
        .as_array()
        .ok_or("Missing items array")?;
    Ok(a.iter()
        .filter(|v| v["id"].as_str().is_some_and(|s| !s.is_empty()))
        .map(summary)
        .collect())
}
fn untag(v: &Value) -> Result<(&str, &Value)> {
    if let Some(s) = v.as_str() {
        return Ok((s, &Value::Null));
    }
    if let Some(o) = v.as_object().filter(|o| o.len() == 1) {
        let (k, v) = o.iter().next().unwrap();
        return Ok((k, v));
    }
    Err("Unexpected item variant")
}
fn add(fields: &mut Vec<Field>, payload: &Value, key: &str, cli_key: &str, kind: Kind) {
    if payload[key].is_null() || payload[key].as_str() == Some("") {
        return;
    }
    fields.push(Field {
        key: cli_key.into(),
        label: key.replace('_', " "),
        value: if kind == Kind::Text {
            Some(text(&payload[key]))
        } else {
            None
        },
        period: period_of(&kind, &payload[key]),
        kind,
        section: String::new(),
    });
}
fn period_of(kind: &Kind, v: &Value) -> u32 {
    if *kind == Kind::Totp {
        totp_period(v.as_str().unwrap_or_default())
    } else {
        0
    }
}
/// Standard TOTP step used when a URI has no (valid) `period` parameter.
pub const DEFAULT_TOTP_PERIOD: u32 = 30;
/// Extract only the `period` parameter of an `otpauth://totp/` URI.
/// Bare base32 seeds and malformed values fall back to 30 seconds. Values
/// outside 1–300 s are rejected as implausible. The seed is not copied.
pub fn totp_period(uri: &str) -> u32 {
    let Some(rest) = uri
        .get(..15)
        .filter(|p| p.eq_ignore_ascii_case("otpauth://totp/"))
        .map(|_| &uri[15..])
    else {
        return DEFAULT_TOTP_PERIOD;
    };
    let query = rest.split_once('?').map_or("", |(_, q)| q);
    let query = query.split('#').next().unwrap_or_default();
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.eq_ignore_ascii_case("period"))
        .and_then(|(_, v)| v.trim().parse::<u32>().ok())
        .filter(|p| (1..=300).contains(p))
        .unwrap_or(DEFAULT_TOTP_PERIOD)
}
/// Seconds until the code for `period` rolls over at Unix time `now`.
pub fn totp_remaining(now: i64, period: u32) -> u32 {
    let p = i64::from(period.max(1));
    (p - now.rem_euclid(p)) as u32
}
/// Index of the TOTP time step containing Unix time `now`.
pub fn totp_step(now: i64, period: u32) -> i64 {
    now.div_euclid(i64::from(period.max(1)))
}
fn extras(fields: &mut Vec<Field>, a: &Value, section: &str, prefix: &str) {
    if let Some(a) = a.as_array() {
        for e in a {
            let name = text(&e["name"]);
            if name.is_empty() {
                continue;
            }
            if let Ok((tag, v)) = untag(&e["content"]) {
                let kind = match normalize(tag).as_str() {
                    "hidden" => Kind::Hidden,
                    "totp" => Kind::Totp,
                    _ => Kind::Text,
                };
                fields.push(Field {
                    key: if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    },
                    label: name,
                    value: if kind == Kind::Text {
                        Some(text(v))
                    } else {
                        None
                    },
                    period: period_of(&kind, v),
                    kind,
                    section: section.into(),
                });
            }
        }
    }
}
fn sections(fields: &mut Vec<Field>, a: &Value) {
    if let Some(a) = a.as_array() {
        for sec in a {
            let name = text(&sec["section_name"]);
            extras(fields, &sec["section_fields"], &name, &name);
        }
    }
}
pub fn detail(s: &str) -> Result<Detail> {
    let v = json(s)?;
    let i = v.get("item").unwrap_or(&v);
    if !i.is_object() || !i["content"].is_object() {
        return Err("Missing item content");
    }
    let c = &i["content"];
    let (tag, p) = untag(&c["content"])?;
    let kind = normalize(tag);
    let mut item = summary(i);
    item.title = nonempty(text(&c["title"]), "Untitled");
    item.kind = kind.clone();
    let mut fields = vec![];
    let spec: &[(&str, &str, Kind)] = match kind.as_str() {
        "login" => &[
            ("email", "email", Kind::Text),
            ("username", "username", Kind::Text),
            ("password", "password", Kind::Hidden),
            ("totp_uri", "totp", Kind::Totp),
        ],
        "creditcard" => &[
            ("cardholder_name", "cardholder_name", Kind::Text),
            ("number", "number", Kind::Hidden),
            ("expiration_date", "expiration_date", Kind::Text),
            ("verification_number", "verification_number", Kind::Hidden),
            ("pin", "pin", Kind::Hidden),
        ],
        "wifi" => &[
            ("ssid", "ssid", Kind::Text),
            ("password", "password", Kind::Hidden),
        ],
        "sshkey" => &[
            ("public_key", "public_key", Kind::Text),
            ("private_key", "private_key", Kind::Hidden),
        ],
        _ => &[],
    };
    for (k, c, kind) in spec {
        add(&mut fields, p, k, c, kind.clone());
    }
    if kind == "identity" {
        for k in [
            "full_name",
            "email",
            "phone_number",
            "birthdate",
            "organization",
            "street_address",
            "zip_or_postal_code",
            "city",
            "state_or_province",
            "country_or_region",
            "company",
            "job_title",
            "website",
            "social_security_number",
            "passport_number",
            "license_number",
        ] {
            let secret = [
                "social_security_number",
                "passport_number",
                "license_number",
            ]
            .contains(&k);
            add(
                &mut fields,
                p,
                k,
                if k == "street_address" { "address" } else { k },
                if secret { Kind::Hidden } else { Kind::Text },
            );
        }
        for k in [
            "extra_personal_details",
            "extra_address_details",
            "extra_contact_details",
            "extra_work_details",
        ] {
            extras(&mut fields, &p[k], "", "");
        }
        sections(&mut fields, &p["extra_sections"]);
    }
    sections(&mut fields, &p["sections"]);
    extras(&mut fields, &c["extra_fields"], "Custom fields", "");
    Ok(Detail {
        item,
        fields,
        note: text(&c["note"]),
        urls: p["urls"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        alias: text(&i["alias_email"]),
        attachments: attachments(&v["attachments"]),
        passkeys: passkeys(&p["passkeys"]),
    })
}
fn passkeys(v: &Value) -> Vec<Passkey> {
    let Some(a) = v.as_array() else {
        return vec![];
    };
    a.iter()
        .filter(|p| p.is_object())
        .map(|p| Passkey {
            domain: text(&p["domain"]),
            rp_name: nonempty(text(&p["rp_name"]), "Passkey"),
            user_name: {
                let u = text(&p["user_name"]);
                if u.is_empty() {
                    text(&p["user_display_name"])
                } else {
                    u
                }
            },
            create_time: p["create_time"].as_i64().unwrap_or(0),
        })
        .collect()
}
fn attachments(v: &Value) -> Vec<Attachment> {
    let Some(a) = v.as_array() else {
        return vec![];
    };
    a.iter()
        .map(|a| {
            let c = if a["content"].is_object() {
                &a["content"]
            } else {
                a
            };
            Attachment {
                id: text(&a["id"]),
                name: nonempty(text(&c["name"]), "Attachment"),
                size: a["size"].as_u64().unwrap_or(0),
                mime_type: text(&c["mime_type"]),
            }
        })
        .collect()
}
/// Human-readable size with binary units, like pass-cli's own output.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// Editable subset of a login. Secret values are zeroized when dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginDraft {
    pub title: String,
    pub username: String,
    pub email: String,
    /// `None` keeps the existing password when updating.
    pub password: Option<zeroize::Zeroizing<String>>,
    pub url: String,
    pub note: String,
}
impl LoginDraft {
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err("A title is required");
        }
        if !self.url.trim().is_empty() && !self.url.trim().contains('.') {
            return Err("The website does not look like an address");
        }
        Ok(())
    }
    /// JSON for `pass-cli item create login --from-template -`, sent on stdin
    /// so that secret values never appear in the process argument list.
    pub fn template(&self) -> zeroize::Zeroizing<String> {
        let opt = |s: &str| {
            if s.trim().is_empty() {
                Value::Null
            } else {
                Value::String(s.trim().into())
            }
        };
        let mut v = serde_json::json!({
            "title": self.title.trim(),
            "username": opt(&self.username),
            "email": opt(&self.email),
            "password": self.password.as_deref().map_or(Value::Null, |p| Value::String(p.to_string())),
            "totp_uri": Value::Null,
            "urls": if self.url.trim().is_empty() { vec![] } else { vec![self.url.trim().to_owned()] },
            "note": self.note,
            "extra_fields": [],
        });
        let out = zeroize::Zeroizing::new(v.to_string());
        if let Value::Object(o) = &mut v {
            if let Some(Value::String(p)) = o.get_mut("password") {
                p.zeroize();
            }
        }
        out
    }
    /// Non-secret `field=value` pairs changed relative to `old`, for
    /// `pass-cli item update`. The password is deliberately excluded: that
    /// command only accepts values as command-line arguments.
    pub fn changed_fields(&self, old: &LoginDraft) -> Vec<(&'static str, String)> {
        let mut out = vec![];
        for (k, new, old) in [
            ("title", &self.title, &old.title),
            ("username", &self.username, &old.username),
            ("email", &self.email, &old.email),
            ("urls", &self.url, &old.url),
            ("note", &self.note, &old.note),
        ] {
            if new.trim() != old.trim() {
                out.push((k, new.trim().to_owned()));
            }
        }
        out
    }
}
/// Pre-fill an editor from a parsed detail (no secrets are available here).
pub fn draft_from(d: &Detail) -> LoginDraft {
    let get = |k: &str| {
        d.fields
            .iter()
            .find(|f| f.key == k && f.section.is_empty())
            .and_then(|f| f.value.clone())
            .unwrap_or_default()
    };
    LoginDraft {
        title: d.item.title.clone(),
        username: get("username"),
        email: get("email"),
        password: None,
        url: d.urls.first().cloned().unwrap_or_default(),
        note: d.note.clone(),
    }
}
pub fn totp(s: &str, field: &str) -> Result<String> {
    let v = json(s)?;
    let o = v.as_object().ok_or("Invalid TOTP response")?;
    o.get(field)
        .or_else(|| {
            if o.len() == 1 {
                o.values().next()
            } else {
                None
            }
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or("No code for this field")
}

/// Human-readable name of a normalized item type.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "login" => "Login",
        "note" => "Note",
        "creditcard" => "Credit card",
        "identity" => "Identity",
        "sshkey" => "SSH key",
        "wifi" => "Wi-Fi network",
        "alias" => "Alias",
        "custom" => "Custom item",
        _ => "Item",
    }
}
/// Symbolic icon for an item type (all in the Adwaita icon theme).
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "login" => "dialog-password-symbolic",
        "creditcard" => "x-office-address-book-symbolic",
        "note" => "document-edit-symbolic",
        "identity" => "avatar-default-symbolic",
        "sshkey" => "utilities-terminal-symbolic",
        "wifi" => "network-wireless-symbolic",
        "alias" => "mail-unread-symbolic",
        _ => "text-x-generic-symbolic",
    }
}
/// Lower-case text used for searching: title, type, visible field values,
/// websites, note, alias address and passkey names. Hidden and TOTP values
/// are never present in a parsed `Detail`, so they can never be indexed.
pub fn search_text(d: &Detail) -> String {
    let mut parts = vec![d.item.title.clone(), kind_label(&d.item.kind).into()];
    for f in &d.fields {
        if f.kind == Kind::Text {
            if let Some(v) = &f.value {
                parts.push(v.clone());
            }
        }
    }
    parts.extend(d.urls.iter().cloned());
    parts.push(d.note.clone());
    parts.push(d.alias.clone());
    for p in &d.passkeys {
        parts.extend([p.domain.clone(), p.rp_name.clone(), p.user_name.clone()]);
    }
    parts.retain(|p| !p.is_empty());
    parts.join("\n").to_lowercase()
}
/// Case-insensitive match of every whitespace-separated query term against
/// the item title/type and, when known, its indexed text.
pub fn matches(item: &Item, indexed: Option<&str>, query: &str) -> bool {
    let title = format!("{}\n{}\n{}", item.title, item.kind, kind_label(&item.kind)).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| title.contains(term) || indexed.is_some_and(|t| t.contains(term)))
}
/// Best secondary line for an item row (username, email or alias).
pub fn subtitle_of(d: &Detail) -> String {
    for key in ["username", "email", "ssid", "cardholder_name", "full_name"] {
        if let Some(v) = d
            .fields
            .iter()
            .find(|f| f.key == key && f.section.is_empty())
            .and_then(|f| f.value.clone())
            .filter(|v| !v.is_empty())
        {
            return v;
        }
    }
    if !d.alias.is_empty() {
        return d.alias.clone();
    }
    String::new()
}

/// Vault or item member, from `vault member list --output json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub share_id: String,
    pub email: String,
    pub name: String,
    pub role: String,
}
impl Member {
    pub fn is_owner(&self) -> bool {
        self.role.eq_ignore_ascii_case("owner")
    }
}
/// Roles accepted by `vault share`, `item share` and `member update`.
pub const ROLES: [(&str, &str); 3] = [
    ("viewer", "Viewer"),
    ("editor", "Editor"),
    ("manager", "Manager"),
];
pub fn members(s: &str) -> Result<Vec<Member>> {
    let v = json(s)?;
    let a = v
        .get("members")
        .unwrap_or(&v)
        .as_array()
        .ok_or("Missing members array")?;
    Ok(a.iter()
        .map(|m| Member {
            share_id: text(&m["member_share_id"]),
            email: text(&m["email"]),
            name: text(&m["name"]),
            // ShareRole serializes as "Viewer" or {"Custom": {"name": …}}.
            role: match &m["role"] {
                Value::String(r) => r.clone(),
                Value::Object(o) => o
                    .get("Custom")
                    .map(|c| text(&c["name"]))
                    .unwrap_or_else(|| "Custom".into()),
                _ => String::new(),
            },
        })
        .filter(|m| !m.share_id.is_empty())
        .collect())
}
/// Pending vault/item invitation from `invite list --output json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub token: String,
    pub kind: String,
    pub inviter: String,
    pub name: String,
}
pub fn invites(s: &str) -> Result<Vec<Invite>> {
    let v = json(s)?;
    let a = v
        .get("invites")
        .unwrap_or(&v)
        .as_array()
        .ok_or("Missing invites array")?;
    Ok(a.iter()
        .map(|i| Invite {
            token: text(&i["token"]),
            kind: text(&i["invite_type"]),
            inviter: text(&i["inviter"]),
            name: text(&i["name"]),
        })
        .filter(|i| !i.token.is_empty())
        .collect())
}
/// `{"password": "…"}` from `password generate … --output json`.
pub fn generated_password(s: &str) -> Result<zeroize::Zeroizing<String>> {
    let v = json(s)?;
    v["password"]
        .as_str()
        .filter(|p| !p.is_empty())
        .map(|p| zeroize::Zeroizing::new(p.to_owned()))
        .ok_or("No password in generator output")
}
/// `{"id": "…", "alias": "…"}` from `item alias create --output json`.
pub fn created_alias(s: &str) -> Result<(String, String)> {
    let v = json(s)?;
    let alias = text(&v["alias"]);
    if alias.is_empty() {
        return Err("No alias in output");
    }
    Ok((text(&v["id"]), alias))
}

/// One editable field of an item type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// Key used in parsed details, mock data and `item update --field`.
    pub key: &'static str,
    pub label: &'static str,
    /// Key in the `item create <type> --from-template` JSON.
    pub template: &'static str,
    pub hidden: bool,
}
const fn f(key: &'static str, label: &'static str, hidden: bool) -> FieldSpec {
    FieldSpec {
        key,
        label,
        template: key,
        hidden,
    }
}
/// Item types that can be created, in menu order.
pub const CREATABLE: [&str; 8] = [
    "login",
    "alias",
    "note",
    "creditcard",
    "identity",
    "wifi",
    "sshkey",
    "custom",
];
pub fn field_specs(kind: &str) -> &'static [FieldSpec] {
    const LOGIN: &[FieldSpec] = &[
        f("username", "Username", false),
        f("email", "Email", false),
        f("password", "Password", true),
        FieldSpec {
            key: "totp",
            label: "2FA secret or otpauth:// URI",
            template: "totp_uri",
            hidden: true,
        },
    ];
    const CARD: &[FieldSpec] = &[
        f("cardholder_name", "Cardholder name", false),
        f("number", "Card number", true),
        f("expiration_date", "Expiration date (YYYY-MM)", false),
        FieldSpec {
            key: "verification_number",
            label: "Security code (CVV)",
            template: "cvv",
            hidden: true,
        },
        f("pin", "PIN", true),
    ];
    const IDENTITY: &[FieldSpec] = &[
        f("full_name", "Full name", false),
        f("email", "Email", false),
        f("phone_number", "Phone number", false),
        f("birthdate", "Birthdate", false),
        FieldSpec {
            key: "address",
            label: "Street address",
            template: "street_address",
            hidden: false,
        },
        f("zip_or_postal_code", "ZIP or postal code", false),
        f("city", "City", false),
        f("country_or_region", "Country or region", false),
        f("company", "Company", false),
        f("job_title", "Job title", false),
        f("website", "Website", false),
        f("passport_number", "Passport number", true),
        f("license_number", "Driver’s license number", true),
        f("social_security_number", "Social security number", true),
    ];
    const WIFI: &[FieldSpec] = &[
        f("ssid", "Network name (SSID)", false),
        f("password", "Password", true),
        f("security", "Security (wpa2, wpa3, wep, open)", false),
    ];
    const CUSTOM: &[FieldSpec] = &[
        f("custom_text", "Text field", false),
        f("custom_hidden", "Hidden field", true),
    ];
    match kind {
        "login" => LOGIN,
        "creditcard" => CARD,
        "identity" => IDENTITY,
        "wifi" => WIFI,
        "custom" => CUSTOM,
        _ => &[],
    }
}
/// pass-cli `item create` subcommand for a normalized type.
pub fn create_subcommand(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "login" => "login",
        "note" => "note",
        "creditcard" => "credit-card",
        "identity" => "identity",
        "wifi" => "wifi",
        "custom" => "custom",
        _ => return None,
    })
}
/// SSH key types offered by `item create ssh-key generate`.
pub const SSH_KEY_TYPES: [&str; 3] = ["ed25519", "rsa4096", "rsa2048"];

/// Editable item of any type. Secret values are zeroized when dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemDraft {
    pub kind: String,
    pub title: String,
    pub note: String,
    pub url: String,
    /// `(spec key, value)`. Empty values are omitted. When editing, an empty
    /// hidden value means “keep the current value”.
    pub values: Vec<(String, zeroize::Zeroizing<String>)>,
    /// Alias prefix (alias items) or key type (SSH keys).
    pub extra: String,
}
impl ItemDraft {
    pub fn get(&self, key: &str) -> &str {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }
    pub fn validate(&self) -> Result<()> {
        match self.kind.as_str() {
            "alias" => {
                let p = self.extra.trim();
                if p.is_empty() {
                    return Err("An alias prefix is required");
                }
                if !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                    || p.starts_with(['.', '-'])
                    || p.ends_with('.')
                {
                    return Err(
                        "Use only letters, digits, dots, dashes and underscores in the prefix",
                    );
                }
                return Ok(());
            }
            "sshkey" if !self.extra.is_empty() && !SSH_KEY_TYPES.contains(&self.extra.as_str()) => {
                return Err("Unsupported SSH key type");
            }
            "wifi" if self.get("ssid").trim().is_empty() => {
                return Err("A network name is required");
            }
            "creditcard" => {
                let exp = self.get("expiration_date").trim();
                let ok = exp.is_empty()
                    || (exp.len() == 7
                        && exp.as_bytes()[4] == b'-'
                        && exp[..4].bytes().all(|b| b.is_ascii_digit())
                        && exp[5..].parse::<u8>().is_ok_and(|m| (1..=12).contains(&m)));
                if !ok {
                    return Err("Use YYYY-MM for the expiration date");
                }
            }
            _ => (),
        }
        if self.title.trim().is_empty() {
            return Err("A title is required");
        }
        if !self.url.trim().is_empty() && !self.url.trim().contains('.') {
            return Err("The website does not look like an address");
        }
        Ok(())
    }
    /// JSON for `item create <type> --from-template -`, written to stdin so
    /// secret values never appear in argv or the environment.
    pub fn template(&self) -> zeroize::Zeroizing<String> {
        let opt = |s: &str| {
            if s.trim().is_empty() {
                Value::Null
            } else {
                Value::String(s.trim().into())
            }
        };
        let mut o = serde_json::Map::new();
        o.insert("title".into(), Value::String(self.title.trim().into()));
        o.insert("note".into(), opt(&self.note));
        let mut extra_fields = vec![];
        for spec in field_specs(&self.kind) {
            let v = self.get(spec.key);
            if self.kind == "custom" {
                if !v.is_empty() {
                    extra_fields.push(serde_json::json!({
                        "kind": if spec.hidden { "hidden" } else { "text" },
                        "title": spec.label,
                        "value": v,
                    }));
                }
                continue;
            }
            let value = if spec.hidden {
                if v.is_empty() {
                    Value::Null
                } else {
                    Value::String(v.into())
                }
            } else {
                opt(v)
            };
            o.insert(spec.template.into(), value);
        }
        match self.kind.as_str() {
            "login" => {
                o.insert(
                    "urls".into(),
                    if self.url.trim().is_empty() {
                        serde_json::json!([])
                    } else {
                        serde_json::json!([self.url.trim()])
                    },
                );
            }
            // `ssid` is required (not optional) in pass-cli's template.
            "wifi" => {
                o.insert("ssid".into(), Value::String(self.get("ssid").trim().into()));
            }
            "custom" => {
                o.insert("sections".into(), serde_json::json!([]));
            }
            _ => (),
        }
        o.insert("extra_fields".into(), Value::Array(extra_fields));
        let mut v = Value::Object(o);
        let out = zeroize::Zeroizing::new(v.to_string());
        drop(Json(std::mem::take(&mut v)));
        out
    }
    /// Non-secret `field=value` pairs changed relative to `old`, for
    /// `item update --field`. Hidden fields are never included: pass-cli only
    /// accepts new values on its command line.
    pub fn changed_fields(&self, old: &ItemDraft) -> Vec<(String, String)> {
        let mut out = vec![];
        for (k, new, old) in [
            ("title", &self.title, &old.title),
            ("note", &self.note, &old.note),
        ] {
            if new.trim() != old.trim() {
                out.push((k.to_owned(), new.trim().to_owned()));
            }
        }
        if self.kind == "login" && self.url.trim() != old.url.trim() {
            out.push(("urls".into(), self.url.trim().to_owned()));
        }
        for spec in field_specs(&self.kind) {
            if spec.hidden || self.kind == "custom" {
                continue;
            }
            let (new, old) = (self.get(spec.key).trim(), old.get(spec.key).trim());
            if new != old {
                out.push((spec.key.to_owned(), new.to_owned()));
            }
        }
        out
    }
    /// True when a hidden value was entered (only possible to apply in the
    /// demo backend or when creating).
    pub fn has_secret_change(&self) -> bool {
        field_specs(&self.kind)
            .iter()
            .any(|s| s.hidden && !self.get(s.key).is_empty())
    }
}
/// Pre-fill an editor from a parsed detail (no secrets are available here).
pub fn item_draft_from(d: &Detail) -> ItemDraft {
    let get = |k: &str| {
        d.fields
            .iter()
            .find(|f| f.key == k && f.section.is_empty())
            .and_then(|f| f.value.clone())
            .unwrap_or_default()
    };
    let mut values = vec![];
    for spec in field_specs(&d.item.kind) {
        let v = if d.item.kind == "custom" {
            d.fields
                .iter()
                .find(|f| f.label == spec.label)
                .and_then(|f| f.value.clone())
                .unwrap_or_default()
        } else {
            get(spec.key)
        };
        if !spec.hidden && !v.is_empty() {
            values.push((spec.key.to_owned(), zeroize::Zeroizing::new(v)));
        }
    }
    ItemDraft {
        kind: d.item.kind.clone(),
        title: d.item.title.clone(),
        note: d.note.clone(),
        url: d.urls.first().cloned().unwrap_or_default(),
        values,
        extra: String::new(),
    }
}
impl From<&LoginDraft> for ItemDraft {
    fn from(l: &LoginDraft) -> Self {
        let mut values = vec![
            (
                "username".to_owned(),
                zeroize::Zeroizing::new(l.username.clone()),
            ),
            ("email".to_owned(), zeroize::Zeroizing::new(l.email.clone())),
        ];
        if let Some(p) = &l.password {
            values.push(("password".into(), p.clone()));
        }
        values.retain(|(_, v)| !v.is_empty());
        ItemDraft {
            kind: "login".into(),
            title: l.title.clone(),
            note: l.note.clone(),
            url: l.url.clone(),
            values,
            extra: String::new(),
        }
    }
}
