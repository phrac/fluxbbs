"""A pretend MeshCore companion radio, for trying the bridge without one.

    python3 tools/fake_meshcore.py [--port 5000] [--name disflux]
        [--channel Public --channel disflux] [--say "1 Bob: hi"]...

Speaks the companion protocol over TCP like WiFi companion firmware. Each
line typed (or given with --say, sent a few seconds after the board
connects) is "SLOT SENDER: TEXT", heard on that channel. What the board
sends on a channel is printed as "SENT SLOT TEXT", with the radio's
"NAME: " in front as a real radio would put it.
"""
import argparse, socket, struct, sys, threading, time

ap = argparse.ArgumentParser()
ap.add_argument('--host', default='127.0.0.1')
ap.add_argument('--port', type=int, default=5000)
ap.add_argument('--name', default='disflux')
ap.add_argument('--channel', action='append', help='channel names, slot 0 first (default Public)')
ap.add_argument('--say', action='append', default=[], help='"SLOT SENDER: TEXT" to hear after connecting')
args = ap.parse_args()
channels = args.channel or ['Public']

lock = threading.RLock()
queue, conn = [], None


def frame(payload):
    return b'>' + struct.pack('<H', len(payload)) + payload


def push(payload):
    with lock:
        if conn:
            conn.sendall(frame(payload))


def hear(line):
    """Queue a channel message (V3 layout) and tickle the app."""
    slot, _, text = line.strip().partition(' ')
    if not slot.isdigit() or not text:
        print('?? want "SLOT SENDER: TEXT"', flush=True)
        return
    msg = bytes([17, 40, 0, 0, int(slot), 0xFF, 0]) + struct.pack('<I', int(time.time())) + text.encode()
    with lock:
        queue.append(msg)
    push(bytes([0x83]))


def reply(cmd):
    c = cmd[0]
    if c == 22:  # CMD_DEVICE_QUERY
        return bytes([13, 14, 50, len(channels)]) + bytes(80)
    if c == 1:  # CMD_APP_START: SELF_INFO, the name after 58 bytes
        return bytes([5]) + bytes(57) + args.name.encode()
    if c == 31:  # CMD_GET_CHANNEL
        if cmd[1] < len(channels):
            return bytes([18, cmd[1]]) + channels[cmd[1]].encode().ljust(32, b'\0') + bytes(16)
        return bytes([1, 2])  # ERR, not found
    if c == 10:  # CMD_SYNC_NEXT_MESSAGE
        with lock:
            return queue.pop(0) if queue else bytes([10])
    if c == 3:  # CMD_SEND_CHANNEL_TXT_MSG: type, slot, time, text
        print(f'SENT {cmd[2]} {args.name}: {cmd[7:].decode("utf-8", "replace")}', flush=True)
        return bytes([0])
    return bytes([0])  # OK to everything else (the time, ...)


def serve(c):
    global conn
    with lock:
        conn = c
    print('CONNECTED', flush=True)
    for s in args.say:
        threading.Timer(3, hear, [s]).start()
    buf = b''
    while True:
        data = c.recv(4096)
        if not data:
            break
        buf += data
        while len(buf) >= 3 and buf[0:1] == b'<':
            n = struct.unpack('<H', buf[1:3])[0]
            if len(buf) < 3 + n:
                break
            cmd, buf = buf[3:3 + n], buf[3 + n:]
            with lock:
                c.sendall(frame(reply(cmd)))
        if buf[:1] not in (b'', b'<'):
            buf = buf[1:]
    with lock:
        conn = None
    print('DISCONNECTED', flush=True)


def stdin():
    for line in sys.stdin:
        if line.strip():
            hear(line)


threading.Thread(target=stdin, daemon=True).start()
srv = socket.create_server((args.host, args.port))
print(f'radio {args.name!r} on {args.host}:{args.port}, channels {channels}', flush=True)
while True:
    c, _ = srv.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
