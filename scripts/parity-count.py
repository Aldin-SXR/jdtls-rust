#!/usr/bin/env python3
"""Count upstream eclipse.jdt.ls tests and their jdtls-rust ports, per test class.

Usage: scripts/parity-count.py [--all] [--markdown]

Upstream tests are the `@Test` and `@ParameterizedTest` methods in
`eclipse.jdt.ls/org.eclipse.jdt.ls.tests*/src` (check out the reference tag first).
A class `org.eclipse.jdt.ls.core.internal.<pkg>.<Class>` is ported in
`tests/<pkg>_<class_snake>.rs` and/or, for unit ports, in a `mod <class_snake>`
block in `src/`. `#[test]` functions count as ported, `#[ignore` ones as ignored.
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UPSTREAM = os.path.join(ROOT, "eclipse.jdt.ls")

# Unit ports that do not follow the `mod <class_snake>` convention: (ported, ignored).
EXTRA = {
    "handlers/InitHandlerTest": (2, 0),  # src/server.rs, src/features/preferences.rs
    "handlers/InlayHintFilterManagerTest": (7, 0),  # src/features/inlay_hint_filter.rs `mod tests`
}


def snake(s):
    s = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", s)
    return re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s).lower()


def upstream_classes():
    out = {}
    for root in ("org.eclipse.jdt.ls.tests/src", "org.eclipse.jdt.ls.tests.syntaxserver/src"):
        for f in glob.glob(os.path.join(UPSTREAM, root, "**/*.java"), recursive=True):
            text = open(f, encoding="utf-8", errors="replace").read()
            n = len(re.findall(r"^\s*@(?:Test|ParameterizedTest)\b", text, re.M))
            if not n:
                continue
            rel = f.split("/core/internal/")[-1] if "/core/internal/" in f else f.split("/src/")[-1].split("/ls/")[-1]
            out[rel[:-5]] = n
    return out


def count(text):
    return len(re.findall(r"#\[test\]", text)), len(re.findall(r"#\[ignore", text))


def unit_modules():
    """`mod <name> {` blocks in src/ → (tests, ignored)."""
    mods = {}
    for f in glob.glob(os.path.join(ROOT, "src/**/*.rs"), recursive=True):
        lines = open(f, encoding="utf-8").read().split("\n")
        for i, line in enumerate(lines):
            m = re.match(r"^(\s*)mod (\w+_test) \{", line)
            if not m:
                continue
            indent = m.group(1)
            body = []
            for l in lines[i + 1:]:
                if l == indent + "}":
                    break
                body.append(l)
            t, ig = count("\n".join(body))
            pt, pig = mods.get(m.group(2), (0, 0))
            mods[m.group(2)] = (pt + t, pig + ig)
    return mods


def main():
    classes = upstream_classes()
    mods = unit_modules()
    rows = []
    for cls, n in sorted(classes.items()):
        parts = cls.split("/")
        name = snake(parts[-1])
        stem = "_".join(parts[:-1] + [name]) if len(parts) > 1 else name
        ported = ignored = 0
        path = os.path.join(ROOT, "tests", stem + ".rs")
        if os.path.exists(path):
            t, ig = count(open(path, encoding="utf-8").read())
            ported += t
            ignored += ig
        t, ig = mods.get(name, (0, 0))
        ported += t
        ignored += ig
        t, ig = EXTRA.get(cls, (0, 0))
        ported += t
        ignored += ig
        ported = min(ported, n)
        rows.append((cls, n, ported, ported - ignored, ignored))
    total = sum(r[1] for r in rows)
    ported = sum(r[2] for r in rows)
    passing = sum(r[3] for r in rows)
    ignored = sum(r[4] for r in rows)
    show_all = "--all" in sys.argv
    if "--markdown" in sys.argv:
        print("| Upstream class | Upstream | Ported | Passing | Ignored |\n|---|---:|---:|---:|---:|")
        for r in rows:
            if show_all or r[2]:
                print(f"| {r[0]} | {r[1]} | {r[2]} | {r[3]} | {r[4]} |")
    else:
        for r in sorted(rows, key=lambda r: r[2] - r[1]):
            if show_all or r[2] < r[1]:
                print(f"{r[1] - r[2]:5d} missing  {r[1]:4d} upstream  {r[2]:4d} ported  {r[4]:3d} ignored  {r[0]}")
    pct = lambda x: f"{100 * x / total:.1f}%"
    print(f"\nclasses {len(rows)}  upstream {total}  ported {ported} ({pct(ported)})  "
          f"passing {passing} ({pct(passing)})  ignored {ignored}  not ported {total - ported} ({pct(total - ported)})")
    areas = {}
    for cls, n, p, ok, _ in rows:
        a = cls.split("/")[0] if "/" in cls else "(root)"
        if a == "framework":
            a = "framework/" + cls.split("/")[1]
        x = areas.setdefault(a, [0, 0, 0])
        x[0] += n
        x[1] += p
        x[2] += ok
    print("\n| Area | Upstream | Ported | Passing | Passing % |\n|---|---:|---:|---:|---:|")
    for a, (n, p, ok) in sorted(areas.items(), key=lambda kv: -kv[1][0]):
        print(f"| {a} | {n} | {p} | {ok} | {100 * ok // n}% |")


if __name__ == "__main__":
    main()
