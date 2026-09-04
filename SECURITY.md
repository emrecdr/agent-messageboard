# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities **privately**, not in a public issue or pull request.

Use GitHub's private vulnerability reporting:
<https://github.com/emrecdr/agent-messageboard/security/advisories/new>. If that form is not
available to you, open a public issue that says *only* that you have a security report and would
like a private channel — no details — and one will be arranged.

You can expect an acknowledgement within a few days. If a fix is warranted it lands on `main`, and
you will be credited in the advisory unless you ask otherwise.

## Supported versions

`amb` is pre-1.0 and installed from source. Only the current `main` (and the most recent tag, once
releases exist) is supported; there are no backports to older tags. The on-disk schema is
disposable by design (see below), so the remedy for most local trouble is to move the board file
aside — `amb doctor` says when that is the answer.

## What `amb` is, for the purpose of a threat model

`amb` is a **local, single-user coordination tool**: one static binary over one SQLite file, no
daemon, no network listener, no authentication (decisions D3, D15). Every session on a board is one
of *your own* agent-CLI sessions, running as *your own* OS user, on one machine. It is not a
multi-user service and does not try to be one.

That fixes the trust boundary. Everything on the board — messages, advisory file claims, memory
notes — is written by cooperating local processes under a single uid. `amb` does not authenticate
message senders, because on the far side of that boundary there is no distinct principal to
authenticate.

### Treated as untrusted (in scope)

- **Hook stdin.** The delivery and memory hooks parse arbitrary JSON handed to them by the host
  CLI. That path is defensive by contract: it **always exits 0**, and it **creates no database for
  a session that never used `amb`** (D9). A malformed or hostile payload must degrade to silence,
  never to a broken session or a spurious board. Reports that break either property are in scope.
- **Sender-written fields.** A message subject, body, or claim intent is attacker-influenceable
  text. It is contained at the renderers so it cannot forge `amb`'s own framing or headings
  (`delivery::UNTRUSTED`, D90), and it is bounded at the writer by refusal rather than truncation
  (D106). A field that escapes its container — injecting what reads as `amb`'s own voice into
  another session's context — is in scope.

### Best-effort, and not to be relied on

- **Secret redaction** in the memory vault is **named-shape matching** (D46), a safety net for
  accidental pastes — not a guarantee. Do not rely on it to scrub secrets from anything you would
  mind leaking; a shape it does not know passes through. New provider key shapes are welcome as
  ordinary contributions.
- **File claims are advisory and never block** (D5). They surface who is editing what; they are
  not a lock and confer no exclusion.

### Out of scope

- Attacks that require a **second machine or a second user** — `amb` has no network surface and no
  cross-machine transport (a same-network hub is an open question, not a feature; see
  `docs/OPEN-QUESTIONS.md` Q11).
- A **malicious local process running as the same user**. It can already read your files, your
  board, and your vault directly; `amb` is not a sandbox and does not defend the user from
  themselves.
- Loss of the **board file** to a crash or deletion. It holds ephemeral coordination state and is
  disposable by decision (D15); the memory vault is the durable half and is protected separately.

If you are unsure whether something is in scope, report it privately anyway and let the triage
sort it out.
