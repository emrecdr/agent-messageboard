#!/usr/bin/env python3
"""Separate "not compiled on this host" from "not tested" in a cargo-mutants report.

**This exists because the two print identically and one of them is not a finding.** Mutating a
`#[cfg(target_os = "linux")]` function on macOS builds fine — the mutated code is simply absent
from the binary — so every test passes and the row prints MISSED. `db.rs` reported **16 such rows
in one run of 29 missed** (M46): 55% of that module's apparent survivors were code this host never
compiled. Read as a score, the module looked half-tested; read correctly, 13 rows were real.

Upstream will not fix this soon and says so. The cargo-mutants book's own limitations page:
*"cargo-mutants does not yet understand conditional compilation, such as `#[cfg(target_os =
"linux")]`. It will report functions for other platforms as missed, when it should know to skip
them."* Checked against 27.1.0 (the current release, 2026-06-02) on 2026-09-02.

**The documented workaround is worse than the problem here, which is why this script exists
instead.** `#[cfg_attr(not(target_os = "linux"), mutants::skip)]` looks exactly right and is a
trap: the book states cargo-mutants *does not evaluate the `cfg_attr` condition* and honours the
inner `mutants::skip` unconditionally. So the annotation that reads "skip this only where it is
not compiled" in fact skips it **everywhere**, including on the Linux leg where the code is live
and the mutant is real — silently removing the one platform whose coverage the annotation was
written to preserve. It would also add `mutants` as a regular (not dev) dependency to a binary
that keeps six.

So the classification is done here, after the run, against the host actually used.

**What it cannot do, named rather than hidden:** it answers *"is this row uncatchable on this
host"*, never *"is this code tested somewhere"*. A phantom row on macOS is a real, unanswered
question for CI's Linux leg, and the fallback arm that compiles on no platform in CI is a question
for nobody — this script moves those rows out of the score, it does not close them. It also
refuses rather than guesses: a `cfg` predicate whose shape it does not know is reported as
UNKNOWN and exits non-zero, because a classifier that silently treats "I cannot tell" as "fine"
is the failure this project keeps finding in its own instruments (M35, M40).

    python3 tools/cfg_phantoms.py [mutants.out]

Exit 0 when every missed row was classified, 1 when it could not answer.
"""

import json
import re
import sys
from pathlib import Path

# What is true of the host running this. Deliberately explicit rather than a table lookup: a
# platform whose flags are guessed is exactly the "I cannot tell" case this refuses.
def host_flags():
    if sys.platform == "darwin":
        return {"target_os": "macos", "target_family": "unix"}, {"unix"}
    if sys.platform.startswith("linux"):
        return {"target_os": "linux", "target_family": "unix"}, {"unix"}
    if sys.platform in ("win32", "cygwin"):
        return {"target_os": "windows", "target_family": "windows"}, {"windows"}
    return None, None


class Unknown(Exception):
    """A predicate shape this script does not model. Never guessed at."""


def evaluate(pred: str, keys: dict, bare: set) -> bool:
    """Evaluate one `cfg` predicate. Raises Unknown rather than returning a default."""
    pred = pred.strip()
    m = re.fullmatch(r"(not|any|all)\s*\((.*)\)", pred, re.S)
    if m:
        op, inner = m.group(1), m.group(2)
        parts = split_top_level(inner)
        if op == "not":
            if len(parts) != 1:
                raise Unknown(pred)
            return not evaluate(parts[0], keys, bare)
        results = [evaluate(p, keys, bare) for p in parts]
        return any(results) if op == "any" else all(results)
    m = re.fullmatch(r'([a-z_]+)\s*=\s*"([^"]*)"', pred)
    if m:
        key, want = m.group(1), m.group(2)
        if key not in keys:
            # An unmodelled key (`feature`, `target_arch`, ...) is not assumed false: assuming
            # false marks live code as phantom, which removes a real survivor from the count.
            raise Unknown(pred)
        return keys[key] == want
    if re.fullmatch(r"[a-z_]+", pred):
        if pred == "test":
            return True  # mutants are built under `cargo test`
        if pred in ("unix", "windows"):
            return pred in bare
        raise Unknown(pred)
    raise Unknown(pred)


def split_top_level(s: str) -> list:
    """Split on commas that are not inside parentheses."""
    parts, depth, cur = [], 0, ""
    for ch in s:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    return [p for p in parts if p.strip()]


def inactive_spans(path: Path, keys: dict, bare: set):
    """Line ranges (1-based, inclusive) governed by a `cfg` that is false on this host.

    The item a `cfg` governs runs to the end of its block, so the span is found by matching braces
    from the first `{` after the attribute. An attribute on a `;`-terminated item (a `use`, a
    field) governs one line and cannot contain a function, which is why that case needs no care.
    """
    text = path.read_text(encoding="utf-8", errors="replace")
    lines = text.splitlines()
    spans, unknowns = [], []
    for i, line in enumerate(lines):
        m = re.match(r"\s*#\[cfg\((.*)", line)
        if not m:
            continue
        # An attribute may wrap. Accumulate until the `#[...]` brackets balance.
        attr, depth, j = "", 0, i
        while j < len(lines):
            for ch in lines[j][lines[j].index("#[") if j == i else 0:]:
                attr += ch
                if ch == "[":
                    depth += 1
                elif ch == "]":
                    depth -= 1
                    if depth == 0:
                        break
            if depth == 0:
                break
            j += 1
        inner = re.fullmatch(r"#\[cfg\((.*)\)\]", attr.strip(), re.S)
        if not inner:
            continue
        try:
            if evaluate(inner.group(1), keys, bare):
                continue
        except Unknown as e:
            # Still find the span. A row inside an unevaluable gate is *unclassified*, and
            # counting it as a real survivor would be this instrument asserting exactly what it
            # has just said it cannot determine.
            unknowns.append((path.as_posix(), i + 1, str(e)))
            spans.append(("?", i + 1, block_end(lines, j)))
            continue
        spans.append(("off", i + 1, block_end(lines, j)))
    return spans, unknowns


def block_end(lines, start: int) -> int:
    """1-based last line of the item beginning at or after `start`, by brace matching."""
    depth, started = 0, False
    for k in range(start, len(lines)):
        for ch in lines[k]:
            if ch == "{":
                depth += 1
                started = True
            elif ch == "}":
                depth -= 1
        if started and depth == 0:
            return k + 1
        if not started and lines[k].rstrip().endswith(";"):
            return k + 1
    return len(lines)


# The classification this script exists to make is one nobody re-checks by hand, so it checks
# itself. Every row below was confirmed by breaking the evaluator — with `evaluate` forced to
# `True`, the whole fixture reports "5 real, 0 not compiled here", which is precisely the
# pre-script status quo this exists to end (M36: a check that cannot fail is not a check).
SELF_TEST = [
    # (cfg attribute or None, expected verdict on a unix host)
    ('#[cfg(target_os = "linux")]', "off" if sys.platform == "darwin" else "real"),
    ('#[cfg(target_os = "macos")]', "real" if sys.platform == "darwin" else "off"),
    ('#[cfg(not(any(target_os = "macos", target_os = "linux")))]', "off"),
    ("#[cfg(unix)]", "real"),
    ("#[cfg(not(unix))]", "off"),
    ("#[cfg(not(test))]", "off"),  # absent from the test binary, so uncatchable by construction
    ('#[cfg(all(unix, target_os = "linux"))]', "off" if sys.platform == "darwin" else "real"),
    ('#[cfg(feature = "whatever")]', "?"),  # unmodelled: refused, never guessed
    (None, "real"),
]


def self_test() -> int:
    import tempfile

    keys, bare = host_flags()
    if keys is None:
        print(f"unmodelled host {sys.platform!r}", file=sys.stderr)
        return 1
    failures = []
    with tempfile.TemporaryDirectory() as td:
        src = Path(td) / "fixture.rs"
        body, want, line = [], [], 1
        for attr, expect in SELF_TEST:
            if attr:
                body.append(attr)
                line += 1
            body.append(f"fn f{len(want)}() -> u32 {{")
            want.append((line, expect))
            body += ["    41", "}", ""]
            line += 4
        src.write_text("\n".join(body))
        spans, unknowns = inactive_spans(src, keys, bare)
        for (ln, expect), (attr, _) in zip(want, SELF_TEST):
            tags = {t for t, a, b in spans if a <= ln <= b}
            got = "off" if "off" in tags else "?" if "?" in tags else "real"
            if got != expect:
                failures.append(f"  line {ln} {attr or '(no cfg)'}: want {expect}, got {got}")
        if len(unknowns) != 1:
            failures.append(f"  expected exactly 1 unmodelled predicate, got {len(unknowns)}")
    if failures:
        print(f"self-test FAILED on {sys.platform}:", file=sys.stderr)
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(f"self-test passed: {len(SELF_TEST)} cfg shapes classified correctly on {sys.platform}.")
    return 0


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        return self_test()
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "mutants.out")
    report = out / "outcomes.json"
    if not report.exists():
        print(f"no report at {report} — this check read nothing, which is an inability to"
              " answer rather than a clean result.", file=sys.stderr)
        return 1
    keys, bare = host_flags()
    if keys is None:
        print(f"unmodelled host platform {sys.platform!r}: refusing to classify.", file=sys.stderr)
        return 1

    data = json.loads(report.read_text())
    missed = [o for o in data.get("outcomes", []) if o.get("summary") == "MissedMutant"]
    if not data.get("outcomes"):
        print("the report lists no outcomes, so this check read nothing.", file=sys.stderr)
        return 1

    cache, phantoms, real, unclassified, unknowns = {}, [], [], [], []
    for o in missed:
        mut = o["scenario"]["Mutant"]
        f = mut["file"]
        if f not in cache:
            cache[f] = inactive_spans(Path(f), keys, bare)
        spans, unk = cache[f]
        unknowns.extend(unk)
        line = mut["function"]["span"]["start"]["line"] if mut.get("function") else \
            mut["span"]["start"]["line"]
        verdicts = {tag for tag, a, b in spans if a <= line <= b}
        if "off" in verdicts:
            phantoms.append(mut["name"])
        elif "?" in verdicts:
            unclassified.append(mut["name"])
        else:
            real.append(mut["name"])

    host = keys["target_os"]
    tail = f", {len(unclassified)} unclassified" if unclassified else ""
    print(f"{len(missed)} missed row(s) on {host}: {len(real)} real,"
          f" {len(phantoms)} not compiled here{tail}.")
    if phantoms:
        print("\nnot present on this host — these are questions for the other platform's leg,")
        print("not survivors, and they must not be counted in this run's score:")
        for name in phantoms:
            print(f"  {name}")
    if real:
        print("\nreal survivors on this host:")
        for name in real:
            print(f"  {name}")
    if unclassified:
        print("\nunclassified — under a cfg gate this script cannot evaluate, so neither"
              " counted nor cleared:")
        for name in unclassified:
            print(f"  {name}")
    if unknowns:
        # Deduplicated: one unmodelled predicate reported once, not once per mutant examined.
        seen = sorted(set(unknowns))
        print(f"\n{len(seen)} cfg predicate(s) this script does not model. It cannot say whether"
              " rows under them are real, so this run is unclassified:", file=sys.stderr)
        for f, ln, pred in seen:
            print(f"  {f}:{ln}  cfg({pred})", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
