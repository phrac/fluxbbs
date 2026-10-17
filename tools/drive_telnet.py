"""Drive a telnet session like a BBS terminal: tdrive.py HOST:PORT 'expect=>send' ...
Answers negotiation as SyncTERM would (TTYPE syncterm, NAWS 80x25, binary).
'!cmd' runs cmd with its stdin/stdout bridged to the connection (IAC-escaped)."""
import os, re, select, socket, subprocess, sys, time

IAC, DONT, DO, WONT, WILL, SB, SE = 255, 254, 253, 252, 251, 250, 240
host, port = sys.argv[1].split(':')
steps = sys.argv[2:]
s = socket.create_connection((host, int(port)))
raw = b''
pending = b''
state = 'data'
sub = b''
log = []

def send(b):
    s.sendall(b)

def handle_cmd(verb, opt):
    log.append((verb, opt))
    if verb == DO:
        if opt == 31:  # NAWS
            send(bytes([IAC, WILL, 31, IAC, SB, 31, 0, 80, 0, 25, IAC, SE]))
        elif opt in (24, 0, 3):
            send(bytes([IAC, WILL, opt]))
        else:
            send(bytes([IAC, WONT, opt]))
    elif verb == WILL:
        send(bytes([IAC, DO if opt in (1, 3, 0) else DONT, opt]))

def feed(data):
    global state, sub, raw
    out = bytearray()
    for b in data:
        if state == 'data':
            if b == IAC: state = 'iac'
            else: out.append(b)
        elif state == 'iac':
            if b == IAC: out.append(IAC); state = 'data'
            elif b in (WILL, WONT, DO, DONT): state = ('verb', b)
            elif b == SB: sub = b''; state = 'sub'
            else: state = 'data'
        elif isinstance(state, tuple):
            handle_cmd(state[1], b); state = 'data'
        elif state == 'sub':
            if b == IAC: state = 'subiac'
            else: sub += bytes([b])
        elif state == 'subiac':
            if b == SE:
                if sub[:2] == bytes([24, 1]):
                    send(bytes([IAC, SB, 24, 0]) + b'syncterm' + bytes([IAC, SE]))
                state = 'data'
            else:
                sub += bytes([b]); state = 'sub'
    raw += bytes(out)
    return bytes(out)

def pump(t):
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([s], [], [], 0.1)
        if r:
            d = s.recv(65536)
            if not d: return False
            feed(d)
    return True

def clean():
    return re.sub(r'\x1b\[[0-9;?]*[A-Za-z]', '', raw.decode('cp437'))

def bridge(cmd):
    p = subprocess.Popen(cmd, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=os.environ.get('XFER_DIR', '.'), stderr=subprocess.DEVNULL)
    out_fd = p.stdout.fileno()
    while True:
        r, _, _ = select.select([s, out_fd], [], [], 0.2)
        if s in r:
            d = s.recv(65536)
            if not d: break
            data = feed(d)
            if data:
                try: p.stdin.write(data); p.stdin.flush()
                except BrokenPipeError: pass
        if out_fd in r:
            d = os.read(out_fd, 65536)
            if not d: break
            send(d.replace(b'\xff', b'\xff\xff'))
        if p.poll() is not None and out_fd not in r:
            break
    return p.wait()

def solve(wrong=False):
    """Read the bot check like a terminal would: replay cursor moves."""
    b = raw[raw.rfind(b'in order'):]
    line, x, i = {}, 0, 0
    while i < len(b):
        if b[i] == 0x1b and b[i+1:i+2] == b'[':
            m = re.match(rb'\x1b\[([0-9;]*)([A-Za-z])', b[i:])
            n = int(m.group(1) or b'1') if m.group(1).isdigit() else 1
            if m.group(2) == b'C': x += n
            elif m.group(2) == b'D': x -= n
            i += m.end(); continue
        c = b[i]
        if c == 13: x = 0
        elif c >= 32: line[x] = chr(c); x += 1
        i += 1
    text = ''.join(line.get(k, ' ') for k in range(max(line) + 1)) if line else ''
    found = re.findall(r'\[([A-Z0-9]+)\]', text)
    print('   challenge reads as:', found)
    for k in found:
        if wrong: k = 'Z' if k != 'Z' else 'Y'
        send(b'\x1b' if k == 'ESC' else k.encode())
        time.sleep(0.15)

keys = {'\\r': '\r', '\\e': '\x1b', '\\x15': '\x15', '{RIGHT}': '\x1b[C', '{LEFT}': '\x1b[D'}
cmark = 0
for step in steps:
    pat, _, out = step.partition('=>')
    optional = pat.startswith('opt:')
    pat = pat.removeprefix('opt:')
    deadline = time.time() + (2.5 if optional else 10)
    m = None
    while time.time() < deadline:
        m = re.search(pat, clean()[cmark:], re.S)
        if m or not pump(0.2): break
    if not m and optional:
        print(f'skipped: {pat!r}'); continue
    if not m:
        print(f'!! TIMEOUT waiting for {pat!r}\n--- tail:\n{clean()[cmark:][-800:]}'); break
    print(f'ok: {pat!r}')
    cmark += m.end()
    if out in ('@solve', '@wrong'):
        solve(out == '@wrong')
        continue
    if out.startswith('!'):
        print(f'   ran {out[1:]!r}: exit {bridge(out[1:])}')
        continue
    for k, v in keys.items(): out = out.replace(k, v)
    send(out.encode('cp437').replace(b'\r', b'\r\x00'))
    time.sleep(0.05)
pump(1.5)
open(os.path.join(os.path.dirname(__file__), 'tlast.raw'), 'wb').write(raw)
print('options seen:', sorted(set(log)))
