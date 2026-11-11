// SPDX-License-Identifier: GPL-3.0-or-later
//! Arguments contain identifiers and field names, never field values.
pub fn info() -> Vec<String> {
    args(&["info", "--output", "json"])
}
pub fn vaults() -> Vec<String> {
    args(&["vault", "list", "--output", "json"])
}
pub fn items(share: &str) -> Vec<String> {
    args(&[
        "item",
        "list",
        "--share-id",
        share,
        "--filter-state",
        "active",
        "--sort-by",
        "alphabetic-asc",
        "--output",
        "json",
    ])
}
pub fn view(share: &str, id: &str) -> Vec<String> {
    args(&[
        "item",
        "view",
        "--share-id",
        share,
        "--item-id",
        id,
        "--output",
        "json",
    ])
}
pub fn field(share: &str, id: &str, field: &str) -> Vec<String> {
    args(&[
        "item",
        "view",
        "--share-id",
        share,
        "--item-id",
        id,
        "--field",
        field,
    ])
}
pub fn totp(share: &str, id: &str, field: &str) -> Vec<String> {
    args(&[
        "item",
        "totp",
        "--share-id",
        share,
        "--item-id",
        id,
        "--field",
        field,
        "--output",
        "json",
    ])
}
/// The template (including any password) is written to stdin, never argv.
pub fn create_login(share: &str) -> Vec<String> {
    args(&[
        "item",
        "create",
        "login",
        "--share-id",
        share,
        "--from-template",
        "-",
    ])
}
/// Only non-secret fields may be passed here: pass-cli takes `--field k=v`
/// values on the command line, which other local processes can read.
pub fn update(share: &str, id: &str, fields: &[(&str, String)]) -> Vec<String> {
    let mut a = args(&["item", "update", "--share-id", share, "--item-id", id]);
    for (k, v) in fields {
        a.push("--field".into());
        a.push(format!("{k}={v}"));
    }
    a
}
/// Reversible: moves the item to the trash instead of deleting it.
pub fn trash(share: &str, id: &str) -> Vec<String> {
    args(&["item", "trash", "--share-id", share, "--item-id", id])
}
pub fn attachment_download(share: &str, id: &str, attachment: &str, output: &str) -> Vec<String> {
    args(&[
        "item",
        "attachment",
        "download",
        "--share-id",
        share,
        "--item-id",
        id,
        "--attachment-id",
        attachment,
        "--output",
        output,
    ])
}
fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_owned()).collect()
}
/// Only allow Proton's HTTPS desktop authentication endpoint.
pub fn login_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|s| {
            s.starts_with("https://account.proton.me/desktop/login?")
                || *s == "https://account.proton.me/desktop/login"
        })
        .map(str::to_owned)
}
