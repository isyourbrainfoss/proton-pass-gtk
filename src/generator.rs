// SPDX-License-Identifier: GPL-3.0-or-later
//! Local password generator. Random bytes come from the kernel CSPRNG
//! (`/dev/urandom`, available inside the Flatpak sandbox) and are mapped to
//! characters with rejection sampling, so every character is uniform.
//! No subprocess, no network and no session are needed, and the value never
//! leaves this process until the user copies or uses it.
use std::io::Read;
use zeroize::{Zeroize, Zeroizing};

pub const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
pub const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
pub const DIGITS: &str = "0123456789";
pub const SYMBOLS: &str = "!#$%&*+-.:;=?@^_~";
/// Characters that are easy to confuse when read aloud or typed by hand.
pub const AMBIGUOUS: &str = "Il1O0o";
pub const MIN_LENGTH: u32 = 4;
pub const MAX_LENGTH: u32 = 64;
pub const MIN_WORDS: u32 = 3;
pub const MAX_WORDS: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomOptions {
    pub length: u32,
    pub uppercase: bool,
    pub numbers: bool,
    pub symbols: bool,
    pub avoid_ambiguous: bool,
}
impl Default for RandomOptions {
    /// Same defaults as `pass-cli password generate random`.
    fn default() -> Self {
        Self {
            length: 20,
            uppercase: true,
            numbers: true,
            symbols: true,
            avoid_ambiguous: false,
        }
    }
}
/// Word separators offered by `pass-cli password generate passphrase`.
pub const SEPARATORS: [(&str, &str); 7] = [
    ("hyphens", "Hyphens"),
    ("spaces", "Spaces"),
    ("periods", "Periods"),
    ("commas", "Commas"),
    ("underscores", "Underscores"),
    ("numbers", "Numbers"),
    ("numbers-and-symbols", "Numbers and symbols"),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassphraseOptions {
    pub words: u32,
    pub separator: String,
    pub capitalise: bool,
    pub numbers: bool,
}
impl Default for PassphraseOptions {
    fn default() -> Self {
        Self {
            words: 5,
            separator: "hyphens".into(),
            capitalise: true,
            numbers: true,
        }
    }
}

fn alphabet(o: &RandomOptions) -> Vec<char> {
    let mut a = String::from(LOWER);
    if o.uppercase {
        a.push_str(UPPER);
    }
    if o.numbers {
        a.push_str(DIGITS);
    }
    if o.symbols {
        a.push_str(SYMBOLS);
    }
    a.chars()
        .filter(|c| !(o.avoid_ambiguous && AMBIGUOUS.contains(*c)))
        .collect()
}
/// Uniform index in `0..n` from a byte source, by rejection sampling.
fn uniform(n: usize, next: &mut impl FnMut() -> Option<u8>) -> Option<usize> {
    assert!(n > 0 && n <= 256);
    let limit = 256 - (256 % n);
    loop {
        let b = next()? as usize;
        if b < limit {
            return Some(b % n);
        }
    }
}
/// Generate with an explicit byte source (tests use a deterministic one).
/// Returns `None` when the byte source fails.
pub fn random_with(
    o: &RandomOptions,
    mut next: impl FnMut() -> Option<u8>,
) -> Option<Zeroizing<String>> {
    let len = o.length.clamp(MIN_LENGTH, MAX_LENGTH) as usize;
    let a = alphabet(o);
    let classes: Vec<Vec<char>> = [
        (true, LOWER),
        (o.uppercase, UPPER),
        (o.numbers, DIGITS),
        (o.symbols, SYMBOLS),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, s)| {
        s.chars()
            .filter(|c| !(o.avoid_ambiguous && AMBIGUOUS.contains(*c)))
            .collect()
    })
    .collect();
    // Retry until every selected class is present (like Proton's generator);
    // this keeps the distribution uniform over the accepted passwords.
    loop {
        let mut chars = Vec::with_capacity(len);
        for _ in 0..len {
            match uniform(a.len(), &mut next) {
                Some(i) => chars.push(a[i]),
                None => {
                    chars.zeroize();
                    return None;
                }
            }
        }
        let ok = classes
            .iter()
            .all(|class| chars.iter().any(|c| class.contains(c)));
        let out = Zeroizing::new(chars.iter().collect::<String>());
        chars.zeroize();
        if ok || len < classes.len() {
            return Some(out);
        }
    }
}
/// Fill from the kernel CSPRNG. Fails closed: no fallback to a weak source.
pub fn random(o: &RandomOptions) -> Result<Zeroizing<String>, &'static str> {
    const ERR: &str = "The system random number generator is unavailable";
    let mut f = std::fs::File::open("/dev/urandom").map_err(|_| ERR)?;
    let mut buf = Zeroizing::new([0u8; 256]);
    let mut pos = buf.len();
    random_with(o, || {
        if pos == buf.len() {
            f.read_exact(&mut buf[..]).ok()?;
            pos = 0;
        }
        pos += 1;
        Some(buf[pos - 1])
    })
    .ok_or(ERR)
}
/// Estimated entropy in bits for a password produced by these options.
pub fn random_entropy_bits(o: &RandomOptions) -> f64 {
    let n = alphabet(o).len() as f64;
    f64::from(o.length.clamp(MIN_LENGTH, MAX_LENGTH)) * n.log2()
}
/// Entropy estimate for a passphrase from Proton's word list (EFF large
/// list, 7 776 words: ~12.9 bits per word, plus separators/digits).
pub fn passphrase_entropy_bits(o: &PassphraseOptions) -> f64 {
    let words = f64::from(o.words.clamp(MIN_WORDS, MAX_WORDS));
    let mut bits = words * 7776f64.log2();
    if o.numbers {
        bits += words * 10f64.log2();
    }
    if o.separator.starts_with("numbers") {
        bits += (words - 1.0) * 10f64.log2();
    }
    bits
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Weak,
    Fair,
    Strong,
    VeryStrong,
}
impl Strength {
    pub fn from_bits(bits: f64) -> Self {
        match bits {
            b if b < 50.0 => Self::Weak,
            b if b < 70.0 => Self::Fair,
            b if b < 100.0 => Self::Strong,
            _ => Self::VeryStrong,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
            Self::VeryStrong => "Very strong",
        }
    }
    /// Adwaita style class for the strength label.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Weak => "error",
            Self::Fair => "warning",
            Self::Strong | Self::VeryStrong => "success",
        }
    }
}
/// Demo-only passphrase for the mock backend: a tiny, clearly fake word
/// list. Real passphrases come from `pass-cli password generate passphrase`.
pub fn demo_passphrase(o: &PassphraseOptions) -> Result<Zeroizing<String>, &'static str> {
    const WORDS: [&str; 16] = [
        "demo",
        "sample",
        "fake",
        "mock",
        "example",
        "test",
        "placeholder",
        "dummy",
        "trial",
        "pretend",
        "sandbox",
        "fixture",
        "stub",
        "draft",
        "practice",
        "preview",
    ];
    let digits = RandomOptions {
        length: 64,
        uppercase: false,
        numbers: true,
        symbols: false,
        avoid_ambiguous: false,
    };
    let noise = random(&digits)?;
    let mut bytes = noise.bytes().cycle();
    let sep = match o.separator.as_str() {
        "spaces" => " ",
        "periods" => ".",
        "commas" => ",",
        "underscores" => "_",
        "numbers" | "numbers-and-symbols" => "7",
        _ => "-",
    };
    let words: Vec<String> = (0..o.words.clamp(MIN_WORDS, MAX_WORDS))
        .map(|_| {
            let w = WORDS[usize::from(bytes.next().unwrap_or(0)) % WORDS.len()];
            let mut w = if o.capitalise {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            } else {
                w.to_string()
            };
            if o.numbers {
                w.push(char::from(b'0' + bytes.next().unwrap_or(b'0') % 10));
            }
            w
        })
        .collect();
    Ok(Zeroizing::new(words.join(sep)))
}
