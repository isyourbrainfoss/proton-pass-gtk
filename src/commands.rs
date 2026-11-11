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
/// Trashed items of one vault (`item list --filter-state trashed`).
pub fn trashed_items(share: &str) -> Vec<String> {
    args(&[
        "item",
        "list",
        "--share-id",
        share,
        "--filter-state",
        "trashed",
        "--sort-by",
        "alphabetic-asc",
        "--output",
        "json",
    ])
}
/// Restore an item from the trash.
pub fn untrash(share: &str, id: &str) -> Vec<String> {
    args(&["item", "untrash", "--share-id", share, "--item-id", id])
}
/// Permanent deletion. Only offered for items already in the trash.
pub fn delete(share: &str, id: &str) -> Vec<String> {
    args(&["item", "delete", "--share-id", share, "--item-id", id])
}
/// Template JSON (with any secrets) goes to stdin, never argv.
pub fn create_item(kind_subcommand: &str, share: &str) -> Vec<String> {
    args(&[
        "item",
        "create",
        kind_subcommand,
        "--share-id",
        share,
        "--from-template",
        "-",
    ])
}
/// The prefix is not secret; the CLI chooses the alias suffix.
pub fn create_alias(share: &str, prefix: &str) -> Vec<String> {
    args(&[
        "item",
        "alias",
        "create",
        "--share-id",
        share,
        "--prefix",
        prefix,
        "--output",
        "json",
    ])
}
/// Generates the key pair inside pass-cli; no key material passes through
/// this app. A passphrase (`--password`) needs a TTY prompt and is not offered.
pub fn generate_ssh_key(share: &str, title: &str, key_type: &str) -> Vec<String> {
    args(&[
        "item",
        "create",
        "ssh-key",
        "generate",
        "--share-id",
        share,
        "--title",
        title,
        "--key-type",
        key_type,
    ])
}
pub fn move_item(from_share: &str, id: &str, to_share: &str) -> Vec<String> {
    args(&[
        "item",
        "move",
        "--from-share-id",
        from_share,
        "--item-id",
        id,
        "--to-share-id",
        to_share,
    ])
}
pub fn vault_create(name: &str) -> Vec<String> {
    args(&["vault", "create", "--name", name])
}
pub fn vault_rename(share: &str, name: &str) -> Vec<String> {
    args(&["vault", "update", "--share-id", share, "--name", name])
}
pub fn vault_delete(share: &str) -> Vec<String> {
    args(&["vault", "delete", "--share-id", share])
}
pub fn vault_members(share: &str) -> Vec<String> {
    args(&[
        "vault",
        "member",
        "list",
        "--share-id",
        share,
        "--output",
        "json",
    ])
}
pub fn vault_share(share: &str, email: &str, role: &str) -> Vec<String> {
    args(&["vault", "share", "--share-id", share, "--role", role, email])
}
pub fn vault_member_role(share: &str, member: &str, role: &str) -> Vec<String> {
    args(&[
        "vault",
        "member",
        "update",
        "--share-id",
        share,
        "--member-share-id",
        member,
        "--role",
        role,
    ])
}
pub fn vault_member_remove(share: &str, member: &str) -> Vec<String> {
    args(&[
        "vault",
        "member",
        "remove",
        "--share-id",
        share,
        "--member-share-id",
        member,
    ])
}
pub fn item_share(share: &str, id: &str, email: &str, role: &str) -> Vec<String> {
    args(&[
        "item",
        "share",
        "--share-id",
        share,
        "--item-id",
        id,
        "--role",
        role,
        email,
    ])
}
pub fn item_members(share: &str, id: &str) -> Vec<String> {
    args(&[
        "item",
        "member",
        "list",
        "--share-id",
        share,
        "--item-id",
        id,
        "--output",
        "json",
    ])
}
pub fn invites() -> Vec<String> {
    args(&["invite", "list", "--output", "json"])
}
pub fn invite_respond(accept: bool, token: &str) -> Vec<String> {
    args(&["invite", if accept { "accept" } else { "reject" }, token])
}
/// Unencrypted HTML archive. `--encrypt` reads its password only from an
/// environment variable or a TTY prompt, neither of which this app uses.
pub fn export_archive(output: &str, attachments: bool) -> Vec<String> {
    let mut a = args(&["export", "archive", "--output", output]);
    if attachments {
        a.push("--include-attachments".into());
    }
    a
}
/// Official passphrase generator (works offline, no session needed).
pub fn generate_passphrase(
    words: u32,
    separator: &str,
    capitalise: bool,
    numbers: bool,
) -> Vec<String> {
    let mut a = args(&["password", "generate", "passphrase", "--count"]);
    a.push(words.to_string());
    a.extend(args(&["--separator", separator, "--capitalise"]));
    a.push(capitalise.to_string());
    a.push("--numbers".into());
    a.push(numbers.to_string());
    a.extend(args(&["--output", "json"]));
    a
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
