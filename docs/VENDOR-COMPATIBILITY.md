# Hosts beyond Claude Code — what `amb` can already serve, and what it cannot

> **Status: research complete, implementation deferred, 2026-09-06.** Nothing here has been built.
> The deferral is `OPEN-QUESTIONS.md` Q15, which owns the decision; this file owns the evidence and
> is where a revisit starts. Every finding carries **verified** or **unverified**, and the
> unverified ones say what verification requires.

Raised by the user: *"we are not just aiming claude code anymore, our tool also should be compatible
with other llm providers and also with google adk."*

## This question was decided once, and reopened once

**D101 decided `amb` stays Claude-Code-only** on 2026-08-31, closing Q8 — no per-vendor hook matrix,
now or on a schedule. **D111 reopened it** on 2026-09-02 on D101's own second condition, *"a second
agent tool is actually in use"*, and shipped Gemini CLI plus the manifest loader.

So this is the *third* pass, and the standing position is not "support everything" — it is D101's
ceiling as amended by D111: a vendor is data, and a host is worth adding when someone is actually
using it. That is the frame the recommendation below sits in, and it is why "build the one host that
was asked for" is a smaller claim than it looks.

## Findings at a glance

| # | Finding | Status | How |
|---|---|---|---|
| F1 | `Vendor` carries paths, events, tool vocabulary — **not the settings file's schema** | **verified** | read `hooks::plan_install` |
| F2 | `plan_install` writes Claude's shape: root `hooks`, `"type": "command"`, nested `{matcher, hooks:[…]}` | **verified** | installed against a throwaway `HOME`, read the file |
| F3 | `delivery::envelope` emits Claude's **output** shape too — `hookSpecificOutput.additionalContext` | **verified** | read the function; its docstring counts 200 / 128 occurrences in the two bundles |
| F4 | Gemini genuinely shares both shapes rather than tolerating them | **verified** | amb's written output vs Gemini's documented format |
| F5 | Cursor's schema is different: flatter, `matcher` on the handler, required top-level `version: 1` | **verified from docs** | [Cursor hooks docs](https://cursor.com/docs/hooks) |
| F6 | Codex CLI's schema — docs describe the *same* nested shape as Claude | **UNVERIFIED** | docs only; see F7 |
| F7 | An earlier claim of mine — "Codex is not a manifest away" — was **wrong**, and its replacement is inadmissible too | **verified as a process failure** | D111 requires the shipped binary; `~/.codex` here is from Feb 2026, predating hooks (v0.114, March) |
| F8 | `amb` already serves a host with **no settings file, no hook system, no session-id env var** | **verified** | payload-only invocation against the real binary |
| F9 | Every `amb` lane has an ADK callback analogue | **verified from docs** | [ADK plugins](https://adk.dev/plugins/) |
| F10 | ADK gives callbacks `tool_args`, so `edit_tools`' closed vocabulary can be sidestepped | **unverified** | docs; needs a running ADK |
| F11 | ADK identity must be `session.id`, not `invocation_id` — `AgentTool` can create a new `InvocationContext` | **unverified** | ADK discussions; needs a running ADK |
| F12 | There is **no cross-vendor hook standard to target**, by design | **verified from docs** | Agent Plugins 1.0 excludes hook behavior; Anthropic not a maintainer |
| F13 | D11's withdrawal condition does **not** fire for ADK | **verified** | condition reads "no hook mechanism at all"; ADK has one, in-process |

## What was examined, and what was not

Stated because a survey's silence is not a finding, and because D11's own survey sentence went stale
by being a count nobody re-read ("none of the five surveyed" when the survey was already fourteen).

| host | examined | how |
|---|---|---|
| Claude Code | yes | shipped vendor, source |
| Gemini CLI | yes | shipped vendor, source, docs, and amb's written output |
| Cursor | yes | published docs only |
| OpenAI Codex CLI | partially | published docs; **no binary available on this machine** |
| Google ADK | yes | published docs only; **not installed** |
| Windsurf, GitHub Copilot CLI, Amp, Cline, Continue, Zed, OpenCode | **no** | not examined at all |
| Aider | not re-examined | **D11** already records it as having no hook system |

## The load-bearing assumption, and it has two dimensions

D111 made a vendor **data** rather than a trait: `Vendor` carries paths, event spellings, tool
vocabulary and session variables, and `~/.config/amb/vendors/*.json` adds one with no rebuild.

What it does **not** carry is either end of the wire format.

**Input — the settings file's schema (F2).** `hooks::plan_install` hardcodes Claude's:

```console
$ HOME=/tmp/fake amb install --vendor gemini-cli
$ cat /tmp/fake/.gemini/settings.json
{"hooks": {"AfterAgent": [{"hooks": [{"command": "…/amb hook turn",
                                      "timeout": 5, "type": "command"}]}]}, …}
```

**Output — the response envelope (F3).** `delivery::envelope` always emits
`hookSpecificOutput.additionalContext`. Its docstring justifies the absence of a vendor branch by
counting the spelling in both shipped bundles — which is honest, and is exactly the reasoning that
stops holding at vendor three.

Both are invisible while every shipped vendor agrees, which is precisely the blind spot
`tool_matcher` had before Gemini arrived (D111's third bullet). **The gap is not that `Vendor` is
wrong; it is that two of its dimensions were never needed yet.**

## Three categories, not two

### A · Same schema family — a manifest, today

Claude Code and Gemini CLI, both shipped. **Verified (F4)** that Gemini's documented format matches
what `amb` writes: a `hooks` object, per-event arrays, `type: "command"` as the only execution
engine, a `matcher` field, `timeout`, JSON on stdin and stdout.

Noted and not acted on: Gemini's matcher is a *regex* for tool events and an *exact string* for
lifecycle events. `amb` writes no matcher on lifecycle events, so the distinction does not bite.

### B · Subprocess hooks, different or unverified schema

**Cursor — verified different (F5).** `hooks.json` is flatter: event → array of handlers directly,
`matcher` on the handler itself, plus a **required top-level `version: 1`**. Writing Claude's shape
would add a nesting level and omit `version`. Event names are camelCase (`sessionStart`,
`postToolUse`, `stop`) — but that half is already expressible, because `Vendor::events` exists.
**Only the schema and the `version` key are the gap.**

**OpenAI Codex CLI — unverified (F6), and the process failure is recorded (F7).** The first pass of
this research told the user Codex "is not a manifest away." Its documentation actually describes
`hooks.<Event> = [ { hooks = [ {handler} ] } ]` — the *same* nested matcher-group shape, with
`type: command` and PascalCase events close to Claude's. So the original claim was probably wrong.

**Neither claim is admissible.** D111 is explicit that vendor values are read out of the shipped
binary and not its documentation, because Gemini 0.55.1 contained no occurrence of `PreToolUse` at
all while its docs suggested a mapping. Codex is not installed here; `~/.codex` exists but its
`config.toml` is 14 bytes dated February 2026, and hooks shipped in v0.114 in March. Its hooks are
also behind a `features.hooks` gate, which is enablement `Vendor` has no field for.

### C · No settings file at all — Google ADK

ADK is a framework whose extension points are in-process Python [Plugins and
callbacks](https://adk.dev/plugins/), registered as `Runner(plugins=[…])`. There is no file for
`amb install` to write, no event vocabulary to spell, and no session-id environment variable.

**And `amb` already serves it (F8).** Verified against the real binary — no
`CLAUDE_CODE_SESSION_ID`, no `GEMINI_SESSION_ID`, no settings file, identity only in the payload:

```console
$ printf '%s' '{"hook_event_name":"SessionStart","session_id":"adk-sess-001","cwd":"/tmp/myapp"}' \
    | env -u CLAUDE_CODE_SESSION_ID AMB_PROJECT=myapp amb hook turn
  delivered:  #1 [broadcast] from "myapp-peer"
  registered: adk-sess-001 | myapp-adk-se | myapp
```

D113 and D114 built that path for payload-only *CLIs*; it happens to be exactly what a *framework*
needs. **The integration inverts** — `amb` does not install itself into ADK; an ADK plugin calls
`amb hook turn`. A small Python file, and no change to `amb`.

Every lane has an analogue **(F9)**:

| `amb` lane | ADK callback |
|---|---|
| `SessionStart` | `before_run_callback` |
| turn end (`Stop`) | `after_run_callback` |
| `PostToolUse` → claims | tool callbacks — receive `tool` **and `tool_args`** |
| `PostToolUseFailure` → capture | `on_tool_error_callback` |
| `additionalContext` injection | `before_model_callback`, modifying `llm_request` |

Three design notes, all **unverified against a running ADK**:

- **`edit_tools` is softer than it looks (F10).** That field is a *closed* tool vocabulary, which
  holds for a CLI and cannot for a framework whose tools are user-defined functions. But ADK hands
  the callback `tool_args` directly, so a plugin can read the file path rather than infer it from a
  tool name — sidestepping the assumption instead of hitting it.
- **Identity must be `session.id` (F11).** `AgentTool` can create a new `InvocationContext`, so
  keying on the invocation would fragment one session into many agents on the board — the failure
  D12 exists to prevent.
- **D9 becomes the plugin's job.** `hook_main` guarantees exit 0 whatever happens, so mail delivery
  can never break a session. A plugin has no exit code; the equivalent is that **every callback must
  swallow its own exceptions**. An ADK adapter that lets a `subprocess` error propagate would break
  the host agent's turn — the one thing D9 forbids — and nothing in `amb` can enforce that from the
  other side of the process boundary.

## There is no standard to target, and that is a finding rather than a gap

**F12.** The obvious modern answer — target an emerging cross-vendor standard instead of N
schemas — does not exist. [Agent Plugins 1.0](https://blakecrosley.com/blog/agent-plugins-standard)
(Vercel with Amazon, Cursor, GitHub, Microsoft, OpenAI; Google at launch) standardises the
**packaging layer only** and explicitly excludes *hook behavior*, slash commands, rules and
installation. **Anthropic is absent from the maintainer roster** and keeps its own plugin format.

The wider survey agrees on the shape of the problem: hooks converged *behaviourally* — session
start, prompt submit, tool use, session end, JSON on stdin, JSON on stdout — while [their interfaces
did not](https://www.speakeasy.com/resources/ai-agent-hooks).

So schema divergence is not a transitional state to wait out. It is fenced off as client-specific
**by design**, which means a shape abstraction inside `amb` is the only route for category B, and no
upstream will make it unnecessary.

## Recommendation

**1 · Build the ADK plugin first, and nothing else.** *Confidence: high.* It needs no change to
`amb` (F8, verified), it is the host that was actually asked for, and it is the only way to test the
payload contract against a genuinely non-CLI caller. Evidence before design — D49's second paragraph
records what this project thinks of the other ordering.

**2 · Do not add a settings-shape abstraction yet.** *Confidence: high.* Only one host is confirmed
different (F5), and one consumer makes it a speculative field. D111's ordering argument is explicit:
the manifest format was designed *after* the second vendor, "because `find_unread_fields.py` is in
the gate and a speculative field is a field nothing reads." That is why there is no unused config
language in this codebase.

**3 · Read the shipped binary before writing any Codex or Cursor manifest.** *Confidence: high, and
this one is a prerequisite rather than a step.* Neither is installed here. F7 is the cost of
skipping it, paid once already in this very document's first draft.

**4 · If a second differing schema is confirmed, the abstraction is two fields, not one.** *Confidence: moderate.* Input schema (F2) **and** output envelope (F3) both hardcode Claude. A design that fixes only the settings writer would leave every response still shaped for Claude, and the failure would be silent in the way F3's own docstring predicts.

## Two design questions left open

Neither is answerable from research; both want the user.

1. **Where does the plugin live** — `contrib/adk/` here, or a separate package? Python in a Rust
   tree. D11 governs what `amb` writes into *other* repositories and says nothing about shipping an
   adapter in its own.
2. **Plugin API or per-agent callbacks?** `Runner(plugins=[…])` is one registration for a whole app;
   per-agent callbacks are finer but attach to each agent. Plugin callbacks take precedence over
   object-level ones, which matters if a host app already uses both.

## What would reopen this

A second host whose settings schema is confirmed different **from a shipped binary rather than from
documentation** — the standard D111 set after Gemini 0.55.1 turned out to contain no occurrence of
`PreToolUse` at all. Cursor is today's candidate and is unconfirmed at that standard.

## Sources

- [Cursor — Hooks](https://cursor.com/docs/hooks) — `hooks.json` schema, event names, config locations
- [OpenAI Codex — Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) — hook events, handler fields, `features.hooks`
- [Google ADK — Plugins](https://adk.dev/plugins/) — `BasePlugin`, callback signatures, `Runner` registration
- [Google ADK — Types of callbacks](https://google.github.io/adk-docs/callbacks/types-of-callbacks/) — per-callback contracts
- [Gemini CLI — Hooks reference](https://geminicli.com/docs/hooks/reference/) — settings.json hook format
- [Agent Plugins 1.0](https://blakecrosley.com/blog/agent-plugins-standard) — scope, maintainers, exclusions
- [AI agent hooks — Speakeasy](https://www.speakeasy.com/resources/ai-agent-hooks) — cross-vendor interface divergence

In-repository: D9 (hooks exit 0), D11 (never writes into a repository unprompted), D12 (identity is
the session id), D11 (the fourteen-tool survey, and the no-hook-mechanism condition), D111 (a vendor is data), D113 and D114
(payload-driven identity and vendor).
