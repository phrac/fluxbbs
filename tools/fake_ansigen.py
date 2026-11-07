#!/usr/bin/env python3
"""A fake ANSI art gateway, for testing flux/bbs's `AN` without the GPU.

    python3 tools/fake_ansigen.py [PORT] [MODE] [SECONDS]   (127.0.0.1, default 8197 ready 3)

It answers as ansilora's scripts/gateway.py does: `GET /status` and
`POST /generate`, with `Authorization: Bearer test`. MODE is what it says
and does:

    ready      the art model is loaded; a gen takes SECONDS
    warming    it's loading: status says warming, a gen takes SECONDS more
    busy       another model is loaded: status says busy, generate 503
    fail       status ready, generate 500 (the model server broke)

It also answers `POST /v1/chat/completions` as an AI would the board's
prompt check (`[ai] base_url = "http://127.0.0.1:PORT/v1"`): YES if the
prompt has the word "forbidden" in it, else NO.

The art is a color test card with the prompt's first letters in it (16,
256 and 24-bit versions, 78x24, SAUCE), and the PNG a gradient.
"""
import base64
import json
import struct
import sys
import time
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8197
MODE = sys.argv[2] if len(sys.argv) > 2 else "ready"
SECONDS = float(sys.argv[3]) if len(sys.argv) > 3 else 3.0
seed = 1000


def png(w=160, h=96):
    rows = b"".join(b"\0" + bytes(v for x in range(w) for v in (x * 255 // w, y * 255 // h, 128)) for y in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def sauce(size, title):
    f = lambda s, n: s.encode("cp437", "replace")[:n].ljust(n, b" ")
    return (b"SAUCE00" + f(title, 35) + f("fake", 20) + f("fake", 20) + b"20261107" + size.to_bytes(4, "little")
            + bytes([1, 1]) + (80).to_bytes(2, "little") + (24).to_bytes(2, "little") + bytes(4) + bytes([0, 3]) + b"IBM VGA".ljust(22, b"\0"))


def art(depth, prompt):
    word = "".join(c for c in prompt.upper() if c.isalnum())[:20] or "FAKE"
    out = bytearray(b"\x1b[0m")
    for r in range(24):
        for c in range(78):
            if depth == "16":
                out += b"\x1b[%d;%dm" % (30 + (c // 10) % 8, 40 + r % 8)
            elif depth == "256":
                out += b"\x1b[38;5;%d;48;5;%dm" % (16 + (r * 78 + c) % 216, 232 + r % 24)
            else:
                out += b"\x1b[38;2;%d;%d;%d;48;2;%d;0;%dm" % (c * 3, r * 10, 200, r * 10, 255 - c * 3)
            ch = word[c - 29] if r == 12 and 29 <= c < 29 + len(word) else "\xb1\xdb\xdc\xdf"[(r + c) % 4]
            out += ch.encode("latin-1")
        out += b"\x1b[0m\r\n"
    body = bytes(out)
    return body + b"\x1a" + sauce(len(body), prompt[:35])


class H(BaseHTTPRequestHandler):
    def reply(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def ok(self):
        if self.headers.get("Authorization") != "Bearer test":
            self.reply(401, {"ok": False, "error": "unauthorized"})
            return False
        return True

    def do_GET(self):
        if self.ok():
            state = {"ready": "ready", "fail": "ready", "warming": "warming", "busy": "busy"}.get(MODE, MODE)
            self.reply(200, {"ok": True, "state": state, "why": "", "job": False, "waiting": 0})

    def do_POST(self):
        global seed
        req = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        if self.path == "/v1/chat/completions":
            asked = req["messages"][-1]["content"]
            print("AI asked:", asked, flush=True)
            answer = "YES" if "forbidden" in asked.lower() else "NO"
            return self.reply(200, {"choices": [{"message": {"content": answer}, "finish_reason": "stop"}]})
        if not self.ok():
            return
        prompt = req.get("prompt", "")
        if MODE == "busy":
            return self.reply(503, {"ok": False, "error": "unavailable", "state": "busy"})
        time.sleep(SECONDS * (2 if MODE == "warming" else 1))
        if MODE == "fail":
            return self.reply(500, {"ok": False, "error": "RuntimeError: the model server made no image"})
        seed += 1
        b = lambda d: base64.b64encode(d).decode()
        self.reply(200, {"ok": True, "seed": seed, "seconds": SECONDS, "prompt": "ansi16 style, " + prompt + ", no text, no letters",
                         "png": b(png()), "ans16": b(art("16", prompt)), "ans256": b(art("256", prompt)), "ans24": b(art("24", prompt))})


print(f"fake ansigen gateway on 127.0.0.1:{PORT}, {MODE}, {SECONDS}s a gen", flush=True)
ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
