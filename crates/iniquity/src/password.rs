//! Password rules (`[passwords]`), new: the original only asked for four
//! characters. Length and a list of common passwords, rather than rules
//! about digits and symbols, following current advice (NIST 800-63B).

use crate::config::Passwords;

/// The longest password accepted.
pub const MAX_LEN: usize = 64;

const COMMON: &str = include_str!("../assets/common-passwords.txt");

/// Why a password was refused.
#[derive(Debug, PartialEq)]
pub enum Weak {
    Short(usize),
    Common,
    Handle,
    Repeated,
}

impl Weak {
    /// The string (in strings.toml) that explains it.
    pub fn string(&self) -> &'static str {
        match self {
            Weak::Short(_) => "PwRuleShort",
            Weak::Common => "PwRuleCommon",
            Weak::Handle => "PwRuleHandle",
            Weak::Repeated => "PwRuleRepeated",
        }
    }
}

fn is_common(pw: &str) -> bool {
    COMMON.lines().any(|l| !l.starts_with('#') && l.trim().eq_ignore_ascii_case(pw))
}

/// Check a new password for `handle` against the rules.
pub fn check(rules: &Passwords, pw: &str, handle: &str) -> Result<(), Weak> {
    let len = pw.chars().count();
    let min = (rules.min_length as usize).max(1);
    if len < min {
        return Err(Weak::Short(min));
    }
    if !rules.refuse_common {
        return Ok(());
    }
    let lower = pw.to_lowercase();
    if lower.chars().all(|c| lower.starts_with(c)) {
        return Err(Weak::Repeated);
    }
    let handle = handle.to_lowercase();
    // (A very short handle would rule out too much.)
    if handle.chars().count() >= 3 && (lower.contains(&handle) || handle.contains(&lower)) {
        return Err(Weak::Handle);
    }
    // "password1!" is no better than "password".
    let core = lower.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_ascii_punctuation());
    if is_common(&lower) || (!core.is_empty() && is_common(core)) {
        return Err(Weak::Common);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let r = Passwords::default();
        assert_eq!(check(&r, "short", "phrac"), Err(Weak::Short(8)));
        assert_eq!(check(&r, "aaaaaaaaaa", "phrac"), Err(Weak::Repeated));
        assert_eq!(check(&r, "PHRAC2026!", "phrac"), Err(Weak::Handle));
        assert_eq!(check(&r, "Password123!", "phrac"), Err(Weak::Common));
        assert_eq!(check(&r, "sysop", "x"), Err(Weak::Short(8)));
        assert_eq!(check(&r, "sysop123", "x"), Err(Weak::Common));
        assert_eq!(check(&r, "correct horse battery", "phrac"), Ok(()));
        let lax = Passwords { min_length: 4, refuse_common: false, ..r };
        assert_eq!(check(&lax, "sysop", "sysop"), Ok(()));
    }
}
