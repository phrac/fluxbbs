//! Access Condition System expressions (`acsOk`), e.g. `s50`, `!k902`,
//! `(s100|fA)&t10`. Adjacent conditions are ANDed; `&`, `|`, `!` and
//! parentheses work as in the manual. Codes are case-insensitive.

use std::collections::BTreeSet;

/// Everything an ACS expression can test.
#[derive(Debug, Clone, Default)]
pub struct AcsEnv {
    pub local: bool,
    pub ok_flag: bool,
    pub age: Option<u32>,
    pub conf_msg: u32,
    pub conf_file: u32,
    pub dsl: u8,
    pub sl: u8,
    pub flags: BTreeSet<char>,
    pub sex: char,
    pub hour: u32,
    pub area_code: String,
    pub posts: u32,
    pub minutes_left: u32,
    pub user_id: u32,
    pub level: char,
    /// 0 = Sunday.
    pub weekday: u32,
    pub minute_of_day: u32,
    /// Post/call ratio meets the level's requirement (`Z`).
    pub ratio_ok: bool,
    /// The node the caller is on (`N`, 2.0).
    pub node: u32,
    /// New: calls made, this one included (`L`): `!l2` is a first call.
    pub calls: u32,
}

pub fn check(expr: &str, env: &AcsEnv) -> bool {
    let chars: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.is_empty() {
        return true;
    }
    let mut p = Parser { s: &chars, i: 0, env };
    // A malformed tail (e.g. a stray ')') fails closed.
    p.or() && p.i == chars.len()
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    env: &'a AcsEnv,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn or(&mut self) -> bool {
        let mut v = self.and();
        while self.peek() == Some('|') {
            self.i += 1;
            let rhs = self.and();
            v = v || rhs;
        }
        v
    }

    fn and(&mut self) -> bool {
        let mut v = self.unary();
        loop {
            match self.peek() {
                Some('&') => self.i += 1,
                Some('|') | Some(')') | None => return v,
                _ => {} // juxtaposition
            }
            let rhs = self.unary();
            v = v && rhs;
        }
    }

    fn unary(&mut self) -> bool {
        match self.peek() {
            Some('!') => {
                self.i += 1;
                !self.unary()
            }
            Some('(') => {
                self.i += 1;
                let v = self.or();
                if self.peek() == Some(')') {
                    self.i += 1;
                }
                v
            }
            Some(_) => self.atom(),
            None => false,
        }
    }

    fn number(&mut self) -> u32 {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.s[start..self.i].iter().collect::<String>().parse().unwrap_or(0)
    }

    fn letter_arg(&mut self) -> char {
        let c = self.peek().unwrap_or('\0');
        self.i += 1;
        c.to_ascii_uppercase()
    }

    fn atom(&mut self) -> bool {
        let e = self.env;
        let c = self.s[self.i].to_ascii_uppercase();
        self.i += 1;
        match c {
            '=' => e.local,
            '*' => e.ok_flag,
            'A' => e.age.is_some_and(|a| a >= self.number()),
            'B' => {
                self.number();
                true // no modem speeds to check
            }
            'C' => e.conf_msg == self.number(),
            'D' => e.dsl as u32 >= self.number(),
            'F' => e.flags.contains(&self.letter_arg()),
            'G' => e.sex.to_ascii_uppercase() == self.letter_arg(),
            'H' => e.hour >= self.number(),
            'N' => e.node == self.number(),
            'K' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                e.area_code == self.s[start..self.i].iter().collect::<String>()
            }
            'P' => e.posts >= self.number(),
            'S' => e.sl as u32 >= self.number(),
            'T' => e.minutes_left >= self.number(),
            'U' => e.user_id == self.number(),
            'V' => e.level.to_ascii_uppercase() >= 'B',
            'W' => e.weekday == self.number(),
            'X' => e.conf_file == self.number(),
            'Y' => e.minute_of_day >= self.number(),
            'Z' => e.ratio_ok,
            'L' => e.calls >= self.number(),
            _ => false,
        }
    }
}

/// New: the flags an expression tests (`fA`), for the flag editor.
pub fn flags_in(expr: &str) -> BTreeSet<char> {
    let s: Vec<char> = expr.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < s.len() {
        match s[i].to_ascii_uppercase() {
            // Both take a letter, which isn't a code of its own.
            'F' | 'G' if i + 1 < s.len() => {
                if s[i].eq_ignore_ascii_case(&'f') && s[i + 1].is_ascii_alphabetic() {
                    out.insert(s[i + 1].to_ascii_uppercase());
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn flags_an_expression_tests() {
        assert_eq!(flags_in("(s100|fA)&!fb"), BTreeSet::from(['A', 'B']));
        // gF is "female", not flag F.
        assert_eq!(flags_in("gFs20"), BTreeSet::new());
    }

    use super::*;

    fn env() -> AcsEnv {
        AcsEnv {
            sl: 50,
            dsl: 25,
            flags: ['A'].into(),
            sex: 'F',
            area_code: "902".into(),
            minutes_left: 30,
            level: 'B',
            ..AcsEnv::default()
        }
    }

    #[test]
    fn stock_expressions() {
        let e = env();
        assert!(check("", &e));
        assert!(check("s25", &e));
        assert!(check("s50", &e));
        assert!(!check("s250", &e));
        assert!(!check("!s50", &e));
        assert!(!check("!k902", &e));
        assert!(check("k902", &e));
        assert!(!check("*", &e));
    }

    #[test]
    fn operators() {
        let e = env();
        assert!(check("s50&d25", &e));
        assert!(check("s50d25", &e)); // juxtaposition is AND
        assert!(!check("s50d26", &e));
        assert!(check("s255|fa", &e));
        assert!(check("(s255|fb)|gf", &e));
        assert!(!check("(s255|fb)&gf", &e));
        assert!(check("!(s255)&t30&v", &e));
        assert!(!check("t31", &e));
        let first = AcsEnv { calls: 1, ..env() };
        assert!(check("!l2", &first) && !check("!l2", &AcsEnv { calls: 2, ..env() }));
        let on2 = AcsEnv { node: 2, ..env() };
        assert!(check("n2", &on2) && !check("n1", &on2) && check("!n1", &on2));
    }

    #[test]
    fn malformed_fails_closed() {
        assert!(!check("s25)", &env()));
        assert!(!check("q", &env()));
    }
}
