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
