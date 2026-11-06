//! Pictures as ANSI (new): GIF, PNG and JPEG files in the gallery are
//! drawn in half-blocks (the top pixel as the background of a `▄`, the
//! bottom one as its colour), in 24-bit colour, or the nearest of xterm's
//! 256 for terminals without it. Animated GIFs play with their own timing.
//! The file itself is never changed. Uploads are checked against size,
//! pixel and frame limits first, so a huge GIF can't tie the board up.
//! New: for callers who turn it on, sixel graphics instead, at the
//! terminal's own pixels (`load_sixel`, `sixel`).

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, ImageDecoder, ImageFormat, RgbaImage};

const IMAGES: [&str; 5] = ["gif", "png", "jpg", "jpeg", "webp"];
/// Decoded pixels allowed across all of a GIF's frames, whatever the
/// limits say, so the work stays bounded.
const MAX_TOTAL_PIXELS: u64 = 300_000_000;

pub fn is_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| IMAGES.iter().any(|i| i.eq_ignore_ascii_case(e)))
}

/// `[files]` image limits; 0 is no limit.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_pixels: u64,
    pub max_frames: usize,
}

/// One frame, `w` cells wide and `h` pixels (`h / 2` cells) high; a
/// sixel one is `w` by `h` pixels, its sixel string in `sixel`.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    px: Vec<[u8; 3]>,
    pub delay_ms: u64,
    pub sixel: Vec<u8>,
}

pub struct Picture {
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
}

impl Picture {
    /// "GIF image, 480x270, 32 frames": an upload's description.
    pub fn describe(&self) -> String {
        let frames = if self.frames.len() > 1 { format!(", {} frames", self.frames.len()) } else { String::new() };
        format!("{} image, {}x{}{frames}", self.format, self.width, self.height)
    }
}

/// Fit `w`x`h` pixels into `cols` cells by `rows` cells (two pixels a
/// cell high), keeping the shape.
fn fit(w: u32, h: u32, cols: usize, rows: usize) -> (u32, u32) {
    let scale = (cols as f64 / w as f64).min((rows * 2) as f64 / h as f64);
    let tw = ((w as f64 * scale).round() as u32).clamp(1, cols as u32);
    let th = ((h as f64 * scale).round() as u32).clamp(2, rows as u32 * 2) & !1;
    (tw, th.max(2))
}

/// Fit `w`x`h` pixels into `pw` by `ph`, keeping the shape.
fn fit_pixels(w: u32, h: u32, pw: u32, ph: u32) -> (u32, u32) {
    let scale = (pw as f64 / w as f64).min(ph as f64 / h as f64);
    (((w as f64 * scale).round() as u32).clamp(1, pw), ((h as f64 * scale).round() as u32).clamp(1, ph))
}

/// The picture at `tw`x`th`, transparent pixels over black.
fn scaled(img: &RgbaImage, tw: u32, th: u32) -> Vec<[u8; 3]> {
    let small = image::imageops::resize(img, tw, th, FilterType::Triangle);
    small
        .pixels()
        .map(|p| {
            let a = p[3] as u16;
            [0, 1, 2].map(|i| (p[i] as u16 * a / 255) as u8)
        })
        .collect()
}

fn frame_of(img: &RgbaImage, tw: u32, th: u32, delay_ms: u64) -> Frame {
    Frame { w: tw as usize, h: th as usize, px: scaled(img, tw, th), delay_ms, sixel: Vec::new() }
}

/// Decode a picture into frames that fit `cols` by `rows` cells, or say
/// why not.
pub fn load(data: &[u8], cols: usize, rows: usize, lim: Limits) -> Result<Picture, String> {
    decode(data, lim, |w, h, _| fit(w, h, cols, rows))
}

/// New: as [`load`], in sixels fitting `pw` by `ph` pixels. A GIF is
/// drawn no bigger than it is, and an animation in 64 colors: each frame
/// is a whole picture, and these keep one to about 100 KB.
pub fn load_sixel(data: &[u8], pw: u32, ph: u32, lim: Limits) -> Result<Picture, String> {
    let mut p = decode(data, lim, |w, h, gif| if gif { fit_pixels(w, h, pw.min(w), ph.min(h)) } else { fit_pixels(w, h, pw, ph) })?;
    let colors = if p.frames.len() > 1 { 64 } else { 256 };
    for f in &mut p.frames {
        f.sixel = sixel(&std::mem::take(&mut f.px), f.w, f.h, colors);
    }
    Ok(p)
}

fn decode(data: &[u8], lim: Limits, size: impl Fn(u32, u32, bool) -> (u32, u32)) -> Result<Picture, String> {
    if lim.max_bytes > 0 && data.len() as u64 > lim.max_bytes {
        return Err(format!("larger than {} bytes", lim.max_bytes));
    }
    let format = image::guess_format(data).map_err(|_| "not a picture".to_string())?;
    // MAX_TOTAL_PIXELS holds even with no limit set, and before any
    // decoding: a garbage header can claim a vast canvas.
    let too_big = |w: u32, h: u32| {
        let px = w as u64 * h as u64;
        px > MAX_TOTAL_PIXELS || lim.max_pixels > 0 && px > lim.max_pixels
    };
    if format == ImageFormat::Gif {
        let dec = GifDecoder::new(Cursor::new(data)).map_err(|e| e.to_string())?;
        let (w, h) = dec.dimensions();
        if too_big(w, h) {
            return Err(format!("{w}x{h} is too many pixels"));
        }
        let (tw, th) = size(w, h, true);
        let mut frames = Vec::new();
        let mut all = dec.into_frames();
        loop {
            // Checked before the next frame is decoded.
            if (frames.len() as u64 + 1) * w as u64 * h as u64 > MAX_TOTAL_PIXELS {
                return Err("too much animation".into());
            }
            let Some(f) = all.next() else { break };
            if lim.max_frames > 0 && frames.len() >= lim.max_frames {
                return Err(format!("more than {} frames", lim.max_frames));
            }
            let f = f.map_err(|e| e.to_string())?;
            let (n, d) = f.delay().numer_denom_ms();
            let ms = n.checked_div(d).unwrap_or(0) as u64;
            // Browsers treat the tiny delays some GIFs carry as 100ms.
            let ms = if ms <= 10 { 100 } else { ms };
            frames.push(frame_of(f.buffer(), tw, th, ms));
        }
        if frames.is_empty() {
            return Err("no frames".into());
        }
        return Ok(Picture { format: "GIF", width: w, height: h, frames });
    }
    let reader = image::ImageReader::with_format(Cursor::new(data), format);
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if too_big(w, h) {
        return Err(format!("{w}x{h} is too many pixels"));
    }
    let img = image::load_from_memory_with_format(data, format).map_err(|e| e.to_string())?.to_rgba8();
    let (tw, th) = size(w, h, false);
    let name = match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::WebP => "WebP",
        _ => "Picture",
    };
    Ok(Picture { format: name, width: w, height: h, frames: vec![frame_of(&img, tw, th, 0)] })
}

/// The nearest of xterm's 256 colours (its 6x6x6 cube or grey ramp).
pub(crate) fn xterm256([r, g, b]: [u8; 3]) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let q = |v: u8| LEVELS.iter().enumerate().min_by_key(|(_, l)| (**l as i32 - v as i32).abs()).map_or(0, |(i, _)| i);
    let (qr, qg, qb) = (q(r), q(g), q(b));
    let cube = [LEVELS[qr], LEVELS[qg], LEVELS[qb]];
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let gi = (avg.saturating_sub(3) / 10).min(23);
    let grey = (8 + 10 * gi) as u8;
    let dist = |c: [u8; 3]| -> i32 { (0..3).map(|i| (c[i] as i32 - [r, g, b][i] as i32).pow(2)).sum() };
    if dist([grey; 3]) < dist(cube) { 232 + gi as u8 } else { (16 + 36 * qr + 6 * qg + qb) as u8 }
}

/// The ANSI for a frame at (`left`, `top`), 1-based. With `prev` (the
/// frame on screen), rows that haven't changed are left alone.
pub fn draw(f: &Frame, prev: Option<&Frame>, truecolor: bool, left: u16, top: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let color = |c: [u8; 3], fore: bool| -> String {
        let k = if fore { 38 } else { 48 };
        if truecolor { format!("{k};2;{};{};{}", c[0], c[1], c[2]) } else { format!("{k};5;{}", xterm256(c)) }
    };
    for row in 0..f.h / 2 {
        let span = row * 2 * f.w..(row * 2 + 2) * f.w;
        if prev.is_some_and(|p| p.w == f.w && p.h == f.h && p.px[span.clone()] == f.px[span.clone()]) {
            continue;
        }
        out.extend(format!("\x1b[{};{}H", top + row as u16, left).as_bytes());
        let (mut fg, mut bg) = (None, None);
        for x in 0..f.w {
            let (up, down) = (f.px[row * 2 * f.w + x], f.px[(row * 2 + 1) * f.w + x]);
            let mut sgr = Vec::new();
            if bg != Some(up) {
                sgr.push(color(up, false));
                bg = Some(up);
            }
            if fg != Some(down) {
                sgr.push(color(down, true));
                fg = Some(down);
            }
            if !sgr.is_empty() {
                out.extend(format!("\x1b[{}m", sgr.join(";")).as_bytes());
            }
            out.push(0xDC);
        }
        out.extend(b"\x1b[0m");
    }
    out
}

/// New: `px` (`w` by `h`) as a sixel string: at most `colors` colors
/// (NeuQuant's, up to 256), pixels 1:1 (CTerm's default is 2:1), and sixels left
/// unset keep what's under them.
pub fn sixel(px: &[[u8; 3]], w: usize, h: usize, colors: usize) -> Vec<u8> {
    let rgba: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    // The sampling (1 is every pixel) is enough at a screen's size.
    let nq = color_quant::NeuQuant::new(10, colors, &rgba);
    let map = nq.color_map_rgb();
    let idx: Vec<u8> = rgba.as_chunks::<4>().0.iter().map(|p| nq.index_of(p) as u8).collect();
    let mut out = format!("\x1bP9;1q\"1;1;{w};{h}").into_bytes();
    let pct = |v: u8| (v as u32 * 100 + 127) / 255;
    let mut used = [false; 256];
    idx.iter().for_each(|&i| used[i as usize] = true);
    for (i, c) in map.as_chunks::<3>().0.iter().enumerate().filter(|(i, _)| used[*i]) {
        out.extend(format!("#{i};2;{};{};{}", pct(c[0]), pct(c[1]), pct(c[2])).as_bytes());
    }
    let mut bands: Vec<Option<Vec<u8>>> = vec![None; 256];
    for top in (0..h).step_by(6) {
        for (dy, y) in (top..(top + 6).min(h)).enumerate() {
            for x in 0..w {
                let i = idx[y * w + x] as usize;
                bands[i].get_or_insert_with(|| vec![0; w])[x] |= 1 << dy;
            }
        }
        let mut first = true;
        for (i, b) in bands.iter_mut().enumerate() {
            let Some(bits) = b.take() else { continue };
            if !first {
                out.push(b'$');
            }
            first = false;
            out.extend(format!("#{i}").as_bytes());
            // Nothing past the color's last sixel in the band.
            let end = bits.iter().rposition(|&v| v != 0).map_or(0, |p| p + 1);
            let mut x = 0;
            while x < end {
                let run = bits[x..end].iter().take_while(|&&v| v == bits[x]).count();
                let ch = 0x3F + bits[x];
                if run > 3 {
                    out.extend(format!("!{run}").as_bytes());
                    out.push(ch);
                } else {
                    out.extend(std::iter::repeat_n(ch, run));
                }
                x += run;
            }
        }
        out.push(b'-');
    }
    out.extend(b"\x1b\\");
    out
}

/// Pictures decoded lately, by where they came from and the size asked.
type Key = (PathBuf, String, u64, usize, usize);
static CACHE: Mutex<Vec<(Key, Arc<Picture>)>> = Mutex::new(Vec::new());
const CACHED: usize = 8;

pub fn cached(key: &Key) -> Option<Arc<Picture>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == key).map(|(_, p)| p.clone())
}

pub fn remember(key: Key, p: Arc<Picture>) {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|(k, _)| *k != key);
    if c.len() >= CACHED {
        c.remove(0);
    }
    c.push((key, p));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(frames: usize, w: u16, h: u16) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
            for i in 0..frames {
                let img = RgbaImage::from_pixel(w as u32, h as u32, image::Rgba([i as u8 * 40, 0, 255, 255]));
                let f = image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(50, 1));
                enc.encode_frame(f).unwrap();
            }
        }
        out
    }

    const ANY: Limits = Limits { max_bytes: 0, max_pixels: 0, max_frames: 0 };

    #[test]
    fn animated_gifs_fit_the_screen() {
        let p = load(&gif(3, 160, 90), 80, 24, ANY).unwrap();
        assert_eq!(p.frames.len(), 3);
        assert_eq!((p.frames[0].w, p.frames[0].h), (80, 44), "shape kept, rows even");
        assert_eq!(p.frames[0].delay_ms, 50);
        assert_eq!(p.describe(), "GIF image, 160x90, 3 frames");
        let tall = load(&gif(1, 100, 400), 80, 24, ANY).unwrap();
        assert_eq!((tall.frames[0].w, tall.frames[0].h), (12, 48));
    }

    #[test]
    fn webp_is_a_picture_whatever_its_name() {
        let img = RgbaImage::from_pixel(160, 90, image::Rgba([255, 0, 0, 255]));
        let mut data = Vec::new();
        img.write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut data)).unwrap();
        let p = load(&data, 80, 24, ANY).unwrap();
        assert_eq!((p.format, p.width, p.height), ("WebP", 160, 90));
        assert!(is_image("PHOTO.WEBP"));
    }

    #[test]
    fn limits_refuse_big_gifs() {
        let lim = |b, p, f| Limits { max_bytes: b, max_pixels: p, max_frames: f };
        let err = |r: Result<Picture, String>| r.err().unwrap_or_default();
        assert!(err(load(&gif(5, 10, 10), 80, 24, lim(0, 0, 4))).contains("frames"));
        assert!(err(load(&gif(1, 100, 100), 80, 24, lim(0, 9_999, 0))).contains("pixels"));
        assert!(err(load(&gif(1, 10, 10), 80, 24, lim(10, 0, 0))).contains("bytes"));
        assert!(load(b"GIF89a but not really", 80, 24, ANY).is_err());
    }

    #[test]
    fn colours_by_terminal() {
        assert_eq!(xterm256([255, 0, 0]), 196);
        assert_eq!(xterm256([128, 128, 128]), 244);
        let f = Frame { w: 1, h: 2, px: vec![[1, 2, 3], [4, 5, 6]], delay_ms: 0, sixel: Vec::new() };
        assert_eq!(draw(&f, None, true, 5, 2), b"\x1b[2;5H\x1b[48;2;1;2;3;38;2;4;5;6m\xdc\x1b[0m");
        assert!(draw(&f, Some(&f), true, 5, 2).is_empty(), "unchanged rows are skipped");
        assert!(String::from_utf8_lossy(&draw(&f, None, false, 1, 1)).contains("48;5;16"));
    }

    #[test]
    fn sixels_at_the_terminals_pixels() {
        // Red over blue, 7 rows: two bands, the second one row high.
        let mut px = vec![[255, 0, 0]; 4 * 3];
        px.extend(vec![[0, 0, 255]; 4 * 4]);
        let s = String::from_utf8(sixel(&px, 4, 7, 256)).unwrap();
        assert!(s.starts_with("\x1bP9;1q\"1;1;4;7#"), "{s}");
        assert!(s.ends_with("-\x1b\\"));
        assert_eq!(s.matches('-').count(), 2, "one newline per band");
        assert!(s.contains("!4"), "runs are counted: {s}");
        assert!(s.contains(";2;100;0;0") && s.contains(";2;0;0;9"), "colors near enough: {s}");
        let p = load_sixel(&gif(2, 160, 90), 640, 384, ANY).unwrap();
        assert_eq!((p.frames[0].w, p.frames[0].h), (160, 90), "a GIF isn't enlarged");
        assert!(p.frames[1].sixel.starts_with(b"\x1bP") && p.frames[1].px.is_empty());
        let big = load_sixel(&gif(1, 1600, 900), 640, 384, ANY).unwrap();
        assert_eq!((big.frames[0].w, big.frames[0].h), (640, 360));
        let img = RgbaImage::from_pixel(160, 90, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let still = load_sixel(&png, 640, 384, ANY).unwrap();
        assert_eq!((still.frames[0].w, still.frames[0].h), (640, 360), "a still fills the screen");
    }
}
