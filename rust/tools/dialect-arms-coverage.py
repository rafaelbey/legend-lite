#!/usr/bin/env python3
# Copyright 2026 Legend Contributors
# SPDX-License-Identifier: Apache-2.0
"""The arms of the eight Java dialects left to port that the oracles reach, measured (docs/rust-port/PHASE1.md step 6a).

A COPY of each dialect's source (Snowflake, BigQuery, Databricks, Trino, ClickHouse, MariaDb, MySql, MongoDb with
MongoDbPipeline and MongoJson) is instrumented in a scratch directory, every line where it was:

  - every `switch (sel)` becomes `switch (ArmHits.at(ID, sel))`, which records the selector's value for the dialect
    rendering and returns it unchanged: an enum switch takes the arm its value labels, so a label is REACHED when a
    render met it (a pattern switch's value is recorded by its class and supertypes; a guarded label is counted by
    its type alone);
  - every `new DialectCapability(` becomes `new DialectCapability(ID, `, and every call of a helper that makes one
    (`wall(fn, why)`, ...) is wrapped as `ArmHits.wall(ID, helper(...))`: a wall is REACHED when a render built it;
  - the base's pass loop records, per dialect, each pass that changed a query (`ArmHits.pass`).

The copies are compiled with the build JDK's javac against core's built jars and put FIRST on the class path of
rust/oracles/mir/DialectArms.java, which renders its inputs and replays the dialects dump (the eight dialects' lines,
each read back by MirJson and answered again on a fresh dialect, held to the dump's answer), so the measure is of the
two oracles together. The instrumented run's oracle must equal the plain run's byte for byte. Usage, from the
repository root, after the plain run of DialectArms.java wrote ORACLE:

  python3 rust/tools/dialect-arms-coverage.py <java> <javac> <classpath> <ORACLE> <dialects-dump.jsonl> <scratch dir> \
      [<the generator, rust/oracles/mir/DialectArms.java by default>]

It prints, per dialect: its exhaustive `call` switch's SqlFn labels reached (a label of MySql's is reached when both
MySql's switch and MariaDb's, to which each arm delegates, met it; MariaDb's methods MySql overrides without calling
`super` are not MySql's), its exhaustive aggregate switches' SqlAgg.Fn labels and every other SqlAgg.Fn switch's, its
own walls, its other enum and constant switches' labels, its pattern switches' labels, and the passes that changed a
query; then every label, wall and pass not reached. Switches and walls in the SqlDialect methods no render, DDL, DML or
engine-text call reaches (OFF_OPS) are counted apart. The dialects dump's text is //rust/oracles:dialects_dump's.
"""

import collections
import os
import re
import subprocess
import sys

DIALECT_DIR = "core/src/main/java/com/legend/sql/dialect"
DIALECTS = ["Snowflake", "BigQuery", "Databricks", "Trino", "ClickHouse", "MariaDb", "MySql", "MongoDb"]
# the files each dialect's own decisions live in (MySql runs MariaDb's code where it does not override it)
OWN = {
    "Snowflake": ["Snowflake"], "BigQuery": ["BigQuery"], "Databricks": ["Databricks"], "Trino": ["Trino"],
    "ClickHouse": ["ClickHouse"], "MariaDb": ["MariaDb"], "MySql": ["MySql", "MariaDb"],
    "MongoDb": ["MongoDb", "MongoDbPipeline", "MongoJson"],
}
INSTRUMENTED = ["Snowflake", "BigQuery", "Databricks", "Trino", "ClickHouse", "MariaDb", "MySql", "MongoDb",
                "MongoDbPipeline", "MongoJson"]
KEYWORDS = {"if", "for", "while", "switch", "return", "new", "catch", "synchronized", "throw", "else", "case"}


def mask(src):
    """The source with every comment and string/char literal's content blanked (same length, newlines kept)."""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2) + 2
            for k in range(i, j):
                if src[k] != "\n":
                    out[k] = " "
            i = j
        elif src.startswith('"""', i):
            j = src.find('"""', i + 3) + 3
            for k in range(i + 3, j - 3):
                if src[k] != "\n":
                    out[k] = " "
            i = j
        elif c == '"' or c == "'":
            j = i + 1
            while src[j] != c:
                j += 2 if src[j] == "\\" else 1
            for k in range(i + 1, j):
                out[k] = " "
            i = j + 1
        else:
            i += 1
    return "".join(out)


def matching(m, i):
    """The index of the bracket closing the one at i (in the masked text)."""
    depth = 0
    for j in range(i, len(m)):
        if m[j] in "({[":
            depth += 1
        elif m[j] in ")}]":
            depth -= 1
            if depth == 0:
                return j
    raise ValueError("unbalanced at %d" % i)


def labels(src, m, body_open, body_close):
    """The case labels of a switch body, each the text between `case` and `->`, at the body's own depth."""
    out = []
    depth = 0
    i = body_open + 1
    while i < body_close:
        ch = m[i]
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        elif depth == 0 and re.match(r"(case|default)\b", m[i:i + 8]) and not re.match(r"\w", m[i - 1]):
            arrow = m.find("->", i)
            text = " ".join(src[i:arrow].split())
            if text.startswith("default"):
                out.append("default")
            else:
                out.extend(split_labels(text[len("case"):].strip()))
            i = arrow + 2
            continue
        i += 1
    return out


def split_labels(text):
    parts, depth, cur = [], 0, ""
    for ch in text:
        if ch in "(<[":
            depth += 1
        elif ch in ")>]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append(cur.strip())
            cur = ""
        else:
            cur += ch
    parts.append(cur.strip())
    return parts


def line_of(src, i):
    return src.count("\n", 0, i) + 1


METHOD = re.compile(r"\n    (?:@\w+\s+)*(?:(?:public|protected|private|static|final|abstract|default)\s+)*"
                    r"(?:<[^>\n]*>\s*)?[\w.][\w.<>\[\]?, @]*\s+(\w+)\s*\(")


def method_at(m, i):
    """The name of the method declared nearest before i (for the report)."""
    best = None
    for mt in METHOD.finditer(m, 0, i):
        if mt.group(1) not in KEYWORDS:
            best = mt.group(1)
    return best


def instrument(name, src, sites, walls, next_id):
    m = mask(src)
    edits = []  # (index, insert_text)
    # the helpers that make a DialectCapability
    helpers = set(re.findall(r"DialectCapability\s+(\w+)\s*\(", m)) - {"DialectCapability"}
    for mt in re.finditer(r"\bswitch\s*\(", m):
        open_ = mt.end() - 1
        close = matching(m, open_)
        body_open = m.index("{", close)
        body_close = matching(m, body_open)
        sid = next_id[0]
        next_id[0] += 1
        sites[sid] = {"file": name, "line": line_of(src, mt.start()), "method": method_at(m, mt.start()),
                      "selector": " ".join(src[open_ + 1:close].split()),
                      "labels": labels(src, m, body_open, body_close)}
        edits.append((open_ + 1, "ArmHits.at(%d, " % sid))
        edits.append((close, ")"))
    for mt in re.finditer(r"\bnew\s+DialectCapability\s*\(", m):
        wid = next_id[0]
        next_id[0] += 1
        method = method_at(m, mt.start())
        walls[wid] = {"file": name, "line": line_of(src, mt.start()), "method": method, "helper": method in helpers}
        edits.append((mt.end(), "%d, " % wid))
    for h in sorted(helpers):
        for mt in re.finditer(r"(?<![\w.])%s\s*\(" % h, m):
            if re.search(r"DialectCapability\s+$", m[max(0, mt.start() - 40):mt.start()]):
                continue  # the declaration
            close = matching(m, mt.end() - 1)
            wid = next_id[0]
            next_id[0] += 1
            walls[wid] = {"file": name, "line": line_of(src, mt.start()), "method": method_at(m, mt.start()),
                          "helper": False, "via": h}
            edits.append((mt.start(), "ArmHits.wall(%d, " % wid))
            edits.append((close + 1, ")"))
    out = src
    for idx, ins in sorted(edits, key=lambda e: e[0], reverse=True):
        out = out[:idx] + ins + out[idx:]
    return out


ARM_HITS = r'''package com.legend.sql.dialect;

/** The coverage recorder of rust/tools/dialect-arms-coverage.py (scratch build only). */
public final class ArmHits {
    private static volatile String dialect = "?";
    private static final java.util.Set<String> HITS = java.util.concurrent.ConcurrentHashMap.newKeySet();

    static {
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            try {
                java.util.List<String> lines = new java.util.ArrayList<>(HITS);
                java.util.Collections.sort(lines);
                java.nio.file.Files.write(java.nio.file.Path.of(System.getProperty("arms.hits")), lines,
                        java.nio.charset.StandardCharsets.UTF_8);
            } catch (java.io.IOException e) {
                throw new java.io.UncheckedIOException(e);
            }
        }));
    }

    public static void dialect(String d) {
        dialect = d;
    }

    public static <T> T at(int site, T v) {
        HITS.add("S\t" + dialect + "\t" + site + "\t" + label(v));
        return v;
    }

    public static <T extends RuntimeException> T wall(int site, T e) {
        HITS.add("W\t" + dialect + "\t" + site);
        return e;
    }

    static void built(int site) {
        HITS.add("W\t" + dialect + "\t" + site);
    }

    public static com.legend.sql.SqlQuery pass(Object pass, com.legend.sql.SqlQuery before,
            com.legend.sql.SqlQuery after) {
        HITS.add("P\t" + dialect + "\t" + pass.getClass().getName() + "\t" + (before.equals(after) ? "same" : "fired"));
        return after;
    }

    /** kind TAB value TAB the value's class and supertypes ('|' after each): an enum constant by its class and name,
     *  a constant by its UTF-16 units in hex, anything else by its class alone. */
    private static String label(Object v) {
        if (v == null) {
            return "null\tnull\t";
        }
        if (v instanceof Enum<?> e) {
            return "enum\t" + e.getDeclaringClass().getName().replace('$', '.') + "." + e.name() + "\t" + supers(v);
        }
        if (v instanceof String || v instanceof Number || v instanceof Character || v instanceof Boolean) {
            StringBuilder b = new StringBuilder("const\t");
            for (char c : String.valueOf(v).toCharArray()) {
                b.append(String.format("%04x", (int) c));
            }
            return b + "\t" + supers(v);
        }
        return "class\t" + v.getClass().getName().replace('$', '.') + "\t" + supers(v);
    }

    private static String supers(Object v) {
        StringBuilder b = new StringBuilder();
        java.util.ArrayDeque<Class<?>> todo = new java.util.ArrayDeque<>(java.util.List.of(v.getClass()));
        java.util.Set<Class<?>> seen = new java.util.LinkedHashSet<>();
        while (!todo.isEmpty()) {
            Class<?> c = todo.pop();
            if (c != null && seen.add(c)) {
                todo.add(c.getSuperclass() == null ? Object.class : c.getSuperclass());
                todo.addAll(java.util.List.of(c.getInterfaces()));
            }
        }
        seen.forEach(c -> b.append(c.getName().replace('$', '.')).append('|'));
        return b.toString();
    }
}
'''

CAPABILITY = r'''package com.legend.sql.dialect;

/** DialectCapability recording where it is built (rust/tools/dialect-arms-coverage.py, scratch build only). */
public final class DialectCapability extends IllegalStateException {

    public DialectCapability(String message) {
        super(message);
    }

    public DialectCapability(int site, String message) {
        super(message);
        ArmHits.built(site);
    }
}
'''


def main():
    java, javac, classpath, oracle, dump, scratch = sys.argv[1:7]
    root = os.getcwd()
    generator = sys.argv[7] if len(sys.argv) > 7 else os.path.join(root, "rust/oracles/mir/DialectArms.java")
    src_dir = os.path.join(scratch, "src", "com", "legend", "sql", "dialect")
    cls_dir = os.path.join(scratch, "classes")
    os.makedirs(src_dir, exist_ok=True)
    os.makedirs(cls_dir, exist_ok=True)
    sites, walls, next_id = {}, {}, [1]
    for name in INSTRUMENTED:
        with open(os.path.join(root, DIALECT_DIR, name + ".java")) as f:
            src = f.read()
        with open(os.path.join(src_dir, name + ".java"), "w") as f:
            f.write(instrument(name, src, sites, walls, next_id))
    with open(os.path.join(root, DIALECT_DIR, "AnsiSqlRenderer.java")) as f:
        base = f.read()
    loop = "q = pass.rewriteRoot(q);"
    assert base.count(loop) == 1, "the base's pass loop moved"
    base = base.replace(loop, "q = ArmHits.pass(pass, q, pass.rewriteRoot(q));")
    with open(os.path.join(src_dir, "AnsiSqlRenderer.java"), "w") as f:
        f.write(base)
    with open(os.path.join(src_dir, "ArmHits.java"), "w") as f:
        f.write(ARM_HITS)
    with open(os.path.join(src_dir, "DialectCapability.java"), "w") as f:
        f.write(CAPABILITY)
    files = [os.path.join(src_dir, x) for x in sorted(os.listdir(src_dir))]
    subprocess.run([javac, "-nowarn", "-encoding", "UTF-8", "-proc:none", "-d", cls_dir, "-cp", classpath] + files,
                   check=True)
    hits_file = os.path.join(scratch, "hits.tsv")
    again = os.path.join(scratch, "dialect-arms.instrumented.jsonl")
    subprocess.run([java, "-Xss64m", "-Darms.hits=" + hits_file, "-cp", cls_dir + os.pathsep + classpath,
                    generator, again, dump], check=True)
    with open(oracle, "rb") as a, open(again, "rb") as b:
        if a.read() != b.read():
            sys.exit("the instrumented run's oracle differs from the plain run's: the recording changed a render")
    report(sites, walls, hits_file)


def label_type(label):
    """A pattern label's type (`SqlExpr.Column c when ...` -> `SqlExpr.Column`), or None for a constant label."""
    if label in ("default", "null"):
        return None
    t = label.split(" when ")[0].strip().split()
    return t[0] if len(t) == 2 else None


def java_literal(label):
    """A constant label's value (a string or char literal decoded, a number as written), or None."""
    if len(label) >= 2 and label[0] == label[-1] and label[0] in "\"'":
        body, out, i = label[1:-1], "", 0
        while i < len(body):
            if body[i] == "\\":
                c = body[i + 1]
                if c == "u":
                    out += chr(int(body[i + 2:i + 6], 16))
                    i += 6
                    continue
                out += {"n": "\n", "r": "\r", "t": "\t", "b": "\b", "f": "\f", "0": "\0"}.get(c, c)
                i += 2
            else:
                out += body[i]
                i += 1
        return out
    if re.fullmatch(r"-?\d+", label):
        return label
    return None


def enum_constants(path):
    """The constants of the first enum declared in a source file."""
    with open(path) as f:
        m = mask(f.read())
    body = m[m.index("{", m.index("enum ")) + 1:]
    body = body[:body.index(";")]
    return {x.strip() for x in body.split(",") if re.fullmatch(r"\s*[A-Z][A-Z_0-9]*\s*", x)}


def overridden_without_super():
    """Per dialect whose own decisions span a parent's file (MySql): the methods it declares that never call the
    parent's (super.name(...)), whose switches and walls in the parent's file it cannot reach."""
    out = {}
    for d, files in OWN.items():
        if len(files) < 2 or d == "MongoDb":
            continue
        with open(os.path.join(DIALECT_DIR, files[0] + ".java")) as f:
            m = mask(f.read())
        declared = {mt.group(1) for mt in METHOD.finditer(m) if mt.group(1) not in KEYWORDS}
        out[d] = {name for name in declared if not re.search(r"\bsuper\.%s\s*\(" % name, m)}
    return out


def report(sites, walls, hits_file):
    values = collections.defaultdict(set)    # (dialect, site) -> {(kind, value, supers)}
    built = collections.defaultdict(set)     # dialect -> wall ids
    fired = collections.defaultdict(dict)    # dialect -> pass -> fired?
    with open(hits_file, encoding="utf-8") as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if p[0] == "S":
                values[(p[1], int(p[2]))].add((p[3], p[4], p[5]))
            elif p[0] == "W":
                built[p[1]].add(int(p[2]))
            elif p[0] == "P":
                fired[p[1]][p[2]] = fired[p[1]].get(p[2], False) or p[3] == "fired"
    sqlfn = enum_constants("core/src/main/java/com/legend/sql/SqlFn.java")
    aggfn = enum_constants("core/src/main/java/com/legend/sql/SqlAgg.java")

    def enum_class(sid):
        """The enum a switch is over: what its values were, else what its labels spell."""
        for d in DIALECTS:
            for kind, v, _ in values.get((d, sid), []):
                if kind == "enum":
                    return v.rsplit(".", 1)[0]
        named = {lab for lab in sites[sid]["labels"] if lab != "default"}
        if named and named <= sqlfn and not named <= aggfn:
            return "com.legend.sql.SqlFn"
        if named and named <= aggfn and not named <= sqlfn:
            return "com.legend.sql.SqlAgg.Fn"
        return None

    def reached_labels(d, sid):
        """The labels of site sid a render on dialect d took: the first arm whose label holds the value (a guarded
        pattern label by its type alone), else default."""
        site = sites[sid]
        got = set()
        for kind, v, supers in values.get((d, sid), []):
            sup = [s for s in supers.split("|") if s]
            taken = None
            for lab in site["labels"]:
                if lab == "default":
                    continue
                t = label_type(lab)
                if kind == "enum" and t is None and (lab == v.rsplit(".", 1)[1] or v.endswith("." + lab)):
                    taken = lab
                elif kind == "const" and t is None and java_literal(lab) is not None and "".join(
                        "%04x" % ord(ch) for ch in java_literal(lab)) == v:
                    taken = lab
                elif kind == "null" and lab == "null":
                    taken = lab
                elif t is not None and kind != "null" and any(s == t or s.endswith("." + t) for s in sup):
                    got.add(lab)
                    if " when " in lab:
                        continue
                    taken = lab
                if taken:
                    break
            got.add(taken or "default")
        return got

    rows = []
    misses = []
    shadowed = overridden_without_super()
    for d in DIALECTS:
        files = OWN[d]

        def owns(info):
            # a parent's method the dialect overrides without calling it is not the dialect's (MySql over MariaDb)
            return info["file"] in files and not (info["file"] != files[0] and info["method"] in shadowed.get(d, ()))
        mine = [sid for sid, s in sites.items() if owns(s)]
        sqlfn_sites = [sid for sid in mine if enum_class(sid) == "com.legend.sql.SqlFn"
                       and len(sites[sid]["labels"]) >= 150]
        agg_sites = [sid for sid in mine if enum_class(sid) == "com.legend.sql.SqlAgg.Fn"]
        exhaustive = [sid for sid in agg_sites if set(sites[sid]["labels"]) == aggfn]
        fn_labels = sorted({lab for sid in sqlfn_sites for lab in sites[sid]["labels"]})
        fn_hit = [lab for lab in fn_labels if all(lab in reached_labels(d, sid) for sid in sqlfn_sites
                                                  if lab in sites[sid]["labels"])]
        for lab in fn_labels:
            if lab not in fn_hit:
                misses.append((d, "SqlFn %s (call)" % lab, "sqlfn"))
        counts = collections.Counter()
        for sid in mine:
            site = sites[sid]
            if sid in sqlfn_sites:
                continue
            got = reached_labels(d, sid)
            off = site["method"] in OFF_OPS
            group = ("off" if off else "aggx" if sid in exhaustive else "agg" if sid in agg_sites else
                     "pattern" if any(label_type(lab) for lab in site["labels"]) else "enum")
            for lab in site["labels"]:
                counts[group + "_total"] += 1
                if lab in got:
                    counts[group + "_hit"] += 1
                elif not off:
                    misses.append((d, "%s:%d %s(switch %s) label %s" % (site["file"], site["line"], site["method"],
                                                                        site["selector"], lab), group))
        own_walls = [w for w, info in walls.items() if owns(info) and not info["helper"]
                     and info["method"] not in OFF_OPS]
        wall_hit = [w for w in own_walls if w in built.get(d, set())]
        for w in own_walls:
            if w not in built.get(d, set()):
                info = walls[w]
                misses.append((d, "%s:%d %s wall%s" % (info["file"], info["line"], info["method"],
                                                       " via " + info["via"] if "via" in info else ""), "wall"))
        off_walls = [w for w, info in walls.items() if owns(info) and not info["helper"]
                     and info["method"] in OFF_OPS]
        passes = fired.get(d, {})
        for p, v in passes.items():
            if not v:
                misses.append((d, "pass %s never changed a query" % p.rsplit(".", 1)[-1], "pass"))

        def frac(k):
            return "%d/%d" % (counts[k + "_hit"], counts[k + "_total"])
        rows.append((d, "%d/%d" % (len(fn_hit), len(fn_labels)), frac("aggx"), frac("agg"),
                     "%d/%d" % (len(wall_hit), len(own_walls)), frac("enum"), frac("pattern"),
                     "%d/%d" % (sum(1 for v in passes.values() if v), len(passes)),
                     "%d labels, %d walls" % (counts["off_total"], len(off_walls))))
    header = ("dialect", "SqlFn (call)", "SqlAgg.Fn (exhaustive)", "SqlAgg.Fn (other)", "own walls",
              "other enum/constant labels", "pattern labels", "passes changing a query", "off the ops")
    print("\t".join(header))
    for r in rows:
        print("\t".join(r))
    print("\n(off the ops: switches and walls in %s, which no render, DDL, DML or engine-text call reaches)"
          % ", ".join(sorted(OFF_OPS)))
    for d in DIALECTS:
        mine = [(x, g) for dd, x, g in misses if dd == d]
        print("\n== %s: %d not reached" % (d, len(mine)))
        for x, g in mine:
            print("  [%s] %s" % (g, x))


# SqlDialect methods no render, DDL, DML or engine-text call reaches (the base calls none of them): their decisions
# are the seam's facts, not this oracle's
OFF_OPS = {"wireTypeName", "typingWireName", "deliversOwn", "sessionTable", "sessionSetup", "keysSnapshot",
           "resultColumnLimit", "needsStaticPivot", "temporaryTables"}

if __name__ == "__main__":
    main()
