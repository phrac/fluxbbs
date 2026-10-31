#!/usr/bin/env python3
"""A fake MRC server, for testing flux/bbs's MRC chat without the real one.

    python3 tools/fake_mrc.py [PORT]        (plain TCP, 127.0.0.1, default 5555)

It answers the way uMRC's server does, as far as the board needs: HELLO after
the board's first line, ping every 30 seconds, rooms (NEWROOM, USERROOM,
ROOMTOPIC), USERLIST, MOTD, LIST, STATS, LOGOFF; chat is echoed to everyone
connected. A remote user, Zed of OtherBBS, is always in #lobby: he greets
whoever joins it and sends them a direct message.
"""
import asyncio
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 5555
conns = set()            # writers
users = {}               # nick -> (room, writer)
topics = {"lobby": "Welcome to the fake MRC lobby"}


def pkt(fu, fs, fr, tu, ext, tr, body):
    return f"{fu}~{fs}~{fr}~{tu}~{ext}~{tr}~{body}~\n".encode()


async def to_all(data):
    for w in list(conns):
        try:
            w.write(data)
            await w.drain()
        except Exception:
            conns.discard(w)


def room_users(room):
    names = [n for n, (r, _) in users.items() if r == room]
    if room == "lobby":
        names.append("Zed")
    return sorted(names, key=str.lower)


async def zed_greets(nick):
    await asyncio.sleep(0.5)
    await to_all(pkt("Zed", "OtherBBS", "lobby", "", "", "lobby", f"|08<|15Zed|08> |07hi {nick}, welcome to the lobby"))
    await to_all(pkt("Zed", "OtherBBS", "", nick, "", "", "|15* |08(|15Zed|08/|14DirectMsg|08) |07psst, this is a direct message"))
    # What other boards really send: raw CP437 (not UTF-8), and a try at
    # driving the terminal with CP437's glyph for ESC.
    await to_all(b"Zed~OtherBBS~lobby~~~lobby~|08<|15Zed|08> |07caf\x82 \xc9\xcd\xbb cp437~\n")
    await to_all(pkt("Zed", "OtherBBS", "lobby", "", "", "lobby", "|08<|15Zed|08> |07\u2190[2J\u2190[H no escape"))


async def server_cmd(w, p):
    fu, fs, fr, tu, ext, tr, body = p
    cmd, _, arg = body.partition(":")
    reply = lambda b, room="": w.write(pkt("SERVER", "", "", fu, "", room, b))
    if body.startswith("NEWROOM:"):
        old, _, new = arg.partition(":")
        users[fu] = (new, w)
        reply(f"USERROOM:{new}")
        reply(f"ROOMTOPIC:{new}:{topics.get(new, 'no topic')}", new)
        await to_all(pkt("SERVER", "", new, "NOTME", "", new, f"|07- |11{fu} |03is Joining |15#{new}"))
        if new == "lobby":
            asyncio.create_task(zed_greets(fu))
    elif body.startswith("NEWTOPIC:"):
        room, _, topic = arg.partition(":")
        topics[room] = topic
        await to_all(pkt("SERVER", "", "", "", "", room, f"ROOMTOPIC:{room}:{topic}"))
    elif cmd == "USERLIST":
        room = users.get(fu, ("lobby", w))[0]
        reply("USERLIST:" + ",".join(room_users(room)), room)
    elif cmd in ("MOTD", "motd"):
        reply("|07Welcome to the |15fake MRC|07 server.")
    elif cmd in ("LIST", "list"):
        for r in sorted(set(["lobby"] + [r for r, _ in users.values()])):
            reply(f"|15#{r} |07({len(room_users(r))}) {topics.get(r, '')}")
    elif cmd == "STATS":
        reply("STATS:3 2 %d 1" % (len(users) + 1))
    elif cmd == "LOGOFF":
        room = users.pop(fu, ("lobby", w))[0]
        await to_all(pkt("SERVER", "", room, "NOTME", "", room, f"|07- |12{fu} |04is Leaving"))
    elif fu == "CLIENT" or cmd in ("IAMHERE", "TERMSIZE", "NICKCHANGED", "BBSMETA"):
        pass
    else:
        reply(f"|12Unknown command: {body}")
    await w.drain()


async def handle(r, w):
    conns.add(w)
    first = await r.read(256)
    print("hello:", first)
    w.write(pkt("SERVER", "", "", "CLIENT", "", "", "HELLO"))
    await w.drain()

    async def pinger():
        while True:
            await asyncio.sleep(30)
            w.write(pkt("SERVER", "", "", "CLIENT", "", "", "ping"))
            await w.drain()

    ping = asyncio.create_task(pinger())
    try:
        while line := await r.readline():
            s = line.decode("cp437", "replace").rstrip("\r\n")
            print("in:", s)
            p = s.split("~")
            if len(p) < 7:
                continue
            if p[3] == "SERVER":
                await server_cmd(w, p[:7])
            else:
                await to_all(line)
    finally:
        ping.cancel()
        conns.discard(w)


async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", PORT)
    print(f"fake MRC on 127.0.0.1:{PORT}")
    async with srv:
        await srv.serve_forever()


asyncio.run(main())
