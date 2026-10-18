"""Render a CP437 ANSI file (ANSI.SYS semantics, 80 columns) to a PNG."""
import re, sys
from PIL import Image, ImageDraw, ImageFont

PAL = [(0,0,0),(0,0,170),(0,170,0),(0,170,170),(170,0,0),(170,0,170),(170,85,0),(170,170,170),
       (85,85,85),(85,85,255),(85,255,85),(85,255,255),(255,85,85),(255,85,255),(255,255,85),(255,255,255)]
ANSI2DOS = [0,4,2,6,1,5,3,7]
W, H = 8, 16
# Any monospace TTF; block characters are drawn directly.
FONT = ImageFont.truetype(__import__('os').environ.get('ANSI_FONT', '/usr/share/fonts/TTF/JetBrainsMonoNerdFontMono-Regular.ttf'), 13)

def xterm256(n):
    if n < 16: return PAL[ANSI2DOS[n & 7] | (n & 8)]
    if n >= 232: v = 8 + 10 * (n - 232); return (v, v, v)
    n -= 16
    return tuple(0 if c == 0 else 55 + 40 * c for c in (n // 36, n // 6 % 6, n % 6))

def rgb_of(c):
    return c if isinstance(c, tuple) else PAL[c]

def render(data, rows_max=60):
    data = data.split(b'\x1a')[0]
    grid = {}
    x = y = 0; fg, bg, bold, blink = 7, 0, False, False; saved = (0, 0)
    i = 0
    while i < len(data):
        b = data[i]
        if b == 0x1b and i + 1 < len(data) and data[i+1] == ord('['):
            m = re.match(rb'\x1b\[([0-9;?]*)([A-Za-z])', data[i:])
            if not m: i += 1; continue
            ps = [int(p) if p.isdigit() else 0 for p in m.group(1).split(b';')] if m.group(1) else []
            f = m.group(2)
            n = (ps[0] if ps and ps[0] else 1)
            if f == b'm':
                ps = ps or [0]
                k = 0
                while k < len(ps):
                    p = ps[k]; k += 1
                    if p in (38, 48) and k < len(ps):
                        # xterm 256-colour / 24-bit: keep the RGB value.
                        if ps[k] == 5 and k + 1 < len(ps): rgb = xterm256(ps[k+1]); k += 2
                        elif ps[k] == 2 and k + 3 < len(ps): rgb = tuple(ps[k+1:k+4]); k += 4
                        else: continue
                        if p == 38: fg = rgb
                        else: bg = rgb
                    elif p == 0: fg, bg, bold, blink = 7, 0, False, False
                    elif p == 1: bold = True
                    elif p == 5: blink = True
                    elif 30 <= p <= 37: fg = ANSI2DOS[p-30]
                    elif 40 <= p <= 47: bg = ANSI2DOS[p-40]
                    elif 90 <= p <= 97: fg = ANSI2DOS[p-90] | 8
                    elif 100 <= p <= 107: bg = ANSI2DOS[p-100] | 8
                    elif p == 22: bold = False
            elif f in b'Hf':
                y = (ps[0] if ps and ps[0] else 1) - 1; x = (ps[1] if len(ps) > 1 and ps[1] else 1) - 1
            elif f == b'A': y = max(0, y - n)
            elif f == b'B': y += n
            elif f == b'C': x = min(79, x + n)
            elif f == b'D': x = max(0, x - n)
            elif f == b'J': grid.clear(); x = y = 0
            elif f == b'K':
                for cx in range(x, 80): grid.pop((cx, y), None)
            elif f == b's': saved = (x, y)
            elif f == b'u': x, y = saved
            i += m.end(); continue
        if b == 8: x = max(0, x - 1)
        elif b == 13: x = 0
        elif b == 10: y += 1
        elif b == 9: x = min(79, (x // 8 + 1) * 8)
        else:
            grid[(x, y)] = (b, fg if isinstance(fg, tuple) else (fg | (8 if bold else 0)), bg)
            x += 1
            if x >= 80: x = 0; y += 1
        i += 1
    rows = min(rows_max, max([p[1] for p in grid] + [0]) + 1)
    img = Image.new('RGB', (80 * W, rows * H), (0, 0, 0))
    d = ImageDraw.Draw(img)
    for (cx, cy), (ch, f, bgc) in grid.items():
        if cy >= rows: continue
        x0, y0 = cx * W, cy * H
        d.rectangle([x0, y0, x0 + W - 1, y0 + H - 1], fill=rgb_of(bgc))
        col = rgb_of(f)
        if ch == 0xDB: d.rectangle([x0, y0, x0+W-1, y0+H-1], fill=col)
        elif ch == 0xDF: d.rectangle([x0, y0, x0+W-1, y0+H//2-1], fill=col)
        elif ch == 0xDC: d.rectangle([x0, y0+H//2, x0+W-1, y0+H-1], fill=col)
        elif ch == 0xDD: d.rectangle([x0, y0, x0+W//2-1, y0+H-1], fill=col)
        elif ch == 0xDE: d.rectangle([x0+W//2, y0, x0+W-1, y0+H-1], fill=col)
        elif ch == 0xFE: d.rectangle([x0+2, y0+5, x0+5, y0+10], fill=col)
        elif ch in (0xB0, 0xB1, 0xB2):
            dens = {0xB0: 0.25, 0xB1: 0.5, 0xB2: 0.75}[ch]
            for py in range(H):
                for px in range(W):
                    on = {0xB0: (px % 4 == 0 and py % 2 == 0) or (px % 4 == 2 and py % 2 == 1),
                          0xB1: (px + py) % 2 == 0,
                          0xB2: not ((px % 4 == 0 and py % 2 == 0) or (px % 4 == 2 and py % 2 == 1))}[ch]
                    if on: d.point((x0+px, y0+py), fill=col)
        elif ch != 32 and ch != 0:
            g = bytes([ch]).decode('cp437')
            if 0 < ch < 32: g = '☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼'[ch-1]
            d.text((x0, y0 + 1), g, font=FONT, fill=col)
    img = img.resize((img.width * 2, img.height * 2), Image.NEAREST)
    return img

if __name__ == '__main__':
    render(open(sys.argv[1], 'rb').read()).save(sys.argv[2])
