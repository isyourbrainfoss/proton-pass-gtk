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
    })
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
            ("url", &self.url, &old.url),
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
