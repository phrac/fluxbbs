//! `d`: the U.S. Drought Monitor (droughtmonitor.unl.edu, weekly, by
//! county) for the caller's county: this week's shares from abnormally
//! dry (D0) to exceptional drought (D4), the last 26 weeks, and the
//! state's. The county comes from the FCC's census lookup, so it's the
//! United States only; anywhere, the rain over the last 30 and 90 days
//! against the normal (from the history's almanac).

use std::io;
use std::time::Duration;

use chrono::NaiveDate;

use super::history::{Almanac, Recent, panel};
use super::{MSG_ROW, Place, Units, client};
use crate::pixels::{Canvas, H, Rgb, Stage, W, mix, painted, rgb, text_width};
use crate::session::Session;

/// The weeks the chart shows.
const WEEKS: usize = 26;

/// The categories' names and the monitor's own colors: none, then D0-D4.
const LEVELS: [(&str, &str, [u8; 3]); 6] = [
    ("none", "no drought", [70, 130, 80]),
    ("D0", "abnormally dry", [255, 255, 0]),
    ("D1", "moderate drought", [252, 211, 127]),
    ("D2", "severe drought", [255, 170, 0]),
    ("D3", "extreme drought", [230, 0, 0]),
    ("D4", "exceptional drought", [115, 0, 0]),
];

fn level_color(i: usize) -> Rgb {
    let c = LEVELS[i].2;
    rgb(c[0], c[1], c[2])
}

/// A week's shares, none then D0-D4, in percent.
#[derive(Clone, Copy)]
struct Week {
    date: NaiveDate,
    share: [f32; 6],
}

/// A county's weeks, and its state's this week.
#[derive(Clone)]
struct Drought {
    county: String,
    state: String,
    weeks: Vec<Week>,
    state_now: Option<Week>,
}

/// Where a place is: the county's FIPS and name, the state's FIPS and
/// abbreviation.
struct County {
    fips: String,
    name: String,
    state_fips: String,
    state: String,
}

/// The county at a place; None outside the US.
async fn county(p: &Place) -> anyhow::Result<Option<County>> {
    let url = reqwest::Url::parse_with_params(
        "https://geo.fcc.gov/api/census/block/find",
        &[("latitude", format!("{:.5}", p.lat)), ("longitude", format!("{:.5}", p.lon)), ("format", "json".into())],
    )?;
    let v: serde_json::Value = client()?.get(url).send().await?.error_for_status()?.json().await?;
    let text = |a: &str, b: &str| v[a][b].as_str().unwrap_or_default().to_string();
    Ok(v["County"]["FIPS"].as_str().map(|f| County { fips: f.to_string(), name: text("County", "name"), state_fips: text("State", "FIPS"), state: text("State", "code") }))
}

/// The monitor's weeks for an area (a county's FIPS or a state's
/// abbreviation), from its comma-separated answer.
async fn weeks(kind: &str, aoi: &str, today: NaiveDate) -> anyhow::Result<Vec<Week>> {
    let from = today - chrono::Duration::weeks(WEEKS as i64 + 1);
    let url = reqwest::Url::parse_with_params(
        &format!("https://usdmdataservices.unl.edu/api/{kind}/GetDroughtSeverityStatisticsByAreaPercent"),
        &[("aoi", aoi.to_string()), ("startdate", from.format("%-m/%-d/%Y").to_string()), ("enddate", today.format("%-m/%-d/%Y").to_string()), ("statisticsType", "2".into())],
    )?;
    let text = client()?.get(url).send().await?.error_for_status()?.text().await?;
    Ok(parse(&text))
}

/// The monitor's table: a header naming `MapDate`, `None` and `D0`-`D4`,
/// then a week a line, newest first. Read oldest first.
fn parse(text: &str) -> Vec<Week> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else { return Vec::new() };
    let cols: Vec<&str> = head.split(',').map(str::trim).collect();
    let at = |name: &str| cols.iter().position(|c| c.eq_ignore_ascii_case(name));
    let (Some(date), Some(none)) = (at("MapDate"), at("None")) else { return Vec::new() };
    let mut out: Vec<Week> = lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let date = NaiveDate::parse_from_str(f.get(date)?, "%Y%m%d").ok()?;
            let mut share = [0.0f32; 6];
            for (i, s) in share.iter_mut().enumerate() {
                *s = f.get(none + i)?.parse().ok()?;
            }
            Some(Week { date, share })
        })
        .collect();
    out.sort_by_key(|w| w.date);
    out.dedup_by_key(|w| w.date);
    out
}

/// The county's drought now, in words: the worst category with any
/// share, and how much of the county it covers.
fn headline(w: &Week) -> (usize, String) {
    let worst = (1..6).rev().find(|&i| w.share[i] >= 0.5);
    match worst {
        None => (0, "no drought".into()),
        Some(i) => {
            let covered: f32 = w.share[i..].iter().sum();
            (i, format!("{} {} in {covered:.0}%", LEVELS[i].0, LEVELS[i].1))
        }
    }
}

impl Session {
    async fn weather_drought_data(&mut self, place: &Place, today: NaiveDate) -> anyhow::Result<Option<Drought>> {
        let key = format!("drought {:.3} {:.3} {today}", place.lat, place.lon);
        if let Some(d) = self.state.weather.recall::<Option<Drought>>(&key, Duration::from_secs(6 * 3600)) {
            return Ok(d);
        }
        let d = match county(place).await? {
            None => None,
            Some(c) => {
                // The state's figures come back empty now and then by its
                // number, now and then by its abbreviation: one, then the
                // other.
                let (weeks_now, mut s) = tokio::join!(weeks("CountyStatistics", &c.fips, today), weeks("StateStatistics", &c.state_fips, today));
                if s.as_ref().map_or(true, |s| s.is_empty()) {
                    s = weeks("StateStatistics", &c.state, today).await;
                }
                let state_now = match s {
                    Ok(s) => s.last().copied(),
                    Err(e) => {
                        tracing::warn!("weather: the drought monitor for {}: {e:#}", c.state);
                        None
                    }
                };
                Some(Drought { county: c.name, state: c.state, weeks: weeks_now?, state_now })
            }
        };
        self.state.weather.remember(key, d.clone());
        Ok(d)
    }

    /// `d`: the drought.
    pub(super) async fn weather_drought(&mut self, st: &mut Stage, place: &Place, units: Units) -> io::Result<()> {
        self.weather_footer(st, "");
        self.stage_line(MSG_ROW, &painted("checking the drought monitor...", [150, 200, 255]), st.truecolor);
        let Some(r) = self.weather_recent(place, units).await else {
            self.stage_line(MSG_ROW, &painted("the past days aren't to be had right now.", [255, 140, 120]), st.truecolor);
            self.read_key().await?;
            return Ok(());
        };
        let today = r.today();
        let d = match self.weather_drought_data(place, today).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("weather: {}: the drought monitor: {e:#}", place.name);
                self.stage_line(MSG_ROW, &painted("the drought monitor isn't answering right now.", [255, 140, 120]), st.truecolor);
                self.read_key().await?;
                return Ok(());
            }
        };
        let (key, mut a) = self.weather_almanac_kept(place, units, today);
        if a.is_none() {
            self.stage_line(MSG_ROW, &painted("digging through the almanac for the normals...", [150, 200, 255]), st.truecolor);
            match self.weather_wait(st, super::history::almanac(place, units, today), |cv, _| draw(cv, d.as_ref(), &r, None, place, units)).await? {
                None => return Ok(()),
                Some(Ok(x)) => {
                    self.state.weather.remember(key, x.clone());
                    a = Some(x);
                }
                Some(Err(e)) => tracing::warn!("weather: {}: the almanac: {e:#}", place.name),
            }
        }
        self.stage_line(MSG_ROW, &painted("", [150, 160, 180]), st.truecolor);
        let note = match d.as_ref().and_then(|d| d.weeks.last()) {
            Some(w) => format!("   the U.S. Drought Monitor, map of {} (out Thursdays)", w.date.format("%b %-d")),
            None => "   rain from open-meteo.com, normals 1991-2020".into(),
        };
        self.weather_footer(st, &note);
        self.weather_hold(st, |cv, _| draw(cv, d.as_ref(), &r, a.as_ref(), place, units)).await?;
        Ok(())
    }
}

/// The page: the headline, this week's bar, the weeks, and the rain.
fn draw(cv: &mut Canvas, d: Option<&Drought>, r: &Recent, a: Option<&Almanac>, place: &Place, units: Units) {
    cv.clear_labels();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            cv.set(x, y, mix(rgb(8, 12, 30), rgb(18, 24, 52), y as f32 / H as f32));
        }
    }
    let (ink, dim, lit) = (rgb(235, 240, 255), rgb(150, 160, 180), rgb(255, 230, 140));
    let title = match d {
        Some(d) => format!(" drought monitor · {}, {} ", d.county, d.state),
        None => format!(" drought · {} ", place.name),
    };
    cv.label((W - title.chars().count().min(W)) / 2, 0, &title, lit, None);
    match d.and_then(|d| d.weeks.last().map(|w| (d, w))) {
        Some((d, now)) => {
            // The headline, large, in its category's color, then in words.
            let (worst, words) = headline(now);
            let big = if worst == 0 { "NO DROUGHT".to_string() } else { format!("{} {}", LEVELS[worst].0, LEVELS[worst].1.split(' ').next().unwrap_or("")).to_uppercase() };
            let c = level_color(worst);
            let w = text_width(&big);
            cv.print((W as i32 - w) / 2, 3, &big, 1, |_, _| c);
            let sub = if worst == 0 { format!("none of {} is dry this week", d.county) } else { format!("{words} of {}, this week", d.county) };
            cv.label((W - sub.chars().count().min(W)) / 2, 5, &sub, ink, None);
            // This week, a bar of its shares, and what each is.
            let (x0, x1) = (3, 76);
            let mut at = x0 as f32;
            for (i, s) in now.share.iter().enumerate() {
                let w = s / 100.0 * (x1 - x0) as f32;
                for x in at.round() as i32..(at + w).round() as i32 {
                    cv.rect(x, 12, 1, 4, level_color(i));
                }
                at += w;
            }
            let mut col = 3;
            for (i, s) in now.share.iter().enumerate() {
                let t = format!("{} {:.0}%", LEVELS[i].0, s);
                cv.label(col, 8, "■", level_color(i), None);
                cv.label(col + 2, 8, &t, if *s > 0.0 { ink } else { dim }, None);
                col += t.chars().count() + 5;
            }
            // The weeks: drought stacked from the bottom, worst lowest.
            let weeks: Vec<&Week> = d.weeks.iter().rev().take(WEEKS).collect::<Vec<_>>().into_iter().rev().collect();
            let (top, bottom) = (18, 27);
            let span = (bottom - top + 1) as f32;
            for (n, w) in weeks.iter().enumerate() {
                let x = 2 + n as i32 * 3 - (weeks.len() as i32 - WEEKS as i32).min(0) * 3;
                let mut y = bottom as f32 + 1.0;
                for i in (1..6).rev() {
                    let h = w.share[i] / 100.0 * span;
                    for yy in (y - h).round() as i32..y.round() as i32 {
                        cv.rect(x, yy, 2, 1, level_color(i));
                    }
                    y -= h;
                }
                for yy in top..y.round() as i32 {
                    cv.rect(x, yy, 2, 1, mix(level_color(0), rgb(8, 12, 30), 0.65));
                }
            }
            cv.label(2, 14, &format!("{WEEKS} weeks ago"), dim, None);
            cv.label(70, 14, "this week", dim, None);
        }
        None => {
            let t = "the U.S. Drought Monitor covers the United States; here's the rain instead.";
            cv.label((W - t.chars().count().min(W)) / 2, 5, t, dim, None);
        }
    }
    // The rain against the normal, 30 and 90 days.
    let mut rows: Vec<(String, String)> = Vec::new();
    for n in [30, 90] {
        let got = r.rain_over(n);
        let normal = a.map(|a| a.rain_before(r.today(), n)).filter(|v| *v > 0.0);
        let v = match normal {
            Some(m) => format!("{} · {:.0}% of the normal {}", units.rain_amount(got), got / m * 100.0, units.rain_amount(m)),
            None => units.rain_amount(got),
        };
        rows.push((format!("{n} days"), v));
    }
    if a.is_none() {
        rows.push((String::new(), "(the normals aren't in yet)".into()));
    }
    if let Some((d, s)) = d.and_then(|d| d.state_now.map(|s| (d, s))) {
        let dry: f32 = s.share[1..].iter().sum();
        let drought: f32 = s.share[2..].iter().sum();
        rows.push((d.state.clone(), format!("{drought:.0}% of the state in drought, {dry:.0}% dry or worse")));
    }
    panel(cv, 0, 79, "rain lately", &rows, (ink, dim, lit));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_monitors_table_reads() {
        let t = "MapDate,FIPS,County,State,None,D0,D1,D2,D3,D4,ValidStart,ValidEnd,StatisticFormatID\r\n\
                 20261020,06019,Fresno County,CA,51.80,48.20,0.00,0.00,0.00,0.00,2026-10-20,2026-10-26,2\r\n\
                 20261013,06019,Fresno County,CA,40.00,30.00,20.00,10.00,0.00,0.00,2026-10-13,2026-10-19,2\r\n";
        let w = parse(t);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].date, NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(), "oldest first");
        assert_eq!(w[1].share, [51.8, 48.2, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(headline(&w[1]), (1, "D0 abnormally dry in 48%".into()));
        assert_eq!(headline(&w[0]), (3, "D2 severe drought in 10%".into()));
        let state = "MapDate,StateAbbreviation,None,D0,D1,D2,D3,D4,ValidStart,ValidEnd,StatisticFormatID\n20261020,NE,38.75,17.93,22.45,15.72,4.52,0.64,2026-10-20,2026-10-26,2\n";
        assert_eq!(parse(state)[0].share[5], 0.64);
        assert_eq!(headline(&Week { date: w[0].date, share: [100.0, 0.0, 0.0, 0.0, 0.0, 0.0] }).1, "no drought");
    }
}
