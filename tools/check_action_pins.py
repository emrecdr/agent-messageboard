#!/usr/bin/env python3
"""Fail if any GitHub Actions workflow references an action by a mutable tag.

In the gate. A tag like `actions/checkout@v6` is a *moving* reference: whoever controls the
action's repository can repoint it after review, and every workflow on every subsequent run picks
up the new code. A 40-character commit SHA cannot move. This is the same reasoning `Cargo.lock`
encodes for dependencies, applied to the one dependency class Cargo does not cover.

**Why a script rather than a convention.** The three workflows here were pinned by hand and
nothing was watching them. `release.yml` is *generated* by `dist`, and `dist`'s default output
uses floating tags — so the pin lives in `dist-workspace.toml` under `github-action-commits`, and
a `dist generate` run with that key removed silently unpins every action in the file. Nothing
would fail. That is this repository's recurring shape: a rule kept by whoever remembered it,
until someone did not.

**A SHA-shaped string is checked, not the SHA itself.** Whether `d23441a…` is really `v6` of
`actions/checkout` is not decidable here, and pretending otherwise would be the "instrument
answering a neighbouring question" failure this project keeps finding. What is decidable is
mutability, which is the property that matters: a resolved SHA is immutable whoever resolved it.
Use `gh api repos/OWNER/NAME/commits/TAG --jq .sha` to obtain one.

A trailing `# v7` comment is encouraged and deliberately *not* required — `dist` does not emit
one, and a rule that the generated file cannot satisfy is a rule that gets switched off.
"""

import pathlib
import re
import sys

# `uses:` may be quoted, and may sit on a `- uses:` list item. The ref is everything after the
# final `@`; an action path itself never contains one.
USES = re.compile(r"""^\s*-?\s*uses:\s*["']?([^"'\s]+)["']?""")
SHA = re.compile(r"^[0-9a-f]{40}$")


def unpinned(path: pathlib.Path) -> list[tuple[int, str, str]]:
    """Every (line number, action, reason) in one workflow that is not pinned to a commit."""
    out = []
    for n, line in enumerate(path.read_text().splitlines(), 1):
        m = USES.match(line)
        if not m:
            continue
        ref = m.group(1)
        # A local composite action is this repository's own code, already at this commit.
        if ref.startswith("./"):
            continue
        # `docker://image:tag` is a different mutability problem with a different fix (a digest);
        # named rather than silently passed, so nobody reads this check as covering it.
        if ref.startswith("docker://"):
            out.append((n, ref, "docker image reference — pin by @sha256: digest"))
            continue
        if "@" not in ref:
            out.append((n, ref, "no ref at all — resolves to the default branch"))
            continue
        _, _, at = ref.rpartition("@")
        if not SHA.match(at):
            out.append((n, ref, f"{at!r} is a mutable tag or branch, not a 40-hex commit"))
    return out


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    workflows = sorted((root / ".github" / "workflows").glob("*.yml"))
    workflows += sorted((root / ".github" / "workflows").glob("*.yaml"))
    if not workflows:
        # A clean run over nothing is the failure mode `check_orphaned_docs.py` shipped with:
        # confident green, nothing read. Refuse instead.
        print("check_action_pins: no workflows found — expected .github/workflows/*.yml")
        return 1

    bad = 0
    for wf in workflows:
        for n, ref, why in unpinned(wf):
            rel = wf.relative_to(root)
            print(f"{rel}:{n}: {ref} — {why}")
            bad += 1

    if bad:
        print(
            f"\ncheck_action_pins: {bad} unpinned action(s). Resolve each with\n"
            f"  gh api repos/OWNER/NAME/commits/TAG --jq .sha\n"
            f"and for .github/workflows/release.yml — which `dist` generates — put the pin in\n"
            f"dist-workspace.toml under [dist.github-action-commits], never in the file itself."
        )
        return 1

    uses = sum(len(list(filter(USES.match, w.read_text().splitlines()))) for w in workflows)
    print(
        f"check_action_pins: ok — {uses} action reference(s) across "
        f"{len(workflows)} workflow(s), all pinned to commits"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
