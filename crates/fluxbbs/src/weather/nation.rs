//! `n`: the country's weather, as the TV map has it: the lower 48 colored
//! by the temperature (filled in between the cities), the cities' own
//! readings on it, radar where it's raining or snowing, and the
//! caller's place. One call to Open-Meteo for every city, kept ten
//! minutes.

use std::io;

use serde_json::Value;

use super::{MSG_ROW, Place, Sky, Units, client, sky, temp_color};
use crate::pixels::{Canvas, H, Stage, W, hash, painted, rgb, scale};
use crate::session::Session;

/// The cities on the map: name, latitude, longitude.
const CITIES: [(&str, f64, f64); 24] = [
    ("Seattle", 47.61, -122.33),
    ("San Francisco", 37.77, -122.42),
    ("Los Angeles", 34.05, -118.24),
    ("Las Vegas", 36.17, -115.14),
    ("Boise", 43.62, -116.20),
    ("Phoenix", 33.45, -112.07),
    ("Salt Lake City", 40.76, -111.89),
    ("Billings", 45.78, -108.50),
    ("Denver", 39.74, -104.99),
    ("Albuquerque", 35.08, -106.65),
    ("Bismarck", 46.81, -100.78),
    ("Dallas", 32.78, -96.80),
    ("Houston", 29.76, -95.37),
    ("Kansas City", 39.10, -94.58),
    ("Minneapolis", 44.98, -93.27),
    ("Chicago", 41.88, -87.63),
    ("St. Louis", 38.63, -90.20),
    ("Detroit", 42.33, -83.05),
    ("Nashville", 36.16, -86.78),
    ("Atlanta", 33.75, -84.39),
    ("New Orleans", 29.95, -90.07),
    ("Miami", 25.76, -80.19),
    ("Washington", 38.90, -77.04),
    ("New York", 40.71, -74.01),
];

/// The lower 48's outline (longitude, latitude), clockwise from Cape
/// Flattery, the border run through the middle of the Great Lakes.
const OUTLINE: &[(f32, f32)] = &[
    (-124.7, 48.4), (-122.8, 49.0), (-95.2, 49.0), (-93.0, 48.6), (-91.0, 48.2), (-89.6, 48.0), (-84.6, 46.6), (-84.4, 46.4),
    (-82.5, 45.3), (-82.4, 43.0), (-82.5, 42.6), (-82.95, 42.3), (-83.0, 42.0), (-81.0, 42.2), (-79.0, 42.7), (-79.0, 43.3), (-76.3, 44.0),
    (-74.7, 45.0), (-71.5, 45.0), (-70.7, 45.4), (-70.0, 46.7), (-69.2, 47.45), (-68.2, 47.35), (-67.8, 47.1), (-67.8, 45.7),
    (-67.0, 44.9), (-68.8, 44.3), (-70.2, 43.6), (-70.8, 42.9), (-71.0, 42.3), (-69.95, 41.7), (-70.6, 41.5), (-71.9, 41.3),
    (-73.6, 41.0), (-74.0, 40.6), (-74.0, 40.2), (-74.4, 39.4), (-74.9, 38.9), (-75.1, 38.4), (-75.4, 37.9), (-75.9, 37.1),
    (-76.3, 36.9), (-75.5, 35.8), (-75.5, 35.2), (-76.6, 34.7), (-77.9, 33.9), (-79.2, 33.2), (-80.0, 32.6), (-81.0, 31.9),
    (-81.4, 30.4), (-81.0, 29.2), (-80.5, 28.4), (-80.0, 26.8), (-80.1, 25.8), (-80.4, 25.2), (-81.1, 25.1), (-81.7, 25.9),
    (-82.1, 26.6), (-82.6, 27.8), (-82.8, 28.9), (-83.6, 29.9), (-84.3, 30.1), (-85.3, 29.7), (-86.4, 30.4), (-87.6, 30.3),
    (-88.0, 30.6), (-89.4, 30.3), (-89.4, 29.2), (-90.1, 29.1), (-91.3, 29.3), (-92.3, 29.5), (-93.8, 29.7), (-94.7, 29.4),
    (-95.5, 28.8), (-96.5, 28.3), (-97.2, 27.7), (-97.4, 26.8), (-97.2, 25.9), (-98.2, 26.1), (-99.1, 26.5), (-99.5, 27.5),
    (-100.3, 28.3), (-100.9, 29.4), (-101.4, 29.8), (-102.4, 29.8), (-103.0, 29.0), (-103.9, 29.3), (-104.6, 29.9), (-105.0, 30.7),
    (-106.5, 31.8), (-108.2, 31.8), (-108.2, 31.33), (-111.1, 31.33), (-114.8, 32.5), (-114.7, 32.7), (-117.1, 32.5), (-117.3, 33.2),
    (-118.3, 33.7), (-118.5, 34.0), (-119.2, 34.1), (-120.5, 34.5), (-120.6, 35.2), (-121.9, 36.3), (-121.9, 36.9), (-122.5, 37.6),
    (-123.0, 38.0), (-123.7, 38.9), (-123.8, 39.8), (-124.4, 40.4), (-124.2, 41.8), (-124.5, 42.8), (-124.1, 44.0), (-124.0, 45.5),
    (-124.0, 46.3), (-124.1, 47.0),
];

/// The Great Lakes as ellipses (center, half-widths in degrees); the
/// border cuts off Canada's side.
const LAKES: [(f32, f32, f32, f32); 5] = [(-87.8, 47.4, 3.2, 0.6), (-86.9, 43.9, 0.75, 2.2), (-82.8, 44.6, 0.7, 1.3), (-81.3, 41.95, 1.9, 0.3), (-77.8, 43.45, 1.2, 0.25)];

/// The map's corner and scale: west and north edges, pixels a degree.
const WEST: f32 = -125.0;
const NORTH: f32 = 49.6;
const PER_LON: f32 = 78.0 / 59.0;
const PER_LAT: f32 = 37.0 / 25.4;
const TOP: i32 = 2;

fn to_px(lon: f32, lat: f32) -> (f32, f32) {
    (1.0 + (lon - WEST) * PER_LON, TOP as f32 + (NORTH - lat) * PER_LAT)
}

fn to_geo(x: f32, y: f32) -> (f32, f32) {
    (WEST + (x - 1.0) / PER_LON, NORTH - (y - TOP as f32) / PER_LAT)
}

/// Whether a point's inside the outline (even-odd).
fn inside(lon: f32, lat: f32) -> bool {
    let mut odd = false;
    let n = OUTLINE.len();
    for i in 0..n {
        let (a, b) = (OUTLINE[i], OUTLINE[(i + n - 1) % n]);
        if (a.1 > lat) != (b.1 > lat) && lon < (b.0 - a.0) * (lat - a.1) / (b.1 - a.1) + a.0 {
            odd = !odd;
        }
    }
    odd
}

fn lake(lon: f32, lat: f32) -> bool {
    LAKES.iter().any(|&(x, y, rx, ry)| ((lon - x) / rx).powi(2) + ((lat - y) / ry).powi(2) <= 1.0)
}

/// A city's weather now.
#[derive(Clone)]
struct Reading {
    name: &'static str,
    lat: f32,
    lon: f32,
    temp: f32,
    sky: Sky,
}

/// Every city's, at once.
async fn readings(units: Units) -> anyhow::Result<Vec<Reading>> {
    let lats: Vec<String> = CITIES.iter().map(|c| format!("{:.2}", c.1)).collect();
    let lons: Vec<String> = CITIES.iter().map(|c| format!("{:.2}", c.2)).collect();
    let mut q = vec![
        ("latitude", lats.join(",")),
        ("longitude", lons.join(",")),
        ("current", "temperature_2m,weather_code".into()),
        ("timezone", "auto".into()),
    ];
    q.extend(units.query());
    let url = reqwest::Url::parse_with_params("https://api.open-meteo.com/v1/forecast", &q)?;
    let v: Value = client()?.get(url).send().await?.error_for_status()?.json().await?;
    let all = v.as_array().ok_or_else(|| anyhow::anyhow!("the cities came back in a shape it doesn't read"))?;
    Ok(CITIES
        .iter()
        .zip(all)
        .filter_map(|(c, v)| {
            Some(Reading { name: c.0, lat: c.1 as f32, lon: c.2 as f32, temp: v["current"]["temperature_2m"].as_f64()? as f32, sky: sky(v["current"]["weather_code"].as_u64()? as u8) })
        })
        .collect())
}

/// The temperature anywhere: the cities' readings, the nearer the more.
fn field(rs: &[Reading], lon: f32, lat: f32) -> f32 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for r in rs {
        let d = ((r.lon - lon) * 0.8).powi(2) + (r.lat - lat).powi(2);
        if d < 0.01 {
            return r.temp;
        }
        let w = 1.0 / (d * d);
        sum += r.temp * w;
        weight += w;
    }
    sum / weight
}

impl Session {
    /// `n`: the nation.
    pub(super) async fn weather_nation(&mut self, st: &mut Stage, place: &Place, units: Units) -> io::Result<()> {
        self.weather_footer(st, "");
        let key = format!("nation {units:?}");
        let rs = match self.state.weather.recall::<Vec<Reading>>(&key, super::KEEP) {
            Some(rs) => rs,
            None => {
                self.stage_line(MSG_ROW, &painted("calling around the country...", [150, 200, 255]), st.truecolor);
                match readings(units).await {
                    Ok(rs) if !rs.is_empty() => {
                        self.state.weather.remember(key, rs.clone());
                        rs
                    }
                    other => {
                        if let Err(e) = other {
                            tracing::warn!("weather: the nation: {e:#}");
                        }
                        self.stage_line(MSG_ROW, &painted("the rest of the country isn't answering right now.", [255, 140, 120]), st.truecolor);
                        self.read_key().await?;
                        return Ok(());
                    }
                }
            }
        };
        let now = crate::clock::now().format("%H:%M").to_string();
        self.stage_line(MSG_ROW, &painted("", [150, 160, 180]), st.truecolor);
        self.weather_footer(st, "   the cities now, from open-meteo.com");
        // The map's colors don't move: work them out once.
        let mut land = vec![None; W * H];
        for y in TOP..40 {
            for x in 0..W as i32 {
                let (lon, lat) = to_geo(x as f32 + 0.5, y as f32 + 0.5);
                if inside(lon, lat) {
                    land[y as usize * W + x as usize] = Some(if lake(lon, lat) { None } else { Some(field(&rs, lon, lat)) });
                }
            }
        }
        let here = (place.lat as f32, place.lon as f32);
        self.weather_hold(st, |cv, t| draw(cv, t, &rs, &land, here, units, &now)).await?;
        Ok(())
    }
}

/// The page: the map, the cities, the extremes, the scale.
fn draw(cv: &mut Canvas, t: f32, rs: &[Reading], land: &[Option<Option<f32>>], here: (f32, f32), units: Units, now: &str) {
    cv.clear_labels();
    let sea = rgb(10, 22, 52);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let c = match land[y as usize * W + x as usize] {
                Some(Some(temp)) => scale(temp_color(units, temp), 0.85),
                Some(None) => rgb(30, 70, 140),
                None => sea,
            };
            cv.set(x, y, c);
        }
    }
    // The coast and the border, darker.
    let is_land = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && land[y as usize * W + x as usize].is_some();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if is_land(x, y) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| !is_land(x + dx, y + dy)) {
                let c = cv.get(x, y).unwrap_or(sea);
                cv.set(x, y, scale(c, 0.55));
            }
        }
    }
    // Radar around the cities where it's raining or snowing, drifting east.
    for (i, r) in rs.iter().enumerate() {
        let (snow, storm) = (r.sky == Sky::Snow, r.sky == Sky::Storm);
        if !matches!(r.sky, Sky::Drizzle | Sky::Rain | Sky::Storm | Sky::Snow) {
            continue;
        }
        let (cx, cy) = to_px(r.lon, r.lat);
        for k in 0..3 {
            let bx = cx - 3.0 + hash(i as i32, k) * 6.0 + (t * 0.4 + k as f32).sin() * 1.5;
            let by = cy - 2.0 + hash(k, i as i32) * 4.0;
            let rad = 2.5 + hash(i as i32 + 7, k) * 2.5;
            for y in (by - rad) as i32..=(by + rad) as i32 {
                for x in (bx - rad * 1.4) as i32..=(bx + rad * 1.4) as i32 {
                    let d = (((x as f32 - bx) / 1.4).powi(2) + (y as f32 - by).powi(2)).sqrt() / rad;
                    if d > 1.0 || !is_land(x, y) {
                        continue;
                    }
                    let c = match () {
                        _ if snow => if d < 0.4 { rgb(255, 255, 255) } else { rgb(170, 200, 255) },
                        _ if storm && d < 0.3 => rgb(230, 40, 40),
                        _ if d < 0.45 && r.sky != Sky::Drizzle => rgb(250, 220, 40),
                        _ => rgb(40, 200, 70),
                    };
                    cv.blend(x, y, c, 0.75 * (1.0 - d * 0.4));
                }
            }
        }
    }
    // The cities: a dot and the temperature beside it, where there's room.
    let mut taken = vec![false; W * (H / 2)];
    for r in rs {
        let (x, y) = to_px(r.lon, r.lat);
        cv.set(x as i32, y as i32, rgb(255, 255, 255));
        let label = format!("{:.0}", r.temp);
        let (col, row) = (x as usize + 1, y as usize / 2);
        let cells = col..col + label.len();
        if row >= H / 2 || cells.end > W || cells.clone().any(|c| taken[row * W + c]) {
            continue;
        }
        cells.for_each(|c| taken[row * W + c] = true);
        let bg = scale(temp_color(units, r.temp), 0.45);
        cv.label(col, row, &label, rgb(255, 255, 255), Some(bg));
    }
    // The caller's place, blinking, if it's on the map.
    let (hx, hy) = to_px(here.1, here.0);
    if (0.0..W as f32).contains(&hx) && (TOP as f32..40.0).contains(&hy) && (t * 2.0).fract() < 0.5 {
        cv.rect(hx as i32 - 1, hy as i32 - 1, 3, 3, rgb(255, 255, 255));
        cv.set(hx as i32, hy as i32, rgb(0, 0, 0));
    }
    // The bar over it, the extremes and the scale under it.
    let bar = rgb(16, 30, 90);
    cv.label(0, 0, &" ".repeat(W), bar, Some(bar));
    cv.label(0, 0, " LIVE ", rgb(255, 255, 255), Some(rgb(200, 30, 30)));
    cv.label(7, 0, &format!("national weather · {now}"), rgb(255, 230, 140), Some(bar));
    let deg = units.temp();
    let hot = rs.iter().max_by(|a, b| a.temp.total_cmp(&b.temp));
    let cold = rs.iter().min_by(|a, b| a.temp.total_cmp(&b.temp));
    let wet: Vec<&str> = rs.iter().filter(|r| matches!(r.sky, Sky::Drizzle | Sky::Rain | Sky::Storm | Sky::Snow)).map(|r| r.name).collect();
    let mut line = String::new();
    if let (Some(h), Some(c)) = (hot, cold) {
        line = format!("hottest {} {:.0}{deg} · coldest {} {:.0}{deg}", h.name, h.temp, c.name, c.temp);
    }
    if !wet.is_empty() {
        line += &format!(" · wet: {}", wet.iter().take(3).copied().collect::<Vec<_>>().join(", "));
    }
    let line: String = line.chars().take(W).collect();
    cv.label((W - line.chars().count()) / 2, 20, &line, rgb(235, 240, 255), Some(rgb(8, 12, 30)));
    let stops: &[f32] = if units == Units::Us { &[0.0, 20.0, 32.0, 45.0, 55.0, 65.0, 75.0, 85.0, 95.0, 105.0] } else { &[-18.0, -7.0, 0.0, 7.0, 13.0, 18.0, 24.0, 29.0, 35.0, 41.0] };
    let cell = W / stops.len();
    for (i, v) in stops.iter().enumerate() {
        let c = temp_color(units, *v);
        let fg = if (c[0] + c[1] + c[2]) > 450.0 { rgb(0, 0, 0) } else { rgb(255, 255, 255) };
        let t = format!("{:^width$}", format!("{v:.0}"), width = cell);
        cv.label(i * cell, 21, &t, fg, Some(c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_knows_land_from_sea() {
        for (name, lat, lon) in CITIES {
            assert!(inside(lon as f32, lat as f32), "{name} is on the map");
        }
        assert!(!inside(-127.0, 40.0), "the Pacific");
        assert!(!inside(-60.0, 40.0), "the Atlantic");
        assert!(!inside(-90.0, 26.0), "the Gulf");
        assert!(!inside(-100.0, 52.0), "Canada");
        assert!(!inside(-102.0, 25.0), "Mexico");
        assert!(lake(-87.0, 43.5), "Lake Michigan");
        let (x, y) = to_px(-96.7, 40.8);
        let (lon, lat) = to_geo(x, y);
        assert!((lon + 96.7).abs() < 0.01 && (lat - 40.8).abs() < 0.01);
        assert!((0.0..80.0).contains(&x) && (2.0..40.0).contains(&y), "Lincoln's on screen");
    }

    #[test]
    fn between_cities_its_their_weather() {
        let r = |lat, lon, temp| Reading { name: "x", lat, lon, temp, sky: Sky::Clear };
        let rs = [r(40.0, -100.0, 50.0), r(40.0, -90.0, 70.0)];
        assert_eq!(field(&rs, -100.0, 40.0), 50.0);
        assert!((field(&rs, -95.0, 40.0) - 60.0).abs() < 0.01);
        assert!(field(&rs, -99.0, 40.0) < 55.0);
    }
}
