#!/usr/bin/env python3
# Copyright 2026 Legend Contributors
# SPDX-License-Identifier: Apache-2.0
"""The plan oracle's host test plans, deduplicated: the writer, the reader, and the check that the one rebuilds the
archive's text exactly (docs/rust-port/PHASE3_SHIM_DESIGN.md section 10, ruling 18; rust/oracles/plan/MANIFEST.tsv).

THE INPUT is the archive the plan oracle froze (plan-test-plans-host.tsv.zst): one line per test plan, sorted by id,
`id TAB T TAB text`, the text the plan's TestPlanJson exactly as Java wrote it, its id the SHA-256 of the text's UTF-16
units, little-endian (the probe's id, and rust/oracles/front's).

THE FORMAT (version 1): two stores, a key index and a frame table.

  test-plans-host.tsv.zst         one line per plan, sorted by id: id TAB T TAB the plan's text, OUTLINED
  test-plan-values.tsv.zst        one line per outlined value: key TAB the value's text, OUTLINED, in the order the plans
                                  first reach the values (the plans in id order; in each text its references in order,
                                  each value's own references before the next: depth first), so that the values one
                                  plan reaches lie in a few frames
  test-plan-values-index.tsv.zst  for every value, sorted: the first 16 hex digits of its key (no two keys share them;
                                  the writer refuses a store where two would) TAB the number of the values frame it is in
  test-plan-frames.tsv            every frame of the two stores, in order: store TAB frame number TAB the key its first
                                  line begins with TAB byte offset TAB byte length (compressed) TAB lines

A VALUE is a JSON value of a plan's text, from its first byte to its last: a string token with its quotes (a member's
name is one too), an array or an object. OUTLINING replaces a value by a REFERENCE: the character U+0001, the value's
KEY as 64 lowercase hex digits, U+0001 (66 bytes). A value's key is its text's id by the plan's rule: the SHA-256 of the
UTF-16 units, little-endian, of the value's own text, nothing outlined. The writer outlines a value when its text is at
least --min bytes of UTF-8 (8,192 by default: of the thresholds measured, 256 to 16,384, the smallest store) and it
occurs at least twice in the archive, counting every occurrence, inside another value too; outermost first: a plan's
line holds the references to its outermost outlined values, and a value's line outlines only values strictly inside it,
so rebuilding always descends and cannot cycle. Each value is stored once.

REBUILDING a text: split it on U+0001; every odd piece is a key; put in its place that value's text, rebuilt the same
way. A plan's line rebuilt is the archive's line byte for byte; a value's rebuilt text hashes to its key.

A REFERENCE CANNOT BE JAVA'S TEXT: TestPlanJson.write is Json.toCompact (json/src/main/java/com/legend/json/Json.java),
which writes no character below U+0020. Every one inside a string or a member's name is escaped (Json.escapeTo: \\b \\f
\\n \\r \\t, or \\u00XX); outside strings the compact writer writes only []{}:, and the quote, true, false, null, and
number lexemes (Long.toString, BigDecimal.toString, Double.toString, or a lexeme its own parser accepted), and no
whitespace. The archive's texts hold no character below U+0020 at all (measured over all 30,212,522,705 bytes), and
the writer refuses a text with U+0001.

FRAMES: each .zst store is a concatenation of independent zstd frames (`zstd -19`), each of whole lines, up to --frame
bytes uncompressed (2 MiB by default; a longer line alone); plain `zstd -d` reads a store whole. A reader decompresses
only the frames it needs: a plan's is the last plans frame whose first key is not above its id; a value's is the
index's. The index is one frame, read whole.

Usage, from the repository root (Python 3.9; the zstd CLI on the path):

  python3 rust/tools/test-plan-texts.py write <plan-test-plans-host.tsv.zst> <out dir> [--min N] [--frame N] [--procs N]
  python3 rust/tools/test-plan-texts.py read <dir> <id> [<id> ...]
        each plan's exact text, then a newline, on stdout, through the frames
  python3 rust/tools/test-plan-texts.py check <dir> [--ids <plan-ids.tsv.zst>] [--sha <the archive text's SHA-256>]
        rebuilds every plan and checks it against its id, every value against its key, the frames against their table,
        the index against the values, the rebuilt archive's SHA-256, the ids against the plan oracle's host rows; and
        times the reader
"""

import bisect
import collections
import hashlib
import os
import random
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from multiprocessing import Pool

PLANS = "test-plans-host.tsv.zst"
VALUES = "test-plan-values.tsv.zst"
INDEX = "test-plan-values-index.tsv.zst"
FRAMES = "test-plan-frames.tsv"
MARK = b"\x01"
PREFIX = 16
DEPTH = 20000   # a value's nesting in a plan: the deepest MIR tree a plan holds is far shallower
# a string token (its escapes included), or a bracket outside one
TOKEN = re.compile(rb'"[^"\\]*(?:\\.[^"\\]*)*"|[\[\]{}]')


def text_id(b):
    """the oracle's id of a text given as its UTF-8 bytes (a lone surrogate as the probe wrote it)"""
    return hashlib.sha256(b.decode("utf-8", "surrogatepass").encode("utf-16-le", "surrogatepass")).hexdigest()


def refs(b):
    return b.split(MARK)[1::2]


def opt(args, name, default):
    if name in args:
        i = args.index(name)
        v = args[i + 1]
        del args[i:i + 2]
        return v
    return default


def unz(path):
    return subprocess.run(["zstd", "-dc", path], capture_output=True, check=True).stdout


# ---- write ------------------------------------------------------------------------------------------------------------

LEAST = 8192
OUTLINE = frozenset()
SENT = set()
KEYS = {}


def init(least, outline):
    global LEAST, OUTLINE
    sys.setrecursionlimit(DEPTH)
    LEAST = least
    OUTLINE = outline


def split_line(raw):
    line = raw[:-1] if raw.endswith(b"\n") else raw
    i, kind, text = line.split(b"\t", 2)
    if len(i) != 64 or kind != b"T":
        raise SystemExit("not an archive line: " + repr(line[:80]))
    return i, text


def forest(text):
    """every value of at least LEAST bytes, as [start, end, the values in it], nested as in the text"""
    vals = []
    stack = []
    for m in TOKEN.finditer(text):
        c = text[m.start()]
        if c == 0x22:
            if m.end() - m.start() >= LEAST:
                vals.append((m.start(), m.end()))
        elif c == 0x5B or c == 0x7B:
            stack.append((m.start(), c))
        else:
            s, o = stack.pop()
            if (o == 0x5B) != (c == 0x5D):
                raise SystemExit("a bracket closed by the other kind")
            if m.end() - s >= LEAST:
                vals.append((s, m.end()))
    if stack:
        raise SystemExit("a bracket never closed")
    vals.sort()
    roots = []
    open_ = []
    for s, e in vals:
        node = [s, e, []]
        while open_ and open_[-1][1] <= s:
            open_.pop()
        (open_[-1][2] if open_ else roots).append(node)
        open_.append(node)
    return roots


def count(raw):
    """pass 1: the line checked; every value of at least LEAST bytes, by the SHA-256 of its UTF-8"""
    i, text = split_line(raw)
    if text_id(text) != i.decode():
        raise SystemExit("an archive line whose id is not its text's: " + i.decode())
    if MARK in text:
        raise SystemExit("a text holding U+0001: " + i.decode())
    out = []
    work = forest(text)
    while work:
        s, e, kids = work.pop()
        out.append(hashlib.sha256(text[s:e]).digest())
        work.extend(kids)
    return out


def outline(text, start, end, kids, values):
    parts = []
    at = start
    for s, e, inner in kids:
        parts.append(text[at:s])
        at = e
        d = hashlib.sha256(text[s:e]).digest()
        if d in OUTLINE:
            key = KEYS.get(d)
            if key is None:
                key = KEYS[d] = text_id(text[s:e]).encode()
            parts.append(MARK + key + MARK)
            if key not in SENT:
                SENT.add(key)
                values[key] = outline(text, s, e, inner, values)
        else:
            parts.append(outline(text, s, e, inner, values))
    parts.append(text[at:end])
    return b"".join(parts)


def rewrite(raw):
    """pass 2: the plan's line outlined, and the values this process has not sent before"""
    i, text = split_line(raw)
    values = {}
    return i + b"\tT\t" + outline(text, 0, len(text), forest(text), values) + b"\n", values, len(raw)


def lines(archive):
    p = subprocess.Popen(["zstd", "-dc", "--long=27", archive], stdout=subprocess.PIPE, bufsize=1 << 24)
    for raw in p.stdout:
        yield raw
    if p.wait():
        raise SystemExit("zstd failed on " + archive)


def compress(chunk):
    return subprocess.run(["zstd", "-19", "-q", "-c"], input=chunk, capture_output=True, check=True).stdout


def write_store(path, lines_, frame):
    """lines_ in the store's order, each ending in a newline: frames of whole lines; [(first key, offset, length, n)]"""
    chunks = []
    cur = []
    size = 0
    for line in lines_:
        if cur and size + len(line) > frame:
            chunks.append(cur)
            cur, size = [], 0
        cur.append(line)
        size += len(line)
    if cur:
        chunks.append(cur)
    with ThreadPoolExecutor(os.cpu_count()) as ex:
        packed = list(ex.map(lambda c: compress(b"".join(c)), chunks))
    table = []
    off = 0
    with open(path, "wb") as o:
        for c, z in zip(chunks, packed):
            o.write(z)
            table.append((c[0][:64].decode(), off, len(z), len(c)))
            off += len(z)
    return table


def first_reach(plans, values):
    """the values in the order the plans first reach them: plans in order, references in order, depth first"""
    order = {}
    for line in plans:
        stack = list(reversed(refs(line)))
        while stack:
            k = stack.pop()
            if k not in order:
                order[k] = len(order)
                stack.extend(reversed(refs(values[k])))
    return sorted(order, key=order.get)


def write(args):
    least = int(opt(args, "--min", "8192"))
    frame = int(opt(args, "--frame", str(2 << 20)))
    procs = int(opt(args, "--procs", str(max(1, (os.cpu_count() or 2) - 2))))
    archive, out = args
    os.makedirs(out, exist_ok=True)
    t0 = time.time()
    counts = collections.Counter()
    with Pool(procs, initializer=init, initargs=(least, frozenset())) as pool:
        for ds in pool.imap(count, lines(archive), chunksize=4):
            counts.update(ds)
    keep = frozenset(d for d, c in counts.items() if c >= 2)
    print(f"pass 1, every line its id's: values of at least {least} bytes {sum(counts.values())}, distinct"
          f" {len(counts)}, outlined (twice or more) {len(keep)}; {round(time.time() - t0)} s", flush=True)
    del counts
    plans = []
    values = {}
    n = total = 0
    with Pool(procs, initializer=init, initargs=(least, keep)) as pool:
        for line, vals, ln in pool.imap(rewrite, lines(archive), chunksize=4):
            n += 1
            total += ln
            plans.append(line)
            for k, v in vals.items():
                if values.setdefault(k, v) != v:
                    raise SystemExit("one key, two stored texts: " + k.decode())
    print(f"pass 2: plans {n}, archive bytes {total}; {round(time.time() - t0)} s", flush=True)
    ids = [p[:64] for p in plans]
    if ids != sorted(set(ids)):
        raise SystemExit("the archive is not sorted by id, or an id is in it twice")
    order = first_reach(plans, values)
    if len(order) != len(values):
        raise SystemExit("a value no plan reaches")
    if len({k[:PREFIX] for k in order}) != len(order):
        raise SystemExit(f"two keys share their first {PREFIX} hex digits: the index cannot tell them apart")
    vlines = [k + b"\t" + values[k] + b"\n" for k in order]
    table = [(PLANS, i) + f for i, f in enumerate(write_store(os.path.join(out, PLANS), plans, frame))]
    vtable = write_store(os.path.join(out, VALUES), vlines, frame)
    table += [(VALUES, i) + f for i, f in enumerate(vtable)]
    frame_of = {}
    at = 0
    for i, (first, off, ln, nlines) in enumerate(vtable):
        for k in order[at:at + nlines]:
            frame_of[k] = i
        at += nlines
    index = b"".join(k[:PREFIX] + b"\t" + str(frame_of[k]).encode() + b"\n" for k in sorted(order))
    with open(os.path.join(out, INDEX), "wb") as o:
        o.write(compress(index))
    with open(os.path.join(out, FRAMES), "w", encoding="utf-8", newline="\n") as o:
        o.write("# store\tframe\tfirst key\tbyte offset\tbyte length\tlines\n")
        for row in table:
            o.write("\t".join(str(x) for x in row) + "\n")
    print(f"plans' lines {sum(len(p) for p in plans)} bytes; values {len(vlines)}, their lines"
          f" {sum(len(v) for v in vlines)} bytes; frames {len(table)}; {round(time.time() - t0)} s")
    for name, data in ((PLANS, plans), (VALUES, vlines), (INDEX, [index])):
        h = hashlib.sha256()
        for line in data:
            h.update(line)
        nl = sum(x.count(b"\n") for x in data)
        print(f"{name}: {os.path.getsize(os.path.join(out, name))} bytes; {nl} lines, SHA-256 of the uncompressed"
              f" text {h.hexdigest()}")
    print(f"{FRAMES}: {os.path.getsize(os.path.join(out, FRAMES))} bytes")


# ---- read -------------------------------------------------------------------------------------------------------------

class Store:
    """a plan by its id, through the frames: its plans frame and its values' frames, each decompressed once"""

    def __init__(self, d):
        self.dir = d
        self.table = collections.defaultdict(list)
        with open(os.path.join(d, FRAMES), encoding="utf-8") as h:
            for line in h:
                if not line.startswith("#"):
                    store, i, first, off, ln, n = line.rstrip("\n").split("\t")
                    if int(i) != len(self.table[store]):
                        raise SystemExit(f"{FRAMES}: {store}'s frames out of order")
                    self.table[store].append((first, int(off), int(ln), int(n)))
        self.firsts = [f[0] for f in self.table[PLANS]]
        self.index = None
        self.frames = {}
        self.values = {}

    def frame(self, store, i):
        got = self.frames.get((store, i))
        if got is None:
            first, off, ln, n = self.table[store][i]
            with open(os.path.join(self.dir, store), "rb") as h:
                h.seek(off)
                z = h.read(ln)
            got = self.frames[(store, i)] = subprocess.run(["zstd", "-dc"], input=z, capture_output=True,
                                                           check=True).stdout
        return got

    @staticmethod
    def find(text, key):
        k = key + b"\t"
        at = 0 if text.startswith(k) else text.find(b"\n" + k) + 1
        if at == 0 and not text.startswith(k):
            raise KeyError(key.decode())
        return text[at + len(k):text.index(b"\n", at)]

    def value(self, key):
        got = self.values.get(key)
        if got is None:
            if self.index is None:
                self.index = {}
                for line in unz(os.path.join(self.dir, INDEX)).split(b"\n")[:-1]:
                    p, f = line.split(b"\t")
                    self.index[p] = int(f)
            got = self.values[key] = self.expand(self.find(self.frame(VALUES, self.index[key[:PREFIX]]), key))
        return got

    def expand(self, b):
        parts = b.split(MARK)
        for i in range(1, len(parts), 2):
            parts[i] = self.value(parts[i])
        return b"".join(parts)

    def plan(self, plan_id):
        i = bisect.bisect_right(self.firsts, plan_id) - 1
        if i < 0:
            raise KeyError(plan_id)
        line = self.find(self.frame(PLANS, i), plan_id.encode())
        if not line.startswith(b"T\t"):
            raise SystemExit("not a test plan's line: " + plan_id)
        return self.expand(line[2:])


def read(args):
    s = Store(args[0])
    for i in args[1:]:
        sys.stdout.buffer.write(s.plan(i) + b"\n")


# ---- check ------------------------------------------------------------------------------------------------------------

def check(args):
    ids_path = opt(args, "--ids", None)
    want_sha = opt(args, "--sha", None)
    d = args[0]
    t0 = time.time()
    s = Store(d)
    whole = {}
    for store in (PLANS, VALUES):
        whole[store] = unz(os.path.join(d, store))
        if b"".join(s.frame(store, i) for i in range(len(s.table[store]))) != whole[store]:
            raise SystemExit(store + ": its frames are not the store")
        for i, (first, off, ln, n) in enumerate(s.table[store]):
            f = s.frame(store, i)
            if not f.startswith(first.encode() + b"\t") or f.count(b"\n") != n or not f.endswith(b"\n"):
                raise SystemExit(f"{store}: frame {i} is not its row")
        print(f"{store}: {len(s.table[store])} frames, each its row; {sum(r[3] for r in s.table[store])} lines;"
              f" SHA-256 of the uncompressed store {hashlib.sha256(whole[store]).hexdigest()}", flush=True)
    stored = {}
    order = []
    frame_of = {}
    lines_ = whole[VALUES].split(b"\n")[:-1]
    at = 0
    for i, (first, off, ln, n) in enumerate(s.table[VALUES]):
        for line in lines_[at:at + n]:
            k, v = line.split(b"\t", 1)
            if k in stored:
                raise SystemExit("a value stored twice: " + k.decode())
            stored[k] = v
            order.append(k)
            frame_of[k] = i
        at += n
    index = unz(os.path.join(d, INDEX))
    want_index = b"".join(k[:PREFIX] + b"\t" + str(frame_of[k]).encode() + b"\n" for k in sorted(stored))
    index_ok = index == want_index and len({k[:PREFIX] for k in stored}) == len(stored)
    print(f"{INDEX}: SHA-256 of the uncompressed index {hashlib.sha256(index).hexdigest()}; every value's frame, by"
          f" its key's first {PREFIX} hex digits, no two alike: {index_ok}", flush=True)
    memo = {}

    def expand(b):
        parts = b.split(MARK)
        for i in range(1, len(parts), 2):
            k = parts[i]
            got = memo.get(k)
            if got is None:
                got = memo[k] = expand(stored[k])
            parts[i] = got
        return b"".join(parts)

    bad = sum(1 for k in order if text_id(expand(stored[k])) != k.decode())
    print(f"values {len(order)}; rebuilt, whose key is not their text's id: {bad}", flush=True)
    h = hashlib.sha256()
    ids = []
    plans = whole[PLANS].split(b"\n")[:-1]
    bad_plans = chars = 0
    for line in plans:
        i, kind, text = line.split(b"\t", 2)
        t = expand(text)
        chars += len(t.decode("utf-8", "surrogatepass"))
        if kind != b"T" or text_id(t) != i.decode():
            bad_plans += 1
        h.update(i + b"\t" + kind + b"\t" + t + b"\n")
        ids.append(i.decode())
    in_order = first_reach([p + b"\n" for p in plans], stored) == order
    print(f"plans {len(ids)}, {chars} characters; rebuilt, whose id is not their text's: {bad_plans}; the values in"
          f" the order the plans first reach them, every one reached: {in_order}", flush=True)
    got = h.hexdigest()
    print(f"the rebuilt archive's SHA-256 {got}" + ("" if want_sha is None else
                                                     ("  = the archive's" if got == want_sha else "  NOT " + want_sha)))
    if ids != sorted(set(ids)):
        raise SystemExit("the plans are not sorted by id, or an id is twice")
    same = None
    if ids_path:
        host = set()
        for line in unz(ids_path).decode().splitlines():
            c = line.split("\t")
            m = re.search(r"testPlan(?:Ahead)?@[^,]*,(HOST|DATABASE|BOTH),", c[2])
            if not (m and m.group(1) in ("DATABASE", "BOTH")):
                host.update(c[5].split(",") if c[5] else [])
        same = host == set(ids)
        print(f"the test plan ids the host judge's rows name in {os.path.basename(ids_path)}: {len(host)}; the store's"
              f" ids exactly: {same}")
    print(f"check: {round(time.time() - t0)} s", flush=True)
    sample = random.Random(18).sample(ids, min(200, len(ids)))
    t1 = time.time()
    touched = []
    for i in sample:
        r = Store(d)
        if text_id(r.plan(i)) != i:
            raise SystemExit("the reader rebuilt a plan wrongly: " + i)
        touched.append(sum(len(f) for f in r.frames.values()))
    cold = (time.time() - t1) / len(sample)
    r = Store(d)
    t1 = time.time()
    for i in sample:
        r.plan(i)
    warm = (time.time() - t1) / len(sample)
    touched.sort()
    print(f"the reader, {len(sample)} random plans, each its id: {cold * 1000:.0f} ms a plan from a fresh reader"
          f" (decompressing, median, {touched[len(touched) // 2]} of {len(whole[PLANS]) + len(whole[VALUES])} bytes,"
          f" the most {touched[-1]}); {warm * 1000:.0f} ms a plan from one reader")
    if bad or bad_plans or not in_order or not index_ok or same is False or (
            want_sha is not None and got != want_sha):
        raise SystemExit("check FAILED")


if __name__ == "__main__":
    sys.setrecursionlimit(DEPTH)
    {"write": write, "read": read, "check": check}[sys.argv[1]](sys.argv[2:])
