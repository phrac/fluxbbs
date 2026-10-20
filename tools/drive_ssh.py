"""Drive an ssh session through a pty: drive_ssh.py USER TERM 'expect=>send' ...
Each step waits (up to 8s) for the regex in the de-ANSI'd output, then sends.
Sends understand \\r, \\e (ESC) and {UP}/{DOWN}/{LEFT}/{RIGHT}; '@solve' answers
the bot check by replaying its cursor moves (as a terminal would).
CPR=1 answers cursor position requests (ESC[6n) as a terminal does;
READ_BPS=n reads the output at n bytes a second, like a slow link."""
import os, pty, re, select, sys, time
user, term, steps = sys.argv[1], sys.argv[2], sys.argv[3:]
pid, fd = pty.fork()
if pid == 0:
    os.environ['TERM'] = term
    os.execvp('ssh', ['ssh', '-F', '/dev/null', '-tt', '-p', os.environ.get("PORT", "2222"), '-o', 'StrictHostKeyChecking=no',
                      '-o', 'UserKnownHostsFile=/dev/null', '-o', 'LogLevel=ERROR', f'{user}@{os.environ.get("HOST", "127.0.0.1")}'])
raw = b''
start = time.time()
cpr = os.environ.get('CPR') == '1'
read_bps = int(os.environ.get('READ_BPS', '0'))
def clean(b):
    t = b.decode('utf-8', 'replace') if term != 'syncterm' else b.decode('cp437')
    return re.sub(r'\x1b\[[0-9;?]*[A-Za-z]', '', t)
def pump(t):
    global raw
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            try: d = os.read(fd, max(1, read_bps // 10) if read_bps else 65536)
            except OSError: return False
            if not d: return False
            if cpr:
                for _ in range((raw[-3:] + d).count(b'\x1b[6n')):
                    os.write(fd, b'\x1b[24;80R')
            raw += d
            if read_bps: time.sleep(0.1)
    return True
def solve(wrong=False):
    """Read the bot check like a terminal would: replay cursor moves.
    The picture check (three big characters in colour) goes to botsolve."""
    # Let the screen finish arriving.
    end, size = time.time() + 5, -1
    while len(raw) != size and time.time() < end:
        size = len(raw)
        pump(0.4)
    start = raw.rfind(b'in order')
    clear = raw.rfind(b'\x1b[2J')
    if clear > start:
        import botsolve
        got = botsolve.solve(raw[clear:])
        print('   picture reads as:', got)
        if wrong: got = ''.join('Z' if c != 'Z' else 'Y' for c in got)
        time.sleep(0.8)
        for k in got:
            os.write(fd, k.encode())
            time.sleep(0.15)
        return
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
        os.write(fd, b'\x1b' if k == 'ESC' else k.encode())
        time.sleep(0.15)

keys = {'\\!': '!', '{UP}': '\x1b[A', '{DOWN}': '\x1b[B', '{LEFT}': '\x1b[D', '{RIGHT}': '\x1b[C', '\\r': '\r', '\\e': '\x1b', '\\x18': '\x18', '\\x0b': '\x0b', '\\x15': '\x15'}
cmark = 0
for step in steps:
    pat, _, send = step.partition('=>')
    optional = pat.startswith('opt:')
    pat = pat.removeprefix('opt:')
    long = pat.startswith('long:')
    pat = pat.removeprefix('long:')
    deadline = time.time() + (2.5 if optional else 45 if long else 8)
    m = None
    while time.time() < deadline:
        m = re.search(pat, clean(raw)[cmark:], re.S)
        if m or not pump(0.2): break
    if not m:
        m = re.search(pat, clean(raw)[cmark:], re.S)
    if not m and optional:
        print(f'skipped: {pat!r}'); continue
    if not m:
        print(f'!! TIMEOUT waiting for {pat!r}\n--- tail:\n{clean(raw)[cmark:][-1500:]}'); break
    print(f'ok: {pat!r}' + (f' ({time.time() - start:.1f}s)' if read_bps or cpr else ''))
    cmark += m.end()
    if send in ('@solve', '@wrong'):
        solve(send == '@wrong')
        continue
    if send.startswith('!'):
        import shlex, subprocess
        time.sleep(0.3)
        r = subprocess.run(shlex.split(send[1:]), stdin=fd, stdout=fd, stderr=open(os.path.join(os.path.dirname(__file__), 'proto.log'), 'a'), cwd=os.environ.get('XFER_DIR', '.'))
        print(f'   ran {send[1:]!r}: exit {r.returncode}')
        continue
    for k, v in keys.items(): send = send.replace(k, v)
    os.write(fd, send.encode('cp437' if term == 'syncterm' else 'utf-8'))
    time.sleep(0.05)
pump(1.5)
open(os.environ.get('RAW_OUT', os.path.join(os.path.dirname(__file__), 'last.raw')), 'wb').write(raw)
os.kill(pid, 9)
