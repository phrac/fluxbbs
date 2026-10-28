//! New: the weather (`WX`; the `weather` component), after Lone Wolf
//! Software's AI-WX door but our own. The sky over the caller's place in
//! half-block pixels, animated (the sun turning, clouds drifting, rain
//! and snow falling, lightning now and then), the temperature large,
//! the conditions beside it, the next 24 hours as a line and the week as
//! a strip of days. With the AI built in and on, `f` has it read the
//! forecast like a TV weatherman.
//!
//! Forecasts come from Open-Meteo (open-meteo.com: free, no key), kept
//! ten minutes a place. A caller's place is the one they last picked,
//! else their profile's location, else they're asked; `WX <place>` on a
//! menu shows a fixed place instead.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::input::{Key, ReadOpts};
use crate::pixels::{Canvas, Rgb, Stage, hash, ink, mix, painted, rgb, scale};
use crate::session::Session;

const MSG_ROW: u16 = 23;
const STATUS_ROW: u16 = 24;
/// How long a forecast is kept before it's fetched again.
const KEEP: Duration = Duration::from_secs(600);

// ---- places and forecasts ------------------------------------------------

/// A place forecasts are for.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// One day of the week's forecast.
#[derive(Clone, Debug, PartialEq)]
struct Day {
    date: String,
    code: u8,
    hi: f32,
    lo: f32,
    /// Chance of rain or snow, in percent.
    wet: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct Forecast {
    temp: f32,
    feels: f32,
    humidity: f32,
    wind: f32,
    /// Where the wind's from, in degrees.
    wind_from: f32,
    code: u8,
    day: bool,
    /// The local time it's for (`2026-10-26T15:00`).
    time: String,
    /// The next 24 hours' temperatures.
    hours: Vec<f32>,
    days: Vec<Day>,
    units: Units,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Units {
    Us,
    Metric,
}

impl Units {
    fn from(s: &str) -> Units {
        if s.eq_ignore_ascii_case("metric") { Units::Metric } else { Units::Us }
    }
    fn temp(self) -> &'static str {
        if self == Units::Us { "°F" } else { "°C" }
    }
    fn speed(self) -> &'static str {
        if self == Units::Us { "mph" } else { "km/h" }
    }
}

/// Places callers picked, and forecasts lately fetched.
pub struct WeatherStore {
    db: Mutex<Connection>,
    cache: Mutex<HashMap<String, (Instant, Forecast)>>,
}

impl WeatherStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut db = crate::sysdata::open_db(path)?;
        crate::sysdata::migrate(&mut db, "weather", SCHEMA, &[])?;
        Ok(Self { db: Mutex::new(db), cache: Mutex::default() })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let mut db = Connection::open_in_memory()?;
        crate::sysdata::migrate(&mut db, "weather", SCHEMA, &[])?;
        Ok(Self { db: Mutex::new(db), cache: Mutex::default() })
    }

    pub fn place(&self, user: u32) -> Option<Place> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT name, lat, lon FROM weather_places WHERE user = ?1", params![user], |r| {
            Ok(Place { name: r.get(0)?, lat: r.get(1)?, lon: r.get(2)? })
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn set_place(&self, user: u32, p: &Place) {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let r = db.execute(
            "INSERT INTO weather_places (user, name, lat, lon) VALUES (?1,?2,?3,?4)
             ON CONFLICT(user) DO UPDATE SET name=?2, lat=?3, lon=?4",
            params![user, p.name, p.lat, p.lon],
        );
        if let Err(e) = r {
            tracing::warn!("weather: keeping a place: {e}");
        }
    }

    fn cached(&self, key: &str) -> Option<Forecast> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(key).filter(|(at, _)| at.elapsed() < KEEP).map(|(_, f)| f.clone())
    }

    fn keep(&self, key: String, f: &Forecast) {
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.retain(|_, (at, _)| at.elapsed() < KEEP);
        c.insert(key, (Instant::now(), f.clone()));
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS weather_places (
    user INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    lat  REAL NOT NULL,
    lon  REAL NOT NULL
);
";

const STATES: [(&str, &str); 51] = [
    ("AL", "Alabama"), ("AK", "Alaska"), ("AZ", "Arizona"), ("AR", "Arkansas"), ("CA", "California"), ("CO", "Colorado"),
    ("CT", "Connecticut"), ("DE", "Delaware"), ("DC", "District of Columbia"), ("FL", "Florida"), ("GA", "Georgia"),
    ("HI", "Hawaii"), ("ID", "Idaho"), ("IL", "Illinois"), ("IN", "Indiana"), ("IA", "Iowa"), ("KS", "Kansas"),
    ("KY", "Kentucky"), ("LA", "Louisiana"), ("ME", "Maine"), ("MD", "Maryland"), ("MA", "Massachusetts"),
    ("MI", "Michigan"), ("MN", "Minnesota"), ("MS", "Mississippi"), ("MO", "Missouri"), ("MT", "Montana"),
    ("NE", "Nebraska"), ("NV", "Nevada"), ("NH", "New Hampshire"), ("NJ", "New Jersey"), ("NM", "New Mexico"),
    ("NY", "New York"), ("NC", "North Carolina"), ("ND", "North Dakota"), ("OH", "Ohio"), ("OK", "Oklahoma"),
    ("OR", "Oregon"), ("PA", "Pennsylvania"), ("RI", "Rhode Island"), ("SC", "South Carolina"), ("SD", "South Dakota"),
    ("TN", "Tennessee"), ("TX", "Texas"), ("UT", "Utah"), ("VT", "Vermont"), ("VA", "Virginia"), ("WA", "Washington"),
    ("WV", "West Virginia"), ("WI", "Wisconsin"), ("WY", "Wyoming"),
];

/// "Lincoln, NE" or "Lincoln, Nebraska" or "Paris, France": the town and
/// the state, province or country after it, a US state spelled out.
fn split_place(q: &str) -> (String, String) {
    let (town, region) = q.split_once(',').map_or((q, ""), |(a, b)| (a, b));
    let region = region.trim();
    let region = STATES.iter().find(|(ab, _)| ab.eq_ignore_ascii_case(region)).map_or(region, |(_, full)| full);
    let region = country_alias(region).unwrap_or(region);
    (town.trim().to_string(), region.to_string())
}

/// Ways of writing the two countries the geocoder reads as towns ("USA"
/// is Uşak, Turkey), as their codes.
const COUNTRY_ALIASES: &[(&str, &str)] = &[
    ("us", "US"), ("usa", "US"), ("america", "US"), ("united states", "US"), ("united states of america", "US"),
    ("uk", "GB"), ("gb", "GB"), ("britain", "GB"), ("great britain", "GB"), ("united kingdom", "GB"),
];

/// The UK's nations, which the geocoder reads as towns ("England" is in
/// Arkansas).
const UK_NATIONS: &[&str] = &["England", "Scotland", "Wales", "Northern Ireland"];

/// `s`'s country code, if it's one of `COUNTRY_ALIASES`.
fn country_alias(s: &str) -> Option<&'static str> {
    let bare = s.trim().replace('.', "").to_ascii_lowercase();
    COUNTRY_ALIASES.iter().find(|(k, _)| *k == bare).map(|(_, code)| *code)
}

/// The state or country `q` names when it's only that ("Nebraska", "NE,
/// USA", "USA", "England, UK"): the geocoder reads these as towns.
fn region_only(q: &str) -> Option<&'static str> {
    let (town, region) = q.split_once(',').map_or((q, ""), |(a, b)| (a, b));
    let within = if region.trim().is_empty() { None } else { Some(country_alias(region)?) };
    let town = town.trim().replace('.', "");
    if within.is_none() {
        match country_alias(&town) {
            Some("US") => return Some("the United States"),
            Some(_) => return Some("the United Kingdom"),
            None => {}
        }
    }
    if within != Some("US")
        && let Some(n) = UK_NATIONS.iter().find(|n| n.eq_ignore_ascii_case(&town))
    {
        return Some(n);
    }
    if within == Some("GB") {
        return None;
    }
    STATES.iter().find(|(ab, full)| ab.eq_ignore_ascii_case(&town) || full.eq_ignore_ascii_case(&town)).map(|(_, full)| *full)
}

/// What a place typed or in a profile comes to.
#[derive(Debug, PartialEq)]
enum Found {
    At(Place),
    /// Only a state or a country: too wide for a forecast, so ask the town.
    Region(String),
    Nowhere,
}

/// The best of the geocoder's answers for `region` (blank: the first).
fn pick(results: &[Value], region: &str) -> Option<Found> {
    let fits = |r: &&Value| {
        region.is_empty()
            || ["admin1", "country", "country_code", "admin2"].iter().any(|k| r[*k].as_str().is_some_and(|v| v.eq_ignore_ascii_case(region)))
    };
    let country = |r: &Value| r["feature_code"].as_str().is_some_and(|c| c.starts_with("PCL"));
    let mut r = results.iter().find(fits)?;
    if country(r) {
        // A country named like its capital (Luxembourg, Kuwait) is the
        // capital; a hamlet called France isn't France.
        let city = results.iter().find(|c| {
            c["feature_code"] == "PPLC" && c["country_code"] == r["country_code"] && c["name"] == r["name"]
        });
        match city {
            Some(c) => r = c,
            None => return Some(Found::Region(r["name"].as_str()?.to_string())),
        }
    }
    let mut name = r["name"].as_str()?.to_string();
    let region = r["admin1"].as_str().or(r["country"].as_str()).unwrap_or_default();
    if let Some((ab, _)) = STATES.iter().find(|(_, full)| full.eq_ignore_ascii_case(region)).filter(|_| r["country_code"].as_str() == Some("US")) {
        name = format!("{name}, {ab}");
    } else if !region.is_empty() {
        name = format!("{name}, {region}");
    }
    Some(Found::At(Place { name, lat: r["latitude"].as_f64()?, lon: r["longitude"].as_f64()? }))
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder().timeout(Duration::from_secs(12)).user_agent("flux/bbs weather").build()?)
}

/// Where `q` is.
async fn geocode(q: &str) -> anyhow::Result<Found> {
    let wide = region_only(q);
    let (town, region) = split_place(q);
    // A state spelled out may be its big city too ("New York"); an
    // abbreviation or a country never is.
    let spelled = |t: &str| STATES.iter().any(|(_, full)| full.eq_ignore_ascii_case(t));
    if let Some(r) = wide
        && !spelled(&town.replace('.', ""))
    {
        return Ok(Found::Region(r.into()));
    }
    if town.is_empty() {
        return Ok(Found::Nowhere);
    }
    let url = reqwest::Url::parse_with_params(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[("name", town.as_str()), ("count", "20"), ("language", "en"), ("format", "json")],
    )?;
    let v: Value = client()?.get(url).send().await?.error_for_status()?.json().await?;
    let results = v["results"].as_array().cloned().unwrap_or_default();
    if let Some(r) = wide {
        return Ok(big_city(&results, &town).unwrap_or(Found::Region(r.into())));
    }
    // A region nothing matched is a miss, not the first town of that name
    // anywhere ("Lincoln, Nebrasca" isn't Lincoln, England).
    Ok(pick(&results, &region).unwrap_or(Found::Nowhere))
}

/// A US city of a million or more named `town`, as "New York" is.
fn big_city(results: &[Value], town: &str) -> Option<Found> {
    let r = results.iter().find(|r| {
        r["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(town))
            && r["country_code"] == "US"
            && r["population"].as_u64().is_some_and(|p| p >= 1_000_000)
    })?;
    pick(std::slice::from_ref(r), "")
}

/// The forecast for `p`.
async fn fetch(p: &Place, units: Units) -> anyhow::Result<Forecast> {
    let mut q = vec![
        ("latitude", format!("{:.4}", p.lat)),
        ("longitude", format!("{:.4}", p.lon)),
        ("current", "temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,wind_direction_10m,weather_code,is_day".into()),
        ("hourly", "temperature_2m".into()),
        ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max".into()),
        ("timezone", "auto".into()),
        ("forecast_days", "7".into()),
    ];
    if units == Units::Us {
        q.push(("temperature_unit", "fahrenheit".into()));
        q.push(("wind_speed_unit", "mph".into()));
    }
    let url = reqwest::Url::parse_with_params("https://api.open-meteo.com/v1/forecast", &q)?;
    let v: Value = client()?.get(url).send().await?.error_for_status()?.json().await?;
    parse(&v, units).ok_or_else(|| anyhow::anyhow!("the forecast came back in a shape it doesn't read"))
}

/// Open-Meteo's answer, read.
fn parse(v: &Value, units: Units) -> Option<Forecast> {
    let c = &v["current"];
    let f = |x: &Value| x.as_f64().map(|n| n as f32);
    let time = c["time"].as_str()?.to_string();
    // The next 24 hours, from this one.
    let times = v["hourly"]["time"].as_array()?;
    let temps = v["hourly"]["temperature_2m"].as_array()?;
    let hour = &time[..time.len().min(13)];
    let from = times.iter().position(|t| t.as_str().is_some_and(|t| t.starts_with(hour))).unwrap_or(0);
    let hours: Vec<f32> = temps.iter().skip(from).take(24).filter_map(f).collect();
    let d = &v["daily"];
    let n = d["time"].as_array()?.len();
    let days = (0..n)
        .filter_map(|i| {
            Some(Day {
                date: d["time"][i].as_str()?.to_string(),
                code: d["weather_code"][i].as_u64()? as u8,
                hi: f(&d["temperature_2m_max"][i])?,
                lo: f(&d["temperature_2m_min"][i])?,
                wet: f(&d["precipitation_probability_max"][i]).unwrap_or(0.0),
            })
        })
        .collect();
    Some(Forecast {
        temp: f(&c["temperature_2m"])?,
        feels: f(&c["apparent_temperature"]).unwrap_or(f32::NAN),
        humidity: f(&c["relative_humidity_2m"]).unwrap_or(f32::NAN),
        wind: f(&c["wind_speed_10m"]).unwrap_or(0.0),
        wind_from: f(&c["wind_direction_10m"]).unwrap_or(0.0),
        code: c["weather_code"].as_u64()? as u8,
        day: c["is_day"].as_u64() != Some(0),
        time,
        hours,
        days,
        units,
    })
}

/// The sky a WMO weather code means.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sky {
    Clear,
    Partly,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
}

fn sky(code: u8) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::Partly,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51..=57 => Sky::Drizzle,
        61..=67 | 80..=82 => Sky::Rain,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Storm,
        _ => Sky::Cloudy,
    }
}

/// A WMO weather code in words.
fn words(code: u8) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80..=82 => "showers",
        85 | 86 => "snow showers",
        95 => "thunderstorms",
        96..=99 => "thunderstorms, hail",
        _ => "unsettled",
    }
}

/// Where the wind's from, as a compass point.
fn compass(deg: f32) -> &'static str {
    const P: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    P[((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

/// A day's short name, from its date.
fn weekday(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map(|d| d.format("%a").to_string()).unwrap_or_default()
}

// ---- the picture ---------------------------------------------------------

const W: i32 = 80;
const H: i32 = 44;

/// The sky behind it all: day or night, clear, grey or stormy.
fn sky_color(s: Sky, day: bool, y: i32) -> Rgb {
    let k = y as f32 / H as f32;
    let (top, bottom) = match (s, day) {
        (Sky::Clear | Sky::Partly, true) => (rgb(40, 110, 200), rgb(140, 190, 235)),
        (Sky::Clear | Sky::Partly, false) => (rgb(4, 6, 24), rgb(22, 26, 62)),
        (Sky::Storm, _) => (rgb(20, 22, 32), rgb(52, 56, 70)),
        (Sky::Fog, true) => (rgb(150, 155, 160), rgb(190, 192, 195)),
        (_, true) => (rgb(90, 100, 115), rgb(150, 158, 170)),
        (_, false) => (rgb(14, 16, 26), rgb(40, 44, 58)),
    };
    mix(top, bottom, k)
}

/// A puff of cloud centered at (cx, cy), `r` across.
fn cloud(cv: &mut Canvas, cx: f32, cy: f32, r: f32, c: Rgb) {
    for (dx, dy, rr) in [(-0.55, 0.15, 0.55), (0.0, -0.1, 0.7), (0.55, 0.15, 0.5), (0.25, 0.3, 0.5), (-0.25, 0.3, 0.5)] {
        let (px, py, pr) = (cx + dx * r, cy + dy * r, rr * r);
        for y in (py - pr) as i32..=(py + pr) as i32 {
            for x in (px - pr * 1.3) as i32..=(px + pr * 1.3) as i32 {
                let (ex, ey) = ((x as f32 - px) / (pr * 1.3), (y as f32 - py) / pr);
                let d = ex * ex + ey * ey;
                if d <= 1.0 {
                    cv.blend(x, y, scale(c, 1.0 - 0.18 * ey.max(0.0)), 0.95);
                }
            }
        }
    }
}

/// The big picture of the sky, in the box (x0, y0) `size` across.
fn icon(cv: &mut Canvas, t: f32, s: Sky, day: bool, x0: i32, y0: i32, size: i32) {
    let (cx, cy) = (x0 as f32 + size as f32 / 2.0, y0 as f32 + size as f32 / 2.0);
    let r = size as f32 / 2.0;
    // The sun (turning) or the moon, behind any cloud.
    if matches!(s, Sky::Clear | Sky::Partly) {
        let (bx, by) = if s == Sky::Partly { (cx - r * 0.25, cy - r * 0.25) } else { (cx, cy) };
        if day {
            for a in 0..12 {
                let ang = a as f32 / 12.0 * std::f32::consts::TAU + t * 0.4;
                for k in 0..4 {
                    let d = r * (0.62 + k as f32 * 0.08);
                    cv.blend((bx + ang.cos() * d) as i32, (by + ang.sin() * d) as i32, rgb(255, 220, 90), 0.85);
                }
            }
            for y in (by - r * 0.45) as i32..=(by + r * 0.45) as i32 {
                for x in (bx - r * 0.45) as i32..=(bx + r * 0.45) as i32 {
                    let d = ((x as f32 - bx).powi(2) + (y as f32 - by).powi(2)).sqrt() / (r * 0.45);
                    if d <= 1.0 {
                        cv.set(x, y, mix(rgb(255, 250, 200), rgb(255, 190, 40), d));
                    }
                }
            }
        } else {
            for y in (by - r * 0.4) as i32..=(by + r * 0.4) as i32 {
                for x in (bx - r * 0.4) as i32..=(bx + r * 0.4) as i32 {
                    let (dx, dy) = (x as f32 - bx, y as f32 - by);
                    let lit = (dx * dx + dy * dy).sqrt() <= r * 0.4;
                    let shadow = ((dx - r * 0.18).powi(2) + (dy + r * 0.08).powi(2)).sqrt() <= r * 0.34;
                    if lit && !shadow {
                        cv.set(x, y, rgb(235, 235, 215));
                    }
                }
            }
        }
    }
    let grey = match s {
        Sky::Storm => rgb(70, 72, 84),
        Sky::Rain | Sky::Drizzle => rgb(150, 156, 168),
        _ => rgb(225, 228, 235),
    };
    let drift = (t * 0.6).sin() * 1.5;
    match s {
        Sky::Clear => {}
        Sky::Partly => cloud(cv, cx + r * 0.25 + drift, cy + r * 0.25, r * 0.55, grey),
        Sky::Fog => {
            for band in 0..5 {
                let y = y0 + 4 + band * (size / 6);
                let shift = ((t * 4.0 + band as f32 * 7.0) as i32).rem_euclid(size);
                for x in x0..x0 + size {
                    let on = (x - x0 + shift).rem_euclid(size) < size * 3 / 4;
                    if on {
                        cv.blend(x, y, rgb(220, 222, 225), 0.8);
                        cv.blend(x, y + 1, rgb(220, 222, 225), 0.4);
                    }
                }
            }
        }
        _ => {
            cloud(cv, cx - r * 0.2 + drift, cy - r * 0.25, r * 0.6, scale(grey, 0.92));
            cloud(cv, cx + r * 0.15 - drift, cy - r * 0.05, r * 0.65, grey);
        }
    }
    // What falls from it.
    let base = (cy + r * 0.35) as i32;
    let fall = |cv: &mut Canvas, n: i32, speed: f32, c: Rgb, len: i32, wobble: f32| {
        for i in 0..n {
            let x = x0 + 3 + ((hash(i, 7) * (size - 6) as f32) as i32);
            let span = (y0 + size - base).max(1) as f32;
            let y = base as f32 + ((t * speed + hash(i, 3) * span) % span);
            let x = x + ((t * 2.0 + i as f32).sin() * wobble) as i32;
            for k in 0..len {
                cv.blend(x, y as i32 + k, c, 0.9);
            }
        }
    };
    match s {
        Sky::Drizzle => fall(cv, 8, 9.0, rgb(150, 190, 255), 1, 0.0),
        Sky::Rain => fall(cv, 16, 16.0, rgb(120, 170, 255), 2, 0.0),
        Sky::Snow => fall(cv, 16, 4.0, rgb(250, 250, 255), 1, 1.2),
        Sky::Storm => {
            fall(cv, 16, 18.0, rgb(110, 150, 230), 2, 0.0);
            // Lightning now and then.
            if (t * 0.7).fract() < 0.06 {
                let mut x = cx;
                for y in base..y0 + size {
                    x += if (y / 3) % 2 == 0 { 0.6 } else { -0.8 };
                    cv.set(x as i32, y, rgb(255, 255, 200));
                }
            }
        }
        _ => {}
    }
}

/// The whole screen.
fn draw(cv: &mut Canvas, t: f32, f: &Forecast, place: &str) {
    cv.clear_labels();
    let s = sky(f.code);
    let flash = s == Sky::Storm && (t * 0.7).fract() < 0.06;
    for y in 0..H {
        for x in 0..W {
            let mut c = sky_color(s, f.day, y);
            if !f.day && matches!(s, Sky::Clear | Sky::Partly) && hash(x, y) > 0.985 {
                c = mix(c, rgb(255, 255, 230), 0.5 + 0.5 * (t * 2.0 + hash(y, x) * 9.0).sin().abs());
            }
            cv.set(x, y, if flash { mix(c, rgb(220, 220, 255), 0.35) } else { c });
        }
    }
    // The picture, left; the numbers, right.
    icon(cv, t, s, f.day, 3, 2, 26);
    let text: Rgb = if f.day && !matches!(s, Sky::Storm) { rgb(255, 255, 255) } else { rgb(235, 235, 245) };
    let dim = scale(text, 0.8);
    cv.label(34, 1, &place.chars().take(44).collect::<String>(), text, None);
    // The temperature large (rows 2-8), the sky in words under it, then
    // the details, then the next 24 hours as a line (rows 14-15).
    let big = format!("{:.0}", f.temp);
    cv.print(34, 4, &big, 2, |_, _| text);
    let after = 34 + crate::pixels::text_width(&big) * 2 + 2;
    cv.label(after as usize, 2, f.units.temp(), text, None);
    cv.label(34, 9, words(f.code), text, None);
    let mut row = 10;
    if !f.feels.is_nan() {
        cv.label(34, row, &format!("feels like {:.0}{}", f.feels, f.units.temp()), dim, None);
        row += 1;
    }
    if !f.humidity.is_nan() {
        cv.label(34, row, &format!("humidity {:.0}%", f.humidity), dim, None);
        row += 1;
    }
    cv.label(34, row, &format!("wind {:.0} {} from the {}", f.wind, f.units.speed(), compass(f.wind_from)), dim, None);
    if f.hours.len() > 1 {
        let (lo, hi) = f.hours.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        cv.label(34, 13, &format!("next 24 hours {lo:.0}-{hi:.0}{}", f.units.temp()), dim, None);
        let (gx, gy, gw, gh) = (34, 28, 44, 4);
        for (i, v) in f.hours.iter().enumerate() {
            let x = gx + (i as f32 / (f.hours.len() - 1) as f32 * (gw - 1) as f32) as i32;
            let k = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            let y = gy + gh - 1 - (k * (gh - 1) as f32).round() as i32;
            cv.set(x, y, mix(rgb(120, 200, 255), rgb(255, 160, 80), k));
        }
    }
    // The week, a strip along the bottom.
    for y in 32..H {
        for x in 0..W {
            let c = cv.get(x, y).unwrap_or_default();
            cv.set(x, y, scale(c, 0.45));
        }
    }
    for (i, d) in f.days.iter().take(7).enumerate() {
        let x0 = 2 + i as i32 * 11;
        icon(cv, t, sky(d.code), true, x0 + 1, 32, 8);
        cv.label(x0 as usize + 1, 20, &weekday(&d.date), text, None);
        if d.wet >= 20.0 {
            cv.label(x0 as usize + 5, 20, &format!("{:.0}%", d.wet), rgb(150, 200, 255), None);
        }
        let hilo = format!("{:.0}/{:.0}", d.hi, d.lo);
        cv.label(x0 as usize + 1, 21, &hilo, dim, None);
    }
}

/// The forecaster's words in a box over the picture: solid, edged, its
/// title in the top edge, and a shadow down and to the right.
fn forecast_box(cv: &mut Canvas, title: &str, lines: &[String]) {
    let (panel, edge, ink, lit) = (rgb(14, 20, 42), rgb(90, 120, 175), rgb(235, 240, 255), rgb(255, 230, 140));
    let (x0, x1) = (3, 76);
    let h = lines.len() + 4;
    let r0 = (cv.h / 2 - 1).saturating_sub(h) / 2;
    let r1 = r0 + h - 1;
    // The shadow: the cell right of each row and the row under it, dimmed.
    for row in r0 + 1..=r1 + 1 {
        for col in x0 + 1..=x1 + 1 {
            if row > r1 || col > x1 {
                for y in [2 * row, 2 * row + 1] {
                    if let Some(c) = cv.get(col as i32, y as i32) {
                        cv.set(col as i32, y as i32, scale(c, 0.3));
                    }
                }
                if let Some(Some(g)) = cv.text.get_mut(row * cv.w + col) {
                    g.fg = scale(g.fg, 0.3);
                    g.bg = g.bg.map(|b| scale(b, 0.3));
                }
            }
        }
    }
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
    let title: String = title.chars().take(x1 - x0 - 8).collect();
    cv.label(x0 + 2, r0, "[ ", edge, Some(panel));
    cv.label(x0 + 4, r0, &title, lit, Some(panel));
    cv.label(x0 + 4 + title.chars().count(), r0, " ]", edge, Some(panel));
    for (i, l) in lines.iter().enumerate() {
        cv.label(x0 + 2, r0 + 2 + i, l, ink, Some(panel));
    }
}

// ---- the door ----------------------------------------------------------------

impl Session {
    /// `WX`: the weather where the caller is, or at the place a menu
    /// names (`WX Lincoln, NE`).
    pub async fn weather(&mut self, param: &str) -> io::Result<()> {
        let units = Units::from(&self.state.config().weather.units);
        self.node.update(|n| n.activity = "checking the weather".into());
        let fixed = !param.trim().is_empty();
        let mut place = match self.weather_place(param).await? {
            Some(p) => p,
            None => return Ok(()),
        };
        if self.plain() || self.screen.rows() < 24 {
            return self.weather_plain(&place, units).await;
        }
        let mut st = self.stage_open();
        let r = self.weather_stage(&mut st, &mut place, units, fixed).await;
        self.stage_close();
        r
    }

    /// The place: the menu's, the caller's own, their profile's, or one
    /// they type now. None if there's none to be had.
    async fn weather_place(&mut self, param: &str) -> io::Result<Option<Place>> {
        if !param.trim().is_empty() {
            return self.weather_find(param.trim()).await;
        }
        if let Some(p) = self.state.weather.place(self.user.id) {
            return Ok(Some(p));
        }
        let from_profile = [self.user.location.clone(), self.state.config().weather.place.clone()];
        for q in from_profile.iter().filter(|q| !q.trim().is_empty()) {
            if let Ok(Found::At(p)) = geocode(q).await {
                return Ok(Some(p));
            }
        }
        self.weather_ask().await
    }

    /// Find `q`, saying so if it can't be.
    async fn weather_find(&mut self, q: &str) -> io::Result<Option<Place>> {
        match geocode(q).await {
            Ok(Found::At(p)) => Ok(Some(p)),
            miss => {
                let s = self.weather_miss(q, miss);
                self.show_line(&s).await?;
                Ok(None)
            }
        }
    }

    /// What to say when `q` gave no place.
    fn weather_miss(&self, q: &str, found: anyhow::Result<Found>) -> String {
        match found {
            Ok(Found::Region(r)) => self.strings().fmt("WxWhichTown", &[&r]),
            Ok(_) => self.strings().fmt("WxNoPlace", &[q]),
            Err(e) => {
                tracing::warn!("weather: finding {q:?}: {e:#}");
                self.strings().get("WxDown").to_string()
            }
        }
    }

    /// Ask where, and keep it for next time.
    async fn weather_ask(&mut self) -> io::Result<Option<Place>> {
        loop {
            self.say("WxWhere").await?;
            let q = self.read_string(ReadOpts::new(40)).await?;
            self.newline();
            if q.trim().is_empty() {
                return Ok(None);
            }
            if let Some(p) = self.weather_find(q.trim()).await? {
                if self.user.id != 0 {
                    self.state.weather.set_place(self.user.id, &p);
                }
                return Ok(Some(p));
            }
        }
    }

    async fn weather_forecast(&mut self, place: &Place, units: Units) -> Option<Forecast> {
        let key = format!("{:.2},{:.2},{units:?}", place.lat, place.lon);
        if let Some(f) = self.state.weather.cached(&key) {
            return Some(f);
        }
        match fetch(place, units).await {
            Ok(f) => {
                self.state.weather.keep(key, &f);
                Some(f)
            }
            Err(e) => {
                tracing::warn!("weather: {}: {e:#}", place.name);
                None
            }
        }
    }

    async fn weather_stage(&mut self, st: &mut Stage, place: &mut Place, units: Units, fixed: bool) -> io::Result<()> {
        loop {
            let Some(f) = self.weather_forecast(place, units).await else {
                let s = painted(&crate::editor::strip_colours(self.strings().get("WxDown")), [255, 140, 120]);
                self.stage_line(MSG_ROW, &s, st.truecolor);
                self.read_key().await?;
                return Ok(());
            };
            let line = format!("as of {}  ·  forecast from open-meteo.com", f.time.replace('T', " "));
            self.stage_line(MSG_ROW, &painted(&line, [150, 160, 180]), st.truecolor);
            self.weather_status(st, fixed);
            let since = Instant::now();
            let key = loop {
                let t = st.t();
                draw(&mut st.cv, t, &f, &place.name);
                self.stage_show(st, true).await?;
                if self.key_waiting().await? {
                    break self.read_key().await?;
                }
                if since.elapsed() > Duration::from_secs(600) {
                    return Ok(());
                }
                tokio::time::sleep(self.idle_pause(110)).await;
            };
            match key.upper() {
                Some('L') if !fixed => {
                    self.screen.write_bytes(format!("\x1b[{MSG_ROW};1H\x1b[0m\x1b[K").as_bytes());
                    self.screen.goto(1, MSG_ROW);
                    self.say("WxWhere").await?;
                    let q = self.read_string(ReadOpts::new(40)).await?;
                    if !q.trim().is_empty() {
                        match geocode(q.trim()).await {
                            Ok(Found::At(p)) => {
                                if self.user.id != 0 {
                                    self.state.weather.set_place(self.user.id, &p);
                                }
                                *place = p;
                            }
                            miss => {
                                let s = crate::editor::strip_colours(&self.weather_miss(q.trim(), miss));
                                self.stage_line(MSG_ROW, &painted(&s, [255, 140, 120]), st.truecolor);
                                tokio::time::sleep(Duration::from_millis(1500)).await;
                            }
                        }
                    }
                    st.enc.forget();
                }
                #[cfg(feature = "ai")]
                Some('F') if self.state.config().ai.enabled && self.state.config().weather.ai_forecast => {
                    self.weather_forecaster(st, &f, &place.name).await?;
                    st.enc.forget();
                }
                _ if matches!(key, Key::Esc) => return Ok(()),
                Some('Q') => return Ok(()),
                _ => {}
            }
        }
    }

    fn weather_status(&mut self, st: &Stage, fixed: bool) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        let ai = cfg!(feature = "ai") && self.state.config().ai.enabled && self.state.config().weather.ai_forecast;
        let mut first = true;
        for (k, rest) in [("f", "orecaster"), ("l", "ocation"), ("q", "uit")] {
            if (k == "f" && !ai) || (k == "l" && fixed) {
                continue;
            }
            if !first {
                out.extend(b"  ");
            }
            first = false;
            ink(&mut out, [130, 140, 160], st.truecolor);
            out.extend(b"[");
            ink(&mut out, [255, 230, 140], st.truecolor);
            out.extend(k.as_bytes());
            ink(&mut out, [130, 140, 160], st.truecolor);
            out.extend(b"]");
            out.extend(rest.as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
    }

    /// `f`: the AI reads the forecast, over the picture.
    #[cfg(feature = "ai")]
    async fn weather_forecaster(&mut self, st: &mut Stage, f: &Forecast, place: &str) -> io::Result<()> {
        let cfg = self.state.config().ai.clone();
        if !self.ai_count(cfg.daily_limit) {
            let s = crate::editor::strip_colours(&self.strings().fmt("TarotLimit", &[&cfg.daily_limit.to_string()]));
            self.stage_line(MSG_ROW, &painted(&s, [255, 140, 120]), st.truecolor);
            self.read_key().await?;
            return Ok(());
        }
        self.stage_line(MSG_ROW, &painted("the forecaster is looking at the maps...", [150, 200, 255]), st.truecolor);
        let week: Vec<String> = f
            .days
            .iter()
            .map(|d| format!("{}: {}, high {:.0}, low {:.0}, {:.0}% chance of rain or snow", weekday(&d.date), words(d.code), d.hi, d.lo, d.wet))
            .collect();
        let system = "You're the weather forecaster on a local TV station's late news, reading the forecast for a caller to a \
            bulletin board system. Write 5 to 7 short sentences: now, the next day or two, and the week ahead, warm and a little \
            showy, with one bit of practical advice (a coat, an umbrella, sunscreen). Plain text for an 80-column terminal: no \
            markdown, no emoji, no lists. Use the units given and don't invent numbers.";
        let prompt = format!(
            "Place: {place}\nNow: {:.0}{}, {}, feels like {:.0}, humidity {:.0}%, wind {:.0} {} from the {}\nWeek:\n{}",
            f.temp,
            f.units.temp(),
            words(f.code),
            f.feels,
            f.humidity,
            f.wind,
            f.units.speed(),
            compass(f.wind_from),
            week.join("\n")
        );
        // The picture keeps moving while the forecaster thinks, and Esc
        // gives up on them. A model that spends its tokens thinking can
        // come back empty: once more, then give up.
        let mut status = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        ink(&mut status, [130, 140, 160], st.truecolor);
        status.extend(b"[esc] never mind\x1b[0m");
        self.screen.write_bytes(&status);
        let mut text = None;
        for _ in 0..2 {
            let ask = crate::aimemory::complete(&cfg, system, &prompt);
            tokio::pin!(ask);
            let answer = loop {
                tokio::select! {
                    a = &mut ask => break a,
                    _ = tokio::time::sleep(self.idle_pause(110)) => {
                        let t = st.t();
                        draw(&mut st.cv, t, f, place);
                        self.stage_show(st, true).await?;
                        if self.key_waiting().await? {
                            let k = self.read_key().await?;
                            if matches!(k, Key::Esc) || k.upper() == Some('Q') {
                                return Ok(());
                            }
                        }
                    }
                }
            };
            match answer {
                Ok(t) if !t.trim().is_empty() => {
                    text = Some(crate::editor::strip_colours(&t).replace("**", ""));
                    break;
                }
                Ok(_) => tracing::warn!("{}: weather: the forecaster's answer was empty", self.user.handle),
                Err(e) => {
                    tracing::warn!("{}: weather: the forecaster: {e:#}", self.user.handle);
                    break;
                }
            }
        }
        let Some(text) = text else {
            self.stage_line(MSG_ROW, &painted("the forecaster isn't on the air right now.", [255, 140, 120]), st.truecolor);
            self.read_key().await?;
            return Ok(());
        };
        self.screen.write_bytes(format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K").as_bytes());
        let lines: Vec<String> = text.lines().flat_map(|l| crate::multinode::wrap(l.trim(), 70)).filter(|l| !l.is_empty()).take(16).collect();
        let t = st.t();
        draw(&mut st.cv, t, f, place);
        forecast_box(&mut st.cv, &format!("the forecast for {place}"), &lines);
        self.stage_show(st, false).await?;
        self.stage_line(MSG_ROW, &painted("press a key", [150, 160, 180]), st.truecolor);
        self.read_key().await?;
        Ok(())
    }

    /// Without ANSI: the weather in words.
    async fn weather_plain(&mut self, place: &Place, units: Units) -> io::Result<()> {
        let Some(f) = self.weather_forecast(place, units).await else { return self.say_line("WxDown").await };
        self.show_line(&format!("|U3{}|U2: |U1{:.0}{}, {}", place.name, f.temp, units.temp(), words(f.code))).await?;
        if !f.feels.is_nan() {
            self.show_line(&format!("|U1feels like {:.0}{}, humidity {:.0}%, wind {:.0} {} from the {}", f.feels, units.temp(), f.humidity, f.wind, units.speed(), compass(f.wind_from))).await?;
        }
        for d in &f.days {
            self.show_line(&format!("|U3{:<4}|U1{:>4.0}/{:<4.0} {}{}", weekday(&d.date), d.hi, d.lo, words(d.code), if d.wet >= 20.0 { format!(", {:.0}% wet", d.wet) } else { String::new() })).await?;
        }
        self.show_line("|U2forecast from open-meteo.com").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_read_with_or_without_their_state() {
        assert_eq!(split_place("Lincoln, NE"), ("Lincoln".into(), "Nebraska".into()));
        assert_eq!(split_place("Paris, France"), ("Paris".into(), "France".into()));
        assert_eq!(split_place("  Omaha "), ("Omaha".into(), "".into()));
        let results: Vec<Value> = serde_json::from_str(
            r#"[{"name":"Lincoln","admin1":"England","country":"United Kingdom","country_code":"GB","latitude":53.2,"longitude":-0.5},
                {"name":"Lincoln","admin1":"Nebraska","country":"United States","country_code":"US","latitude":40.8,"longitude":-96.7}]"#,
        )
        .unwrap();
        assert_eq!(pick(&results, "Nebraska").unwrap(), Found::At(Place { name: "Lincoln, NE".into(), lat: 40.8, lon: -96.7 }));
        assert!(matches!(pick(&results, "").unwrap(), Found::At(p) if p.name == "Lincoln, England"));
        let results: Vec<Value> = serde_json::from_str(
            r#"[{"name":"Canada","feature_code":"PCLI","country":"Canada","country_code":"CA","latitude":60.1,"longitude":-113.6},
                {"name":"Canada","feature_code":"PPL","admin1":"Kentucky","country":"United States","country_code":"US","latitude":37.6,"longitude":-82.3}]"#,
        )
        .unwrap();
        assert_eq!(pick(&results, "").unwrap(), Found::Region("Canada".into()));
        assert!(matches!(pick(&results, "Kentucky").unwrap(), Found::At(p) if p.name == "Canada, KY"));
        let ny: Vec<Value> = serde_json::from_str(
            r#"[{"name":"New York","feature_code":"PPL","admin1":"New York","country":"United States","country_code":"US","population":8804190,"latitude":40.71,"longitude":-74.01},
                {"name":"Nebraska","feature_code":"PPL","admin1":"Indiana","country":"United States","country_code":"US","latitude":39.1,"longitude":-85.4}]"#,
        )
        .unwrap();
        assert!(matches!(big_city(&ny, "new york"), Some(Found::At(p)) if p.name == "New York, NY"));
        assert_eq!(big_city(&ny, "Nebraska"), None, "a hamlet isn't the state");
        let results: Vec<Value> = serde_json::from_str(
            r#"[{"name":"Luxembourg","feature_code":"PCLI","country":"Luxembourg","country_code":"LU","latitude":49.75,"longitude":6.17},
                {"name":"Luxembourg","feature_code":"PPLC","admin1":"Luxembourg","country":"Luxembourg","country_code":"LU","latitude":49.61,"longitude":6.13}]"#,
        )
        .unwrap();
        for region in ["", "Luxembourg"] {
            assert_eq!(pick(&results, region).unwrap(), Found::At(Place { name: "Luxembourg, Luxembourg".into(), lat: 49.61, lon: 6.13 }));
        }
        assert!(pick(&results, "Texas").is_none());
        assert_eq!(region_only("Nebraska"), Some("Nebraska"));
        assert_eq!(region_only(" ne "), Some("Nebraska"));
        assert_eq!(region_only("Nebraska, USA"), Some("Nebraska"));
        assert_eq!(region_only("N.Y., U.S.A."), Some("New York"));
        assert_eq!(region_only("Firth, NE"), None);
        assert_eq!(region_only("Nebraska City"), None);
        assert_eq!(region_only("Georgia, Europe"), None);
        assert_eq!(region_only("U.S.A."), Some("the United States"));
        assert_eq!(region_only("United States"), Some("the United States"));
        assert_eq!(region_only("UK"), Some("the United Kingdom"));
        assert_eq!(region_only(" england "), Some("England"));
        assert_eq!(region_only("England, UK"), Some("England"));
        assert_eq!(region_only("Wales, Great Britain"), Some("Wales"));
        assert_eq!(region_only("England, USA"), None, "a town of that name");
        assert_eq!(region_only("Nebraska, UK"), None);
        assert_eq!(region_only("London, England"), None);
        assert_eq!(split_place("London, UK"), ("London".into(), "GB".into()));
        assert_eq!(split_place("Firth, U.S.A."), ("Firth".into(), "US".into()));
    }

    #[test]
    fn a_forecast_reads() {
        let v: Value = serde_json::from_str(
            r#"{"current":{"time":"2026-10-26T15:00","temperature_2m":65.2,"apparent_temperature":63.0,"relative_humidity_2m":40,
                "wind_speed_10m":12.5,"wind_direction_10m":315,"weather_code":3,"is_day":1},
                "hourly":{"time":["2026-10-26T14:00","2026-10-26T15:00","2026-10-26T16:00"],"temperature_2m":[64.0,65.2,66.0]},
                "daily":{"time":["2026-10-26","2026-10-27"],"weather_code":[3,61],"temperature_2m_max":[67.0,58.0],
                "temperature_2m_min":[48.0,44.0],"precipitation_probability_max":[5,70]}}"#,
        )
        .unwrap();
        let f = parse(&v, Units::Us).unwrap();
        assert_eq!((f.temp, f.code, f.day), (65.2, 3, true));
        assert_eq!(f.hours, vec![65.2, 66.0], "from this hour on");
        assert_eq!(f.days.len(), 2);
        assert_eq!((sky(f.days[1].code), words(f.days[1].code)), (Sky::Rain, "light rain"));
        assert_eq!(compass(f.wind_from), "NW");
        assert_eq!(weekday(&f.days[0].date), "Mon");
    }

    #[test]
    fn every_sky_draws() {
        let f0 = |code: u8, day: bool| Forecast {
            temp: 70.0,
            feels: 68.0,
            humidity: 40.0,
            wind: 5.0,
            wind_from: 90.0,
            code,
            day,
            time: "2026-10-26T15:00".into(),
            hours: (0..24).map(|i| 60.0 + i as f32).collect(),
            days: vec![Day { date: "2026-10-26".into(), code, hi: 70.0, lo: 50.0, wet: 30.0 }],
            units: Units::Us,
        };
        for code in [0, 2, 3, 45, 53, 63, 73, 95] {
            for day in [true, false] {
                let mut cv = Canvas::new(W as usize, H as usize);
                draw(&mut cv, 1.3, &f0(code, day), "Lincoln, NE");
                assert!(cv.px.iter().any(|p| p[0] + p[1] + p[2] > 300.0), "something bright in {code}");
            }
        }
    }

    /// `WX_OUT=dir cargo test --features full live_weather -- --ignored`:
    /// a real forecast for Lincoln, drawn to `weather.ans` there.
    #[tokio::test]
    #[ignore]
    async fn live_weather() {
        let dir = std::path::PathBuf::from(std::env::var("WX_OUT").expect("WX_OUT"));
        let Found::At(p) = geocode("Lincoln, NE").await.unwrap() else { panic!("not found") };
        assert_eq!(geocode("France").await.unwrap(), Found::Region("France".into()));
        assert!(matches!(geocode("Luxembourg").await.unwrap(), Found::At(_)), "the capital");
        assert_eq!(geocode("Kuwait").await.unwrap(), Found::Region("Kuwait".into()), "its capital is Kuwait City");
        assert!(matches!(geocode("London, UK").await.unwrap(), Found::At(p) if p.name == "London, England"));
        assert!(matches!(geocode("New York").await.unwrap(), Found::At(p) if p.name == "New York, NY"));
        assert_eq!(geocode("Nebraska").await.unwrap(), Found::Region("Nebraska".into()));
        assert_eq!(geocode("NY").await.unwrap(), Found::Region("New York".into()));
        assert_eq!(geocode("Lincoln, Nebrasca").await.unwrap(), Found::Nowhere);
        assert_eq!(p.name, "Lincoln, NE");
        let f = fetch(&p, Units::Us).await.unwrap();
        assert_eq!(f.days.len(), 7);
        let mut cv = Canvas::new(W as usize, H as usize);
        draw(&mut cv, 2.0, &f, &p.name);
        let mut out = b"\x1b[2J".to_vec();
        out.extend(crate::pixels::Encoder::new(true, W as usize, H as usize).frame(&cv, 1));
        std::fs::write(dir.join("weather.ans"), out).unwrap();
        println!("{} {:.0} {}", p.name, f.temp, words(f.code));
    }

}
