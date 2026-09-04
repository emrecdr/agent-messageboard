#!/usr/bin/env python3
"""Items that had a doc comment and now have none — M63's defect, made mechanical.

**The shape.** Insert a new item between a `///` block and the function it documents, and the
block adopts the newcomer. Nothing fails: `cargo doc` renders happily, the compiler is silent, and
the newcomer now carries a summary describing something else while the robbed item carries none.
`claims::end_session` opened *"Take or renew a claim"* — the summary of the function that **writes**
claims, on the one that **lapses** them — and `identity::MAX_NAME`, a `usize`, opened *"The roster
upsert, reporting anything it displaced."* Nine instances by 2026-09-04, four of them landing in
the hours after M63 recorded the shape and said no guard existed. A defect that recurs while its
description is still the newest text in the file is one a description cannot hold.

**Scope, stated because the omission is deliberate.** This reports only items that **lost** a doc
they used to have. It says nothing about an item that shipped bare — `messages::unknown_project`
did, and this check would not have caught it. That is a different defect (a backlog: 33 public
items are undocumented today, and 242 with struct fields counted, which is why `missing_docs` was
measured and declined in M63) and it wants a different decision. Mixing them would make this check
un-adoptable on day one, which is the failure mode of every lint turned on over a large codebase.

**Keyed on a qualified path, never a bare name, and that is the whole reason this is a second
attempt.** The first version keyed on `(file, name)`. D114 added `Events::all` beside the existing
module-level `vendors::all`, and it reported the module-level one as having lost a doc it still
has — one false positive in four on its first outing, from exactly the name-collision blind spot
this project had recorded in `find_unread_fields.py` two days earlier (M65). An instrument that
reproduces the defect it was built to catch is worse than none, because it arrives with the
authority of a check. So the key here is `file::Impl::name`, tracked by brace depth.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

ITEM = re.compile(
    r"^\s*(?:pub(?:\([a-z:]+\))?\s+)?"
    r"(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\"[A-Za-z]+\"\s+)?"
    r"(fn|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
SCOPE_OPEN = re.compile(r"^\s*(?:pub(?:\([a-z:]+\))?\s+)?(impl|mod|trait)\b([^{]*)\{")


def scope_label(kind: str, rest: str) -> str:
    """`impl Events`, `impl Display for Check` and `mod x` reduce to the type they hang off."""
    rest = rest.strip()
    if kind == "impl":
        # `<T> Trait for Type` and `Type` both key on the last path segment before `{`.
        target = rest.split(" for ")[-1] if " for " in rest else rest
        target = re.sub(r"<[^>]*>", "", target).strip()
        return target.split("::")[-1] or "impl"
    return rest.split()[0] if rest.split() else kind


def documented_items(text: str) -> dict:
    """`{qualified_name: has_doc}` for every fn/const/static outside `#[cfg(test)]`."""
    out, stack, depth = {}, [], 0
    skip_until = None
    lines = text.split("\n")
    for i, line in enumerate(lines):
        stripped = line.strip()
        # A `#[cfg(test)]` module is not shipped; its contents are out of scope.
        if skip_until is not None:
            depth += line.count("{") - line.count("}")
            if depth <= skip_until:
                skip_until = None
            continue
        if stripped.startswith("#[cfg(test)]"):
            skip_until = depth
            depth += line.count("{") - line.count("}")
            continue

        m = ITEM.match(line)
        if m:
            j = i - 1
            while j >= 0 and (
                lines[j].strip().startswith("#[") or lines[j].strip().startswith("#!")
            ):
                j -= 1
            has_doc = j >= 0 and lines[j].strip().startswith("///")
            key = "::".join(stack + [m.group(2)])
            # A name repeated within one scope (a macro-ish duplicate) keeps the first verdict.
            out.setdefault(key, has_doc)
        else:
            s = SCOPE_OPEN.match(line)
            if s:
                stack.append(scope_label(s.group(1), s.group(2)))
                depth += line.count("{") - line.count("}")
                if depth <= len(stack) - 1:
                    stack.pop()
                continue
        before = depth
        depth += line.count("{") - line.count("}")
        while stack and depth < before and depth <= len(stack) - 1:
            stack.pop()
    return out


def at_ref(ref: str, path: str) -> str:
    r = subprocess.run(
        ["git", "show", f"{ref}:{path}"], capture_output=True, text=True, cwd=ROOT
    )
    return r.stdout if r.returncode == 0 else ""


def main() -> int:
    base = sys.argv[1] if len(sys.argv) > 1 else "HEAD"
    findings = []
    for p in sorted(list((ROOT / "src").rglob("*.rs"))):
        rel = p.relative_to(ROOT).as_posix()
        before = at_ref(base, rel)
        if not before:
            continue  # a new file cannot have lost anything
        now = documented_items(p.read_text())
        was = documented_items(before)
        for key, had in was.items():
            if had and key in now and not now[key]:
                findings.append((rel, key))

    if not findings:
        print(f"no item lost a doc comment since {base}.")
        return 0
    print(f"{len(findings)} item(s) lost a doc comment since {base} — M63's shape:\n")
    for rel, key in findings:
        print(f"  {rel} :: {key}")
    print(
        "\nAn item inserted between a `///` block and its function adopts the block, leaving that\n"
        "function bare and the newcomer describing something else. Move the block back down onto\n"
        "the item it describes. If the doc was deliberately removed, this check has to be told so\n"
        "by the doc being replaced rather than deleted."
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
