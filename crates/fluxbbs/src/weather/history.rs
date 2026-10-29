//! `h`: the last 30 days, high to low and the rain, against the normal
//! (1991-2020); and the day's records since 1940. Both from Open-Meteo:
//! the recent days from its forecast service, the rest from its archive
//! (a big fetch, about ten seconds; kept a day a place).

use std::io;
use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use serde_json::Value;

use super::{MSG_ROW, Place, Units, client, line, temp_color};
use crate::pixels::{Canvas, H, Rgb, Stage, W, mix, painted, rgb};
use crate::session::Session;

/// The days fetched back (the drought's longest look).
pub(super) const BACK: usize = 92;
/// The days the chart shows.
const DAYS: usize = 30;
/// The normals' years.
const NORMALS: std::ops::RangeInclusive<i32> = 1991..=2020;

/// The days up to today: highs, lows and rain (NaN where there's none).
#[derive(Clone)]
pub(super) struct Recent {
    pub dates: Vec<NaiveDate>,
    pub hi: Vec<f32>,
    pub lo: Vec<f32>,
    pub rain: Vec<f32>,
}

impl Recent {
    /// Today, where the place is.
    pub fn today(&self) -> NaiveDate {
        self.dates.last().copied().unwrap_or_default()
    }

    /// The last `n` whole days (today's isn't over).
    pub fn last(&self, n: usize) -> std::ops::Range<usize> {
        let end = self.dates.len().saturating_sub(1);
        end.saturating_sub(n)..end
    }

    /// The rain over the last `n` whole days.
    pub fn rain_over(&self, n: usize) -> f32 {
        self.rain[self.last(n)].iter().filter(|v| !v.is_nan()).sum()
    }
}

/// A day of the year, over the years: the normal high, low and rain, and
/// the records.
#[derive(Clone, Copy, Default)]
pub(super) struct DayClimate {
    pub hi: f32,
    pub lo: f32,
    pub rain: f32,
    pub record_hi: (f32, i32),
    pub record_lo: (f32, i32),
    pub wettest: (f32, i32),
}

/// Every day of the year's climate (by its day in a leap year), and
/// today's a year ago.
#[derive(Clone)]
pub(super) struct Almanac {
    pub days: Vec<DayClimate>,
    pub last_year: Option<(f32, f32)>,
    pub since: i32,
}

impl Almanac {
    pub fn day(&self, d: NaiveDate) -> DayClimate {
        self.days[slot(d)]
    }

    /// The normal rain over the `n` days before `today`.
    pub fn rain_before(&self, today: NaiveDate, n: usize) -> f32 {
        (1..=n as i64).map(|k| self.day(today - chrono::Duration::days(k)).rain).sum()
    }
}

/// A date's place in a leap year (Feb 29 its own).
fn slot(d: NaiveDate) -> usize {
    NaiveDate::from_ymd_opt(2000, d.month(), d.day()).map_or(0, |x| x.ordinal0() as usize)
}

fn floats(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().map_or(f32::NAN, |n| n as f32)).collect()).unwrap_or_default()
}

fn dates(v: &Value) -> Vec<NaiveDate> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())).collect()).unwrap_or_default()
}

/// The last `BACK` days and today.
pub(super) async fn recent(p: &Place, units: Units) -> anyhow::Result<Recent> {
    let mut q = vec![
        ("latitude", format!("{:.4}", p.lat)),
        ("longitude", format!("{:.4}", p.lon)),
        ("daily", "temperature_2m_max,temperature_2m_min,precipitation_sum".into()),
        ("past_days", BACK.to_string()),
        ("forecast_days", "1".into()),
        ("timezone", "auto".into()),
    ];
    q.extend(units.query());
    let url = reqwest::Url::parse_with_params("https://api.open-meteo.com/v1/forecast", &q)?;
    let v: Value = client()?.get(url).send().await?.error_for_status()?.json().await?;
    let d = &v["daily"];
    let r = Recent { dates: dates(&d["time"]), hi: floats(&d["temperature_2m_max"]), lo: floats(&d["temperature_2m_min"]), rain: floats(&d["precipitation_sum"]) };
    if r.dates.len() < DAYS + 1 || r.hi.len() != r.dates.len() || r.lo.len() != r.dates.len() || r.rain.len() != r.dates.len() {
        anyhow::bail!("the past days came back short");
    }
    Ok(r)
}

/// The place's climate since 1940, from the archive (which runs a few
/// days behind).
pub(super) async fn almanac(p: &Place, units: Units, today: NaiveDate) -> anyhow::Result<Almanac> {
    let mut q = vec![
        ("latitude", format!("{:.4}", p.lat)),
        ("longitude", format!("{:.4}", p.lon)),
        ("start_date", "1940-01-01".into()),
        ("end_date", (today - chrono::Duration::days(7)).to_string()),
        ("daily", "temperature_2m_max,temperature_2m_min,precipitation_sum".into()),
        ("timezone", "auto".into()),
    ];
    q.extend(units.query());
    let url = reqwest::Url::parse_with_params("https://archive-api.open-meteo.com/v1/archive", &q)?;
    let c = reqwest::Client::builder().timeout(Duration::from_secs(60)).user_agent("flux/bbs weather").build()?;
    let v: Value = c.get(url).send().await?.error_for_status()?.json().await?;
    let d = &v["daily"];
    let (ds, hi, lo, rain) = (dates(&d["time"]), floats(&d["temperature_2m_max"]), floats(&d["temperature_2m_min"]), floats(&d["precipitation_sum"]));
    if ds.is_empty() || hi.len() != ds.len() || lo.len() != ds.len() || rain.len() != ds.len() {
        anyhow::bail!("the archive came back in a shape it doesn't read");
    }
    Ok(climate(&ds, &hi, &lo, &rain, today))
}

/// The archive's days, summed up by day of the year.
fn climate(ds: &[NaiveDate], hi: &[f32], lo: &[f32], rain: &[f32], today: NaiveDate) -> Almanac {
    let mut sums = vec![[0.0f64; 3]; 366];
    let mut counts = vec![[0u32; 3]; 366];
    let mut days = vec![DayClimate { record_hi: (f32::MIN, 0), record_lo: (f32::MAX, 0), wettest: (f32::MIN, 0), ..Default::default() }; 366];
    let year_ago = today.with_year(today.year() - 1).unwrap_or(today - chrono::Duration::days(365));
    let mut last_year = None;
    for (i, d) in ds.iter().enumerate() {
        let k = slot(*d);
        let y = d.year();
        let normal = NORMALS.contains(&y);
        for (j, v) in [hi[i], lo[i], rain[i]].into_iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            if normal {
                sums[k][j] += v as f64;
                counts[k][j] += 1;
            }
            let rec = &mut days[k];
            match j {
                0 if v > rec.record_hi.0 => rec.record_hi = (v, y),
                1 if v < rec.record_lo.0 => rec.record_lo = (v, y),
                2 if v > rec.wettest.0 => rec.wettest = (v, y),
                _ => {}
            }
        }
        if *d == year_ago && !hi[i].is_nan() && !lo[i].is_nan() {
            last_year = Some((hi[i], lo[i]));
        }
    }
    for (k, day) in days.iter_mut().enumerate() {
        let avg = |j: usize| if counts[k][j] > 0 { (sums[k][j] / counts[k][j] as f64) as f32 } else { f32::NAN };
        (day.hi, day.lo, day.rain) = (avg(0), avg(1), avg(2));
    }
    Almanac { days, last_year, since: ds.first().map_or(1940, |d| d.year()) }
}

impl Session {
    /// The recent days for `place`, kept ten minutes.
    pub(super) async fn weather_recent(&mut self, place: &Place, units: Units) -> Option<Recent> {
        let key = format!("recent {:.2} {:.2} {units:?}", place.lat, place.lon);
        if let Some(r) = self.state.weather.recall::<Recent>(&key, super::KEEP) {
            return Some(r);
        }
        match recent(place, units).await {
            Ok(r) => {
                self.state.weather.remember(key, r.clone());
                Some(r)
            }
            Err(e) => {
                tracing::warn!("weather: {}: the past days: {e:#}", place.name);
                None
            }
        }
    }

    /// The almanac for `place`, if it's been fetched today.
    pub(super) fn weather_almanac_kept(&self, place: &Place, units: Units, today: NaiveDate) -> (String, Option<Almanac>) {
        let key = format!("almanac {:.2} {:.2} {units:?} {today}", place.lat, place.lon);
        let a = self.state.weather.recall::<Almanac>(&key, Duration::from_secs(86_400));
        (key, a)
    }

    /// `h`: the history.
    pub(super) async fn weather_history(&mut self, st: &mut Stage, place: &Place, units: Units) -> io::Result<()> {
        self.weather_footer(st, "");
        self.stage_line(MSG_ROW, &painted("looking back...", [150, 200, 255]), st.truecolor);
        let Some(r) = self.weather_recent(place, units).await else {
            self.stage_line(MSG_ROW, &painted("the past days aren't to be had right now.", [255, 140, 120]), st.truecolor);
            self.read_key().await?;
            return Ok(());
        };
        let today = r.today();
        let (key, mut a) = self.weather_almanac_kept(place, units, today);
        let mut failed = false;
        if a.is_none() {
            self.stage_line(MSG_ROW, &painted("digging through the almanac...", [150, 200, 255]), st.truecolor);
            let got = self.weather_wait(st, almanac(place, units, today), |cv, _| draw(cv, &r, None, &place.name, units, false)).await?;
            match got {
                None => return Ok(()),
                Some(Ok(x)) => {
                    self.state.weather.remember(key, x.clone());
                    a = Some(x);
                }
                Some(Err(e)) => {
                    tracing::warn!("weather: {}: the almanac: {e:#}", place.name);
                    failed = true;
                }
            }
        }
        let since = a.as_ref().map_or(1940, |a| a.since);
        self.stage_line(MSG_ROW, &painted("", [150, 160, 180]), st.truecolor);
        self.weather_footer(st, &format!("   normals 1991-2020, records since {since}, from open-meteo.com"));
        self.weather_hold(st, |cv, _| draw(cv, &r, a.as_ref(), &place.name, units, failed)).await?;
        Ok(())
    }
}

/// The page: the chart across the top, the rain under it, the month and
/// the day in boxes.
fn draw(cv: &mut Canvas, r: &Recent, a: Option<&Almanac>, place: &str, units: Units, failed: bool) {
    cv.clear_labels();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            cv.set(x, y, mix(rgb(8, 12, 30), rgb(18, 24, 52), y as f32 / H as f32));
        }
    }
    let title = format!(" the last {DAYS} days in {place} ");
    cv.label((W - title.chars().count().min(W)) / 2, 0, &title, rgb(255, 230, 140), None);
    let days = r.last(DAYS);
    let normal = |d: NaiveDate| a.map(|a| a.day(d)).filter(|c| !c.hi.is_nan());
    // The scale: the days' range and the normals', a little room over and
    // under, in steps of ten (five Celsius).
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for i in days.clone() {
        for v in [r.hi[i], r.lo[i]] {
            if !v.is_nan() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if let Some(c) = normal(r.dates[i]) {
            lo = lo.min(c.lo);
            hi = hi.max(c.hi);
        }
    }
    if lo > hi {
        (lo, hi) = (0.0, 1.0);
    }
    let step = if units == Units::Us { 10.0 } else { 5.0 };
    let (lo, hi) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    let (top, bottom) = (3, 21);
    let ty = |t: f32| bottom as f32 - (t - lo) / (hi - lo).max(1.0) * (bottom - top) as f32;
    let x_of = |i: usize| 6 + (i * 12 / 5) as i32;
    // The lines every step, each labeled where it has a row of its own.
    let mut t = lo;
    let mut labeled = usize::MAX;
    while t <= hi {
        let y = ty(t).round() as i32;
        for x in 5..78 {
            cv.set(x, y, rgb(28, 36, 64));
        }
        let row = y as usize / 2;
        if row != labeled {
            cv.label(0, row, &format!("{t:>4.0}"), rgb(130, 140, 160), None);
            labeled = row;
        }
        t += step;
    }
    // The normal high and low, a dim line each; then each day, low to high.
    let mut prev: Option<(f32, f32, f32)> = None;
    for (n, i) in days.clone().enumerate() {
        let x = x_of(n) as f32 + 1.0;
        if let Some(c) = normal(r.dates[i]) {
            if let Some((px, ph, pl)) = prev {
                line(cv, (px, ty(ph)), (x, ty(c.hi)), rgb(150, 110, 110), 1);
                line(cv, (px, ty(pl)), (x, ty(c.lo)), rgb(100, 120, 160), 1);
            }
            prev = Some((x, c.hi, c.lo));
        }
    }
    for (n, i) in days.clone().enumerate() {
        if r.hi[i].is_nan() || r.lo[i].is_nan() {
            continue;
        }
        let (y0, y1) = (ty(r.hi[i]).round() as i32, ty(r.lo[i]).round() as i32);
        for y in y0..=y1 {
            let t = lo + (bottom as f32 - y as f32) / (bottom - top) as f32 * (hi - lo);
            cv.rect(x_of(n), y, 2, 1, temp_color(units, t));
        }
    }
    // The rain, in blue bars under it.
    let most = days.clone().map(|i| r.rain[i]).filter(|v| !v.is_nan()).fold(if units == Units::Us { 1.0 } else { 25.0 }, f32::max);
    cv.label(0, 12, "rain", rgb(120, 170, 255), None);
    for (n, i) in days.clone().enumerate() {
        let p = r.rain[i];
        if p.is_nan() || p <= 0.0 {
            continue;
        }
        let h = ((p / most) * 6.0).ceil().max(1.0) as i32;
        cv.rect(x_of(n), 28 - h, 2, h, rgb(70, 140, 255));
    }
    for x in 5..78 {
        cv.set(x, 28, rgb(34, 42, 72));
    }
    for n in (0..DAYS).step_by(7) {
        let d = r.dates[days.start + n];
        cv.label(x_of(n) as usize, 14, &d.format("%b %-d").to_string(), rgb(130, 140, 160), None);
    }
    // The month.
    let (ink, dim, lit) = (rgb(235, 240, 255), rgb(150, 160, 180), rgb(255, 230, 140));
    let deg = units.temp();
    let mut month: Vec<(String, String)> = Vec::new();
    let pick = |v: &[f32], better: fn(f32, f32) -> bool| {
        let mut best: Option<usize> = None;
        for i in days.clone() {
            if !v[i].is_nan() && best.is_none_or(|b| better(v[i], v[b])) {
                best = Some(i);
            }
        }
        best
    };
    if let Some(i) = pick(&r.hi, |a, b| a > b) {
        month.push(("warmest".into(), format!("{:.0}{deg} on {}", r.hi[i], r.dates[i].format("%b %-d"))));
    }
    if let Some(i) = pick(&r.lo, |a, b| a < b) {
        month.push(("coldest".into(), format!("{:.0}{deg} on {}", r.lo[i], r.dates[i].format("%b %-d"))));
    }
    if a.is_some() {
        let (mut off, mut n) = (0.0, 0);
        for i in days.clone() {
            if let (false, Some(c)) = (r.hi[i].is_nan(), normal(r.dates[i])) {
                off += r.hi[i] - c.hi;
                n += 1;
            }
        }
        if n > 0 {
            let off = off / n as f32;
            let word = if off.abs() < 0.5 { "right at normal".to_string() } else { format!("{:.0}{deg} {} normal", off.abs(), if off > 0.0 { "above" } else { "below" }) };
            month.push(("highs".into(), word));
        }
    }
    let wet = days.clone().filter(|&i| r.rain[i] > 0.0).count();
    let total = r.rain_over(DAYS);
    month.push(("rain".into(), format!("{} on {wet} day{}", units.rain_amount(total), if wet == 1 { "" } else { "s" })));
    if let Some(a) = a {
        let normal = a.rain_before(r.today(), DAYS);
        if normal > 0.0 {
            month.push(("normal".into(), format!("{} ({:.0}% of it)", units.rain_amount(normal), total / normal * 100.0)));
        }
    }
    let mut day: Vec<(String, String)> = Vec::new();
    match a {
        Some(a) => {
            let c = a.day(r.today());
            day.push(("normal".into(), format!("{:.0}{deg} / {:.0}{deg}", c.hi, c.lo)));
            day.push(("warmest".into(), format!("{:.0}{deg} in {}", c.record_hi.0, c.record_hi.1)));
            day.push(("coldest".into(), format!("{:.0}{deg} in {}", c.record_lo.0, c.record_lo.1)));
            if c.wettest.0 > 0.0 {
                day.push(("wettest".into(), format!("{} in {}", units.rain_amount(c.wettest.0), c.wettest.1)));
            }
            if let Some((h, l)) = a.last_year {
                day.push(("last year".into(), format!("{h:.0}{deg} / {l:.0}{deg}")));
            }
        }
        None if failed => day.push((String::new(), "the almanac isn't answering.".into())),
        None => day.push((String::new(), "digging through the almanac...".into())),
    }
    let today = r.today().format("%b %-d").to_string();
    panel(cv, 0, 38, &format!("the last {DAYS} days"), &month, (ink, dim, lit));
    panel(cv, 40, 79, &format!("on this day · {today}"), &day, (ink, dim, lit));
}

/// A box over rows 15 to 21: its title in the top edge, a label and a
/// value a row.
pub(super) fn panel(cv: &mut Canvas, x0: usize, x1: usize, title: &str, rows: &[(String, String)], (ink, dim, lit): (Rgb, Rgb, Rgb)) {
    let (r0, r1) = (15, 21);
    let (panel, edge) = (rgb(14, 20, 42), rgb(90, 120, 175));
    for row in r0..=r1 {
        for col in x0..=x1 {
            let ch = match (row == r0, row == r1, col == x0, col == x1) {
                (true, _, true, _) => "┌",
                (true, _, _, true) => "┐",
                (_, true, true, _) => "└",
                (_, true, _, true) => "┘",
                (true, ..) | (_, true, ..) => "─",
                (_, _, true, _) | (.., true) => "│",
                _ => " ",
            };
            cv.label(col, row, ch, edge, Some(panel));
        }
    }
    let title: String = title.chars().take(x1 - x0 - 6).collect();
    cv.label(x0 + 1, r0, "[ ", edge, Some(panel));
    cv.label(x0 + 3, r0, &title, lit, Some(panel));
    cv.label(x0 + 3 + title.chars().count(), r0, " ]", edge, Some(panel));
    let width = x1 - x0 - 3;
    for (i, (label, value)) in rows.iter().take(r1 - r0 - 1).enumerate() {
        let text: String = if label.is_empty() { value.clone() } else { format!("{label:<10}{value}") };
        let text: String = text.chars().take(width).collect();
        cv.label(x0 + 2, r0 + 1 + i, &text, ink, Some(panel));
        if !label.is_empty() {
            cv.label(x0 + 2, r0 + 1 + i, label, dim, Some(panel));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_almanac_sums_up_the_years() {
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        let ds = [d(1950, 10, 29), d(1995, 10, 29), d(2005, 10, 29), d(2025, 10, 29), d(2000, 2, 29)];
        let hi = [80.0, 60.0, 50.0, 61.0, 40.0];
        let lo = [12.0, 30.0, 40.0, 41.0, 20.0];
        let rain = [0.0, 1.0, 0.5, f32::NAN, 0.2];
        let a = climate(&ds, &hi, &lo, &rain, d(2026, 10, 29));
        let c = a.day(d(2026, 10, 29));
        assert_eq!((c.hi, c.lo, c.rain), (55.0, 35.0, 0.75), "normals are 1991-2020's");
        assert_eq!(c.record_hi, (80.0, 1950));
        assert_eq!(c.record_lo, (12.0, 1950));
        assert_eq!(c.wettest, (1.0, 1995));
        assert_eq!(a.last_year, Some((61.0, 41.0)));
        assert_eq!(a.day(d(2024, 2, 29)).record_hi, (40.0, 2000), "Feb 29 is its own day");
        assert_eq!(a.since, 1950);
    }
}
