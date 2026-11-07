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
/// The biggest one picture (or GIF canvas) may be, whatever the limits
/// say: 8192x8192, 256 MB decoded.
const MAX_PICTURE_PIXELS: u64 = 1 << 26;

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

const SIXEL_SIDE: u32 = 2048;
const SIXEL_ZOOM: u32 = 4;

/// New: as [`load`], in sixels fitting `pw` by `ph` pixels. A GIF is
/// drawn no bigger than it is, and an animation in 64 colors: each frame
/// is a whole picture, and these keep one to about 100 KB.
pub fn load_sixel(data: &[u8], pw: u32, ph: u32, lim: Limits) -> Result<Picture, String> {
    // At most SIXEL_SIDE a side and SIXEL_ZOOM times the picture: the
    // cell size and rows come from the caller's terminal.
    let (pw, ph) = (pw.clamp(1, SIXEL_SIDE), ph.clamp(1, SIXEL_SIDE));
    let mut p = decode(data, lim, |w, h, gif| {
        let zoom = if gif { 1 } else { SIXEL_ZOOM };
        fit_pixels(w, h, pw.min(w.saturating_mul(zoom)), ph.min(h.saturating_mul(zoom)))
    })?;
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
    // MAX_PICTURE_PIXELS holds even with no limit set, and before any
    // decoding: a garbage header can claim a vast canvas.
    // Nor an empty one, which has nothing to scale (a header can say 0).
    let check = |w: u32, h: u32| {
        let px = w as u64 * h as u64;
        if px == 0 {
            Err(format!("{w}x{h} has no pixels"))
        } else if px > MAX_PICTURE_PIXELS || lim.max_pixels > 0 && px > lim.max_pixels {
            Err(format!("{w}x{h} is too many pixels"))
        } else {
            Ok(())
        }
    };
    if format == ImageFormat::Gif {
        let mut dec = GifDecoder::new(Cursor::new(data)).map_err(|e| e.to_string())?;
        let (w, h) = dec.dimensions();
        check(w, h)?;
        // Each frame says its own size, which can be far past the
        // canvas's: decoding one of 65535x65535 takes gigabytes.
        let mut all_px = 0u64;
        for (fw, fh) in gif_frames(data).into_iter().filter(|&(fw, fh)| fw > 0 && fh > 0) {
            check(fw, fh)?;
            all_px += fw as u64 * fh as u64;
            if all_px > MAX_TOTAL_PIXELS {
                return Err("too much animation".into());
            }
        }
        // A frame is no bigger than the canvas: one that claims to be (a
        // few bytes can claim gigabytes) isn't decoded. Room for the
        // canvas, the frame and a little over.
        let mut limits = image::Limits::default();
        limits.max_alloc = Some((w as u64 * h as u64 * 4 * 2).max(1 << 20));
        dec.set_limits(limits).map_err(|e| e.to_string())?;
        let (tw, th) = size(w, h, true);
        // Each frame costs its pixels decoded or drawn, whichever's more.
        let per_frame = (w as u64 * h as u64).max(tw as u64 * th as u64);
        let mut frames = Vec::new();
        let mut all = dec.into_frames();
        loop {
            // Checked before the next frame is decoded.
            if (frames.len() as u64 + 1) * per_frame > MAX_TOTAL_PIXELS {
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
    check(w, h)?;
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

/// New: the sizes a GIF's frames say they are, from their image
/// descriptors, without decoding them (as far as the blocks can be read).
fn gif_frames(d: &[u8]) -> Vec<(u32, u32)> {
    let u16_at = |i: usize| d.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32);
    // A colour table after this flag byte: 3 << (n + 1) bytes.
    let table = |flags: u8| if flags & 0x80 != 0 { 3usize << ((flags & 7) + 1) } else { 0 };
    // Sub-blocks from `i`, to just past their terminator.
    let skip = |mut i: usize| {
        while let Some(&n) = d.get(i) {
            i += 1 + n as usize;
            if n == 0 {
                break;
            }
        }
        i
    };
    let mut out = Vec::new();
    let mut i = 13 + d.get(10).map_or(0, |&f| table(f));
    while let Some(&b) = d.get(i) {
        match b {
            0x21 => i = skip(i + 2),
            0x2C => {
                out.push((u16_at(i + 5), u16_at(i + 7)));
                i = skip(i + 10 + d.get(i + 9).map_or(0, |&f| table(f)) + 1);
            }
            _ => break,
        }
    }
    out
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

    pub(super) fn gif(frames: usize, w: u16, h: u16) -> Vec<u8> {
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
    fn sixel_targets_are_capped() {
        let mut png = Vec::new();
        RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255])).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        // Cells of 64x128 and 200 rows: 5120x25600 asked for.
        let p = load_sixel(&png, 80 * 64, 200 * 128, ANY).unwrap();
        assert_eq!((p.frames[0].w, p.frames[0].h), (40, 40), "four times the picture at most");
    }

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

    /// Found by fuzzing: a GIF whose header says it's 0 pixels wide
    /// panicked fitting it in sixels.
    #[test]
    fn empty_pictures_are_refused() {
        let mut g = gif(1, 4, 4);
        g[6..10].copy_from_slice(&[0, 0, 4, 0]);
        assert!(load_sixel(&g, 640, 384, ANY).is_err());
        assert!(load(&g, 80, 24, ANY).is_err());
    }

    /// Found by fuzzing: a 79-byte GIF whose frame says it's 65289x65535
    /// took seconds (and gigabytes) before failing to decode.
    #[test]
    fn huge_frames_are_refused_first() {
        let g = b"GIF89a\x09\x00\x05\x00\x80\x00\x00\x00\x00\x00\x00\x00\x00!\xf9\x04\x08\x00\x00\x00\x00,\x00\x00\x00\x00\x09\xff\xff\xff\x00\x05\x00\x02\x00\x00\xdf\x00\x00\x00\x80\x05\x02\x05\x84\x8f\xa9\xcb]\x00;";
        assert_eq!(gif_frames(g), [(65289, 65535)]);
        let t = std::time::Instant::now();
        assert!(load(g, 80, 24, ANY).err().unwrap_or_default().contains("too many pixels"));
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
        // Ordinary frames are read as they are.
        assert_eq!(gif_frames(&gif(3, 16, 9)), [(16, 9); 3]);
    }

    /// Found by fuzzing: a GIF with no width panicked in the sixel
    /// fitting; one whose frame claims far more than its canvas (31 bytes
    /// asking for 1.5 GB) was decoded all the same.
    #[test]
    fn empty_and_oversized_frames_are_refused() {
        let lim = crate::config::Files::default().image_limits();
        let empty = b"GIF89a\x14\x00\x00\x00\x00,\x00\x00\x00\x00";
        assert!(load_sixel(empty, 640, 384, lim).is_err() && load(empty, 80, 24, ANY).is_err());
        let huge = b"GIF89a\x14\x00\x0a\x00\x80\x00\x00\x00\x00\x00\x00\x00\x00,\x00\x00\x00\x00\x89PNG\x0d\x0a\x0a";
        let start = std::time::Instant::now();
        assert!(load(huge, 80, 24, lim).is_err() && load(huge, 80, 24, ANY).is_err());
        let wide = b"GIF89a\xff\xff\x01\x00\x80\x00\x00\x00\x00\x00\x00\x00\x00,>l\xe3\x94\xde\xa3\xc4\xd5\x02\x02\x00";
        assert!(load(wide, 80, 2, ANY).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
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

#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::fuzzkit::{Rng, Target};
    use crate::fuzzcorpus;

    /// The board's default `[files]` limits.
    const LIM: Limits = Limits { max_bytes: 10 << 20, max_pixels: 2048 * 2048, max_frames: 300 };

    fn seeds() -> Vec<Vec<u8>> {
        let img = RgbaImage::from_fn(13, 7, |x, y| image::Rgba([x as u8 * 19, y as u8 * 36, 128, if x == 3 { 0 } else { 255 }]));
        let mut out = Vec::new();
        for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP, ImageFormat::Gif] {
            let mut b = Vec::new();
            let ok = if f == ImageFormat::Jpeg { image::DynamicImage::ImageRgba8(img.clone()).to_rgb8().write_to(&mut Cursor::new(&mut b), f) } else { img.write_to(&mut Cursor::new(&mut b), f) };
            if ok.is_ok() {
                out.push(b);
            }
        }
        let mut gif = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut gif);
            for i in 0..3u8 {
                let f = image::Frame::from_parts(RgbaImage::from_pixel(9, 5, image::Rgba([i * 80, 0, 255, 255])), 0, 0, image::Delay::from_numer_denom_ms(5, 1));
                enc.encode_frame(f).unwrap();
            }
        }
        out.push(gif);
        out
    }

    /// Picture files as uploads give them: decoded in cells and in
    /// sixels, each frame drawn.
    #[test]
    #[ignore]
    fn fuzz_picture_load() {
        // Up to 300 frames, each scaled and quantized: bounded, not quick.
        let mut t = Target::new("picture_load", &[]).max_len(64 * 1024).slow_ms(3000);
        t.corpus = seeds();
        t.run(|input, rng| {
            let (cols, rows) = (1 + rng.below(200), 1 + rng.below(100));
            if let Ok(p) = load(input, cols, rows, LIM) {
                let mut prev = None;
                for f in &p.frames {
                    assert!(f.w <= cols && f.h <= rows * 2 && f.px.len() == f.w * f.h);
                    let _ = draw(f, prev, rng.one_in(2), 1, 1);
                    prev = Some(f);
                }
            }
            let (pw, ph) = (rng.number() as u32, rng.number() as u32);
            if let Ok(p) = load_sixel(input, pw, ph, LIM) {
                for f in &p.frames {
                    assert!(f.w as u32 <= SIXEL_SIDE && f.h as u32 <= SIXEL_SIDE && f.sixel.ends_with(b"\x1b\\"));
                }
            }
        });
    }


    const DICT: &[&[u8]] = &[b"GIF89a", b"\x21\xf9\x04", b"\x2c", b"\x3b", b"NETSCAPE2.0", b"\x89PNG\r\n\x1a\n", b"IHDR", b"IDAT", b"IEND", b"acTL", b"\xff\xd8\xff", b"\xff\xc0", b"\xff\xda", b"RIFF", b"WEBPVP8L", b"VP8X", b"ANIM", b"ANMF"];

    fn pictures() -> Vec<Vec<u8>> {
        let img = RgbaImage::from_fn(32, 18, |x, y| image::Rgba([(x * 8) as u8, (y * 14) as u8, 128, if x == 0 { 0 } else { 255 }]));
        let mut out = vec![tests::gif(3, 20, 10), tests::gif(1, 1, 1)];
        for f in [ImageFormat::Png, ImageFormat::Jpeg] {
            let mut d = Vec::new();
            let pic = if f == ImageFormat::Jpeg { image::DynamicImage::ImageRgba8(img.clone()).to_rgb8().into() } else { image::DynamicImage::ImageRgba8(img.clone()) };
            pic.write_to(&mut Cursor::new(&mut d), f).unwrap();
            out.push(d);
        }
        let mut webp = Vec::new();
        img.write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut webp)).unwrap();
        out.push(webp);
        out
    }

    fn check(p: &Picture, cols: usize, rows: usize) {
        for f in &p.frames {
            assert!(f.w >= 1 && f.w <= cols && f.h >= 2 && f.h <= rows * 2, "{}x{}", f.w, f.h);
            let _ = draw(f, None, true, 1, 1);
            let _ = draw(f, Some(f), false, 1, 1);
        }
        let _ = p.describe();
    }

    /// Pictures as the board's limits take them (its defaults).
    #[test]
    #[ignore]
    fn fuzz_picture() {
        let lim = crate::config::Files::default().image_limits();
        fuzzcorpus::run("picture", pictures(), DICT, 20_000, |d| {
            if let Ok(p) = load(d, 80, 23, lim) {
                check(&p, 80, 23);
            }
            if d.len() % 4 == 0
                && let Ok(p) = load_sixel(d, 640, 384, lim)
            {
                assert!(p.frames.iter().all(|f| f.w <= 640 && f.h <= 384 && !f.sixel.is_empty()));
            }
        });
    }

    fn pixels(rng: &mut Rng, input: &[u8], n: usize) -> Vec<[u8; 3]> {
        (0..n).map(|i| match rng.below(4) {
            0 => [0, 0, 0],
            1 => [255, 255, 255],
            _ => [input.get(i * 3).copied().unwrap_or(0), input.get(i * 3 + 1).copied().unwrap_or(0), input.get(i * 3 + 2).copied().unwrap_or(0)],
        }).collect()
    }

    /// Sixels and half-blocks from any pixels at any size.
    #[test]
    #[ignore]
    fn fuzz_picture_encode() {
        Target::new("picture_encode", &[b"\x00\x00\x00\xff\xff\xff\x80\x40\x20"]).max_len(4096).run(|input, rng| {
            let (wide, tall) = (if rng.one_in(8) { 600 } else { 40 }, if rng.one_in(8) { 300 } else { 20 });
            let (w, h) = (1 + rng.below(wide), 1 + rng.below(tall));
            let px = pixels(rng, input, w * h);
            let colors = *rng.pick(&[1, 2, 16, 64, 255, 256]);
            let s = sixel(&px, w, h, colors);
            assert!(s.starts_with(b"\x1bP") && s.ends_with(b"\x1b\\"));
            assert!(s.len() <= 64 + w * h * 8 + h.div_ceil(6) * 256 * 8 + 256 * 24, "{} bytes for {w}x{h}", s.len());
            let f = Frame { w, h: h & !1, px: px[..w * (h & !1)].to_vec(), delay_ms: 0, sixel: Vec::new() };
            let g = Frame { w, h: h & !1, px: pixels(rng, input, w * (h & !1)), delay_ms: 0, sixel: Vec::new() };
            let _ = draw(&f, Some(&g), rng.one_in(2), 1 + rng.below(80) as u16, 1 + rng.below(200) as u16);
            let _ = xterm256(px[0]);
        });
    }

    /// With no limits set (0): only the board's own bounds hold.
    #[test]
    #[ignore]
    fn fuzz_picture_unlimited() {
        let lim = Limits { max_bytes: 0, max_pixels: 0, max_frames: 0 };
        // As big as MAX_PICTURE_PIXELS, and as slow to scale.
        fuzzcorpus::run_panics_only("picture_unlimited", pictures(), DICT, 5_000, |d| {
            if let Ok(p) = load(d, 80, 2, lim) {
                check(&p, 80, 2);
            }
        });
    }
}
