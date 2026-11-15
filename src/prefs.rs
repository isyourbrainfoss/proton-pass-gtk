// SPDX-License-Identifier: GPL-3.0-or-later
//! Non-secret user preferences, stored as a small `key=value` file in the
//! XDG config directory (inside Flatpak: `~/.var/app/<app-id>/config`).
//! Never store credentials, extra passwords, codes or item contents here.
use crate::generator::{self, PassphraseOptions, RandomOptions};
use std::path::{Path, PathBuf};

pub const CLIPBOARD_CHOICES: [u32; 6] = [10, 20, 30, 45, 60, 90];
/// Minutes; 0 means never.
pub const AUTO_LOCK_CHOICES: [u32; 7] = [0, 1, 2, 5, 10, 15, 30];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}
impl ColorScheme {
    pub const ALL: [ColorScheme; 3] = [Self::System, Self::Light, Self::Dark];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub clipboard_seconds: u32,
    pub auto_lock_minutes: u32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// Share ID of the last opened vault (an identifier, not a secret).
    pub last_vault: String,
    pub color_scheme: ColorScheme,
    /// Ask for the Pass extra password before signing in.
    pub extra_password: bool,
    /// Search usernames, websites, notes and aliases too. For real accounts
    /// this loads each item’s details one at a time in the background.
    pub deep_search: bool,
    /// Password generator defaults (options only, never generated values).
    pub gen_passphrase: bool,
    pub gen_length: u32,
    pub gen_uppercase: bool,
    pub gen_numbers: bool,
    pub gen_symbols: bool,
    pub gen_avoid_ambiguous: bool,
    pub gen_words: u32,
    pub gen_separator: String,
    pub gen_capitalise: bool,
    pub gen_word_numbers: bool,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            clipboard_seconds: 30,
            auto_lock_minutes: 5,
            width: 1280,
            height: 800,
            maximized: false,
            last_vault: String::new(),
            color_scheme: ColorScheme::System,
            extra_password: false,
            deep_search: false,
            gen_passphrase: false,
            gen_length: 20,
            gen_uppercase: true,
            gen_numbers: true,
            gen_symbols: true,
            gen_avoid_ambiguous: false,
            gen_words: 5,
            gen_separator: "hyphens".into(),
            gen_capitalise: true,
            gen_word_numbers: true,
        }
    }
}
fn nearest(choices: &[u32], v: u32) -> u32 {
    *choices.iter().min_by_key(|c| c.abs_diff(v)).unwrap()
}
impl Prefs {
    /// Unknown keys and invalid values are ignored; numbers are clamped to
    /// the offered choices so a hand-edited file cannot disable clearing.
    pub fn parse(s: &str) -> Self {
        let mut p = Self::default();
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let num = || v.parse::<u32>().ok();
            match k {
                "clipboard-seconds" => {
                    if let Some(n) = num() {
                        p.clipboard_seconds = nearest(&CLIPBOARD_CHOICES, n);
                    }
                }
                "auto-lock-minutes" => {
                    if let Some(n) = num() {
                        p.auto_lock_minutes = nearest(&AUTO_LOCK_CHOICES, n);
                    }
                }
                "width" => {
                    if let Ok(n) = v.parse::<i32>() {
                        p.width = n.clamp(320, 8192);
                    }
                }
                "height" => {
                    if let Ok(n) = v.parse::<i32>() {
                        p.height = n.clamp(400, 8192);
                    }
                }
                "maximized" => p.maximized = v == "true",
                "last-vault" => {
                    if v.chars().all(|c| c.is_ascii_graphic()) && v.len() <= 256 {
                        p.last_vault = v.into();
                    }
                }
                "color-scheme" => {
                    if let Some(c) = ColorScheme::parse(v) {
                        p.color_scheme = c;
                    }
                }
                "extra-password" => p.extra_password = v == "true",
                "deep-search" => p.deep_search = v == "true",
                "generator-mode" => p.gen_passphrase = v == "passphrase",
                "generator-length" => {
                    if let Some(n) = num() {
                        p.gen_length = n.clamp(generator::MIN_LENGTH, generator::MAX_LENGTH);
                    }
                }
                "generator-uppercase" => p.gen_uppercase = v == "true",
                "generator-numbers" => p.gen_numbers = v == "true",
                "generator-symbols" => p.gen_symbols = v == "true",
                "generator-avoid-ambiguous" => p.gen_avoid_ambiguous = v == "true",
                "generator-words" => {
                    if let Some(n) = num() {
                        p.gen_words = n.clamp(generator::MIN_WORDS, generator::MAX_WORDS);
                    }
                }
                "generator-separator" => {
                    if generator::SEPARATORS.iter().any(|(k, _)| *k == v) {
                        p.gen_separator = v.into();
                    }
                }
                "generator-capitalise" => p.gen_capitalise = v == "true",
                "generator-word-numbers" => p.gen_word_numbers = v == "true",
                _ => (),
            }
        }
        p
    }
    pub fn serialize(&self) -> String {
        format!(
            "# Proton Pass GTK preferences. Contains no secrets.\n\
             clipboard-seconds={}\nauto-lock-minutes={}\nwidth={}\nheight={}\n\
             maximized={}\nlast-vault={}\ncolor-scheme={}\nextra-password={}\n\
             deep-search={}\ngenerator-mode={}\ngenerator-length={}\n\
             generator-uppercase={}\ngenerator-numbers={}\ngenerator-symbols={}\n\
             generator-avoid-ambiguous={}\ngenerator-words={}\ngenerator-separator={}\n\
             generator-capitalise={}\ngenerator-word-numbers={}\n",
            self.clipboard_seconds,
            self.auto_lock_minutes,
            self.width,
            self.height,
            self.maximized,
            self.last_vault,
            self.color_scheme.as_str(),
            self.extra_password,
            self.deep_search,
            if self.gen_passphrase {
                "passphrase"
            } else {
                "random"
            },
            self.gen_length,
            self.gen_uppercase,
            self.gen_numbers,
            self.gen_symbols,
            self.gen_avoid_ambiguous,
            self.gen_words,
            self.gen_separator,
            self.gen_capitalise,
            self.gen_word_numbers,
        )
    }
    pub fn random_options(&self) -> RandomOptions {
        RandomOptions {
            length: self.gen_length,
            uppercase: self.gen_uppercase,
            numbers: self.gen_numbers,
            symbols: self.gen_symbols,
            avoid_ambiguous: self.gen_avoid_ambiguous,
        }
    }
    pub fn passphrase_options(&self) -> PassphraseOptions {
        PassphraseOptions {
            words: self.gen_words,
            separator: self.gen_separator.clone(),
            capitalise: self.gen_capitalise,
            numbers: self.gen_word_numbers,
        }
    }
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|s| Self::parse(&s))
            .unwrap_or_default()
    }
    /// Atomic replace with owner-only permissions.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(self.serialize().as_bytes())?;
        f.sync_all()?;
        std::fs::rename(tmp, path)
    }
}
/// `$PPG_CONFIG_DIR`, else `$XDG_CONFIG_HOME/proton-pass-gtk`, else
/// `~/.config/proton-pass-gtk`.
pub fn default_path() -> PathBuf {
    let dir = std::env::var_os("PPG_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|s| !s.is_empty())
                .map(|d| PathBuf::from(d).join("proton-pass-gtk"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/proton-pass-gtk"))
        })
        .unwrap_or_else(|| PathBuf::from(".proton-pass-gtk"));
    dir.join("settings.ini")
}
