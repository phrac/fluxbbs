#!/usr/bin/env python3
"""Split a `dosrun` trace for `replay_trace.py --chunk` runs in parallel.

    trace_chunks.py trace.bin trace.mem N

Writes chunk_K.json (the record offset and count of chunk K) and
chunk_K.mem (memory as it was when chunk K began, found by undoing the
later records' writes from the final memory) beside the trace.
"""
import json
import mmap
import os
import struct
import sys

HEAD = struct.Struct('<B8I6HHII')


def main():
    path, mem_path, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    out = os.path.dirname(os.path.abspath(path))
    f = open(path, 'rb')
    data = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    total, = struct.unpack_from('<Q', data, 0)
    per = -(-total // n)
    # Pass 1: where each chunk starts.
    starts = []
    p = 8
    for i in range(total):
        if i % per == 0:
            starts.append(p)
        nw = struct.unpack_from('<I', data, p + HEAD.size - 4)[0]
        p += HEAD.size + 6 * nw
    starts.append(p)
    counts = [min(per, total - k * per) for k in range(len(starts) - 1)]
    # Pass 2: memory at each chunk's start, undoing from the end.
    mem = bytearray(open(mem_path, 'rb').read())
    for k in range(len(counts) - 1, -1, -1):
        writes = []
        p = starts[k]
        for _ in range(counts[k]):
            nw = struct.unpack_from('<I', data, p + HEAD.size - 4)[0]
            base = p + HEAD.size
            for i in range(nw):
                a, old, _ = struct.unpack_from('<IBB', data, base + 6 * i)
                writes.append((a, old))
            p = base + 6 * nw
        for a, old in reversed(writes):
            mem[a] = old
        open(os.path.join(out, f'chunk_{k}.mem'), 'wb').write(mem)
        json.dump({'offset': starts[k], 'count': counts[k], 'first': k * per}, open(os.path.join(out, f'chunk_{k}.json'), 'w'))
        print(f'chunk {k}: records {k * per}..{k * per + counts[k]}', flush=True)


if __name__ == '__main__':
    main()
