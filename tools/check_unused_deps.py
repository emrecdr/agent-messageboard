#!/usr/bin/env python3
"""Fail if a crate in `[dependencies]` is never referenced by any code that ships.

In the gate. `anyhow` sat in `Cargo.toml` for the life of the project, was compiled into every
build, and was imported by nothing — while `src/error.rs`'s module docs stated where it was used
(D117). The comment is what made it survive: an unused dependency looks like an oversight, and a
documented one looks deliberate.

**`cargo` structurally cannot report this.** An unused dependency is not dead code — nothing is
compiled from it, so there is no item to lint. `cargo-udeps` needs nightly and `cargo-machete` is
another binary to install and keep current; this repository's gate is Python and `cargo`, and the
question is simple enough to answer directly.

**Comments are stripped before searching, and that is the whole difficulty.** The sibling check
`find_unread_fields.py` was repaired for exactly this (M39): a name mentioned in prose reads as a
use. `src/error.rs` now discusses `anyhow` by name in the paragraph explaining its removal, so a
naive grep would report it as used the moment anyone re-added it — the check would pass *because*
of the comment describing why it should fail.

**What this does not check.** A dependency reachable only through a macro that never spells the
crate name would read as unused here. No such case exists in this tree, and the failure direction
is a false alarm rather than a false clean — which is the safe way round for a gate. If one ever
appears, add it to `ALLOW` with the reason, rather than weakening the search.
"""

import pathlib
import re
import sys

# Crates that are genuinely used without their name appearing in source. Empty, deliberately:
# a populated allowlist is how a check stops meaning anything, so each entry needs a reason.
ALLOW: dict[str, str] = {}

ROOT = pathlib.Path(__file__).resolve().parent.parent


def declared() -> list[str]:
    """Crate names under `[dependencies]` — not dev-dependencies, which only tests bind."""
    out, inside = [], False
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("["):
            inside = s == "[dependencies]"
            continue
        if not inside or not s or s.startswith("#"):
            continue
        if "=" in s:
            out.append(s.split("=", 1)[0].strip())
    return out


def strip_comments(src: str) -> str:
    """Remove `//`-to-end-of-line and `/* */` comments.

    Crude on purpose. A `//` inside a string literal — a URL, say — loses the rest of that line,
    which can only cause a false alarm and never a false clean.
    """
    src = re.sub(r"/\*.*?\*/", " ", src, flags=re.S)
    return "\n".join(line.split("//", 1)[0] for line in src.splitlines())


def shipping_code() -> tuple[str, int]:
    """Every line of source that ends up in the binary, comments removed."""
    files = sorted(ROOT.glob("src/**/*.rs"))
    for extra in ("build.rs",):
        p = ROOT / extra
        if p.exists():
            files.append(p)
    return "\n".join(strip_comments(f.read_text(encoding="utf-8")) for f in files), len(files)


def main() -> int:
    deps = declared()
    if not deps:
        # An inability to answer, not a clean result — the rule `check_docs.py` was repaired for.
        print("check_unused_deps: no [dependencies] found — the check examined nothing")
        return 1
    code, n_files = shipping_code()
    if not n_files:
        print("check_unused_deps: no source files found — the check examined nothing")
        return 1

    unused = []
    for dep in deps:
        if dep in ALLOW:
            continue
        # Cargo maps `-` in a crate name to `_` in the path used by code.
        ident = dep.replace("-", "_")
        used = (
            re.search(rf"\b{re.escape(ident)}\s*::", code)
            or re.search(rf"\buse\s+{re.escape(ident)}\b", code)
            or re.search(rf"\b{re.escape(ident)}\s*!", code)
            or re.search(rf"\bextern\s+crate\s+{re.escape(ident)}\b", code)
        )
        if not used:
            unused.append(dep)

    if unused:
        for dep in unused:
            print(
                f"Cargo.toml: {dep!r} is declared under [dependencies] and never referenced in "
                f"src/ — remove it, or add it to ALLOW in this file with the reason"
            )
        print(
            f"\ncheck_unused_deps: {len(unused)} unused dependency(ies). A dependency nothing "
            f"imports is still compiled, still in Cargo.lock, and still part of what a release "
            f"ships and an audit covers."
        )
        return 1

    print(
        f"check_unused_deps: ok — {len(deps)} dependency(ies) checked against "
        f"{n_files} source file(s), every one referenced"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
