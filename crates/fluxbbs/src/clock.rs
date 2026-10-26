//! The board's clock. Everything the board keeps (when a message was
//! posted, who called when, today's stats) and shows is in the board's
//! own time zone, `[bbs] time_zone` (an IANA name such as
//! `America/Chicago`; blank for the host's, which in a container is
//! UTC), never the host's. Callers may choose a zone of their own
//! (USERCFG), and times are shown to them in it; stored times stay in
//! the board's. New: the original ran on the DOS clock.

use std::str::FromStr;
use std::sync::RwLock;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::{OffsetName, Tz};

/// A time zone: the host's, a named one, or a fixed offset from UTC.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zone {
    Host,
    Named(Tz),
    Fixed(FixedOffset),
}

/// Abbreviations callers type (mostly North American, where the board
/// is) and the zone each means.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("ET", "America/New_York"), ("EST", "America/New_York"), ("EDT", "America/New_York"),
    ("CT", "America/Chicago"), ("CST", "America/Chicago"), ("CDT", "America/Chicago"),
    ("MT", "America/Denver"), ("MST", "America/Denver"), ("MDT", "America/Denver"),
    ("PT", "America/Los_Angeles"), ("PST", "America/Los_Angeles"), ("PDT", "America/Los_Angeles"),
    ("AKST", "America/Anchorage"), ("AKDT", "America/Anchorage"), ("HST", "Pacific/Honolulu"),
    ("AST", "America/Halifax"), ("ADT", "America/Halifax"), ("NST", "America/St_Johns"),
    ("GMT", "UTC"), ("UT", "UTC"), ("Z", "UTC"), ("BST", "Europe/London"),
    ("CET", "Europe/Berlin"), ("CEST", "Europe/Berlin"), ("EET", "Europe/Athens"), ("EEST", "Europe/Athens"),
    ("IST", "Asia/Kolkata"), ("JST", "Asia/Tokyo"), ("KST", "Asia/Seoul"), ("HKT", "Asia/Hong_Kong"),
    ("AEST", "Australia/Sydney"), ("AEDT", "Australia/Sydney"), ("ACST", "Australia/Adelaide"),
    ("AWST", "Australia/Perth"), ("NZST", "Pacific/Auckland"), ("NZDT", "Pacific/Auckland"),
];

/// Zones offered by name when a caller picks one: a label and the zone.
pub const CHOICES: &[(&str, &str)] = &[
    ("Eastern (New York)", "America/New_York"),
    ("Central (Chicago)", "America/Chicago"),
    ("Mountain (Denver)", "America/Denver"),
    ("Arizona (Phoenix)", "America/Phoenix"),
    ("Pacific (Los Angeles)", "America/Los_Angeles"),
    ("Alaska", "America/Anchorage"),
    ("Hawaii", "Pacific/Honolulu"),
    ("Atlantic (Halifax)", "America/Halifax"),
    ("Newfoundland", "America/St_Johns"),
    ("Sao Paulo", "America/Sao_Paulo"),
    ("UTC", "UTC"),
    ("London", "Europe/London"),
    ("Berlin / Paris / Rome", "Europe/Berlin"),
    ("Athens / Helsinki", "Europe/Athens"),
    ("Moscow", "Europe/Moscow"),
    ("India (Kolkata)", "Asia/Kolkata"),
    ("Tokyo", "Asia/Tokyo"),
    ("Hong Kong / Singapore", "Asia/Hong_Kong"),
    ("Sydney / Melbourne", "Australia/Sydney"),
    ("Perth", "Australia/Perth"),
    ("Auckland", "Pacific/Auckland"),
];

impl Zone {
    /// A zone from what a caller or the config typed: an IANA name in
    /// any case (`america/chicago`), an abbreviation (`CST`, `ET`), or
    /// an offset (`-6`, `UTC+5:30`, `+0100`). Blank is `None`.
    pub fn parse(s: &str) -> Option<Zone> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let upper = s.to_ascii_uppercase();
        if let Some((_, name)) = ABBREVIATIONS.iter().find(|(a, _)| *a == upper) {
            return Tz::from_str(name).ok().map(Zone::Named);
        }
        if let Ok(tz) = Tz::from_str(s) {
            return Some(Zone::Named(tz));
        }
        if let Some(tz) = chrono_tz::TZ_VARIANTS.iter().find(|tz| tz.name().eq_ignore_ascii_case(s)) {
            return Some(Zone::Named(*tz));
        }
        let off = upper.trim_start_matches("UTC").trim_start_matches("GMT").trim();
        let (sign, digits) = match off.as_bytes().first()? {
            b'+' => (1, &off[1..]),
            b'-' => (-1, &off[1..]),
            _ => (1, off),
        };
        let (h, m) = match digits.split_once(':') {
            Some((h, m)) => (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?),
            None if digits.len() == 4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
            None => (digits.parse().ok()?, 0),
        };
        if h > 14 || m > 59 {
            return None;
        }
        FixedOffset::east_opt(sign * (h * 3600 + m * 60)).map(Zone::Fixed)
    }

    /// The zone's name, as it was given (what the config and user
    /// records keep).
    pub fn name(&self) -> String {
        match self {
            Zone::Host => String::new(),
            Zone::Named(tz) => tz.name().to_string(),
            Zone::Fixed(o) => offset_text(*o),
        }
    }

    /// A short description for now: `CDT -05:00`, `+05:30`.
    pub fn short(&self) -> String {
        match self {
            Zone::Named(tz) => {
                let o = tz.offset_from_utc_datetime(&Utc::now().naive_utc());
                match o.abbreviation() {
                    Some(a) if !a.starts_with(['+', '-']) => format!("{a} {}", offset_text(o.fix())),
                    _ => offset_text(o.fix()),
                }
            }
            _ => offset_text(self.offset_utc(Utc::now().naive_utc())),
        }
    }

    /// The offset from UTC at a UTC instant.
    fn offset_utc(&self, utc: NaiveDateTime) -> FixedOffset {
        match self {
            Zone::Host => Local.offset_from_utc_datetime(&utc).fix(),
            Zone::Named(tz) => tz.offset_from_utc_datetime(&utc).fix(),
            Zone::Fixed(o) => *o,
        }
    }

    /// The offset from UTC at a local time (the earlier one when the
    /// clocks went back; the offset after a gap when they went forward).
    pub fn offset_local(&self, local: NaiveDateTime) -> FixedOffset {
        match self {
            Zone::Host => Local.offset_from_local_datetime(&local).earliest().map_or_else(|| Local::now().offset().fix(), |o| o.fix()),
            Zone::Named(tz) => match tz.offset_from_local_datetime(&local).earliest() {
                Some(o) => o.fix(),
                None => tz.offset_from_utc_datetime(&local).fix(),
            },
            Zone::Fixed(o) => *o,
        }
    }

    pub fn now(&self) -> DateTime<FixedOffset> {
        let utc = Utc::now();
        utc.with_timezone(&self.offset_utc(utc.naive_utc()))
    }

    /// A UTC time as this zone's local time.
    pub fn utc_to_local(&self, utc: NaiveDateTime) -> NaiveDateTime {
        utc + self.offset_utc(utc)
    }

    /// This zone's local time as UTC.
    pub fn local_to_utc(&self, local: NaiveDateTime) -> NaiveDateTime {
        local - self.offset_local(local)
    }
}

fn offset_text(o: FixedOffset) -> String {
    let secs = o.local_minus_utc();
    let (sign, secs) = if secs < 0 { ('-', -secs) } else { ('+', secs) };
    format!("{sign}{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

static BOARD: RwLock<Zone> = RwLock::new(Zone::Host);

/// The board's zone.
pub fn board() -> Zone {
    *BOARD.read().unwrap_or_else(|e| e.into_inner())
}

/// Start keeping board time in `name`'s zone (blank: the host's). A
/// name that isn't a zone is logged and leaves the host's.
pub fn set_board(name: &str) {
    let zone = match Zone::parse(name) {
        Some(z) => z,
        None if name.trim().is_empty() => Zone::Host,
        None => {
            tracing::warn!("[bbs] time_zone {name:?} isn't a time zone; keeping the host's");
            Zone::Host
        }
    };
    *BOARD.write().unwrap_or_else(|e| e.into_inner()) = zone;
    // Doors see board time too.
    #[cfg(feature = "doors")]
    fluxbbs_dos::set_clock(from_utc);
}

/// Now, in board time.
pub fn now() -> DateTime<FixedOffset> {
    board().now()
}

/// A UTC time in board time.
pub fn from_utc(utc: NaiveDateTime) -> NaiveDateTime {
    board().utc_to_local(utc)
}

/// A board time in another zone.
pub fn convert(at: NaiveDateTime, to: &Zone) -> NaiveDateTime {
    to.utc_to_local(board().local_to_utc(at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn parses_names_abbreviations_and_offsets() {
        assert_eq!(Zone::parse("America/Chicago").unwrap().name(), "America/Chicago");
        assert_eq!(Zone::parse("america/chicago").unwrap().name(), "America/Chicago");
        assert_eq!(Zone::parse("cst").unwrap().name(), "America/Chicago");
        assert_eq!(Zone::parse("utc").unwrap().name(), "UTC");
        assert_eq!(Zone::parse("-6").unwrap().name(), "-06:00");
        assert_eq!(Zone::parse("UTC+5:30").unwrap().name(), "+05:30");
        assert_eq!(Zone::parse("+0100").unwrap().name(), "+01:00");
        assert_eq!(Zone::parse(""), None);
        assert_eq!(Zone::parse("Mars/Olympus"), None);
        assert_eq!(Zone::parse("+15"), None);
    }

    #[test]
    fn converts_between_zones() {
        let central = Zone::parse("America/Chicago").unwrap();
        let eastern = Zone::parse("America/New_York").unwrap();
        let summer = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap().and_hms_opt(21, 30, 0).unwrap();
        let winter = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap().and_hms_opt(21, 30, 0).unwrap();
        assert_eq!(central.local_to_utc(summer), summer + chrono::Duration::hours(5));
        assert_eq!(central.local_to_utc(winter), winter + chrono::Duration::hours(6));
        assert_eq!(eastern.utc_to_local(central.local_to_utc(summer)), summer + chrono::Duration::hours(1));
        let utc = Zone::parse("UTC").unwrap();
        assert_eq!(utc.utc_to_local(central.local_to_utc(winter)), winter + chrono::Duration::hours(6));
        assert_eq!(Zone::parse("+05:30").unwrap().utc_to_local(winter), winter + chrono::Duration::minutes(330));
        assert_eq!(central.short().len(), "CDT -05:00".len());
    }
}
