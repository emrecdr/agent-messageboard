# Hosts beyond Claude Code — what `amb` can already serve, and what it cannot

> **Status: research complete, implementation deferred, 2026-09-06.** Nothing here has been built.
> The deferral is recorded as `OPEN-QUESTIONS.md` Q15, which owns the decision; this file owns the
> evidence and is where a revisit should start. Every claim below is marked **verified** or
> **unverified**, and the unverified ones say what verification requires.
>
> **The one thing to take away if you read no further:** `amb` already serves a host with no
> settings file, no hook system and no session-id environment variable — verified against the real
> binary. D113 and D114 built that path for payload-only *CLIs*, and it turns out to be exactly what
> a *framework* needs. The expensive-looking half of this question is already done.

Raised by the user: *"we are not just aiming claude code anymore, our tool also should be compatible
with other llm providers and also with google adk."*

## The load-bearing assumption

D111 made a vendor **data** rather than a trait: `Vendor` carries paths, event spellings, tool
vocabulary and session variables, and `~/.config/amb/vendors/*.json` adds one with no rebuild.

What `Vendor` does **not** carry is the *shape of the settings file*. `hooks::plan_install`
hardcodes Claude's schema — a root `hooks` object, `"type": "command"`, and the nested
`{matcher, hooks:[…]}` structure. **Verified** by reading the function and by installing against a
throwaway `HOME`:

```console
$ HOME=/tmp/fake amb install --vendor gemini-cli
$ cat /tmp/fake/.gemini/settings.json
{"hooks": {"AfterAgent": [{"hooks": [{"command": "…/amb hook turn",
                                      "timeout": 5, "type": "command"}]}]}, …}
```

That is Claude's shape, written into Gemini's file. It works because Gemini genuinely shares it —
which is the point, and also the risk: **the gap is invisible while every shipped vendor agrees on
the shape.** This is precisely the blind spot `tool_matcher` had before Gemini arrived, recorded in
D111's third bullet.

## Three categories, not two

### A · Same schema family — a manifest, today

| host | status |
|---|---|
| Claude Code | shipped |
| Gemini CLI | shipped |

**Verified** that Gemini's documented format matches what `amb` writes: a `hooks` object, per-event
arrays, `type: "command"` as the only execution engine, a `matcher` field, `timeout`, and JSON on
stdin/stdout. Nothing to do.

One detail noted and not acted on: Gemini's matcher is a *regex* for tool events and an *exact
string* for lifecycle events. `amb` writes no matcher on lifecycle events, so the distinction does
not currently bite.

### B · Subprocess hooks, different or unverified schema

**Cursor — verified different.** [`hooks.json`](https://cursor.com/docs/hooks) is *flatter* than
Claude's: event → array of handlers directly, with `matcher` on the handler itself, plus a
**required top-level `version: 1`**. Writing Claude's shape there would add a nesting level and omit
`version`. Event names are camelCase (`sessionStart`, `postToolUse`, `stop`) — but that half is
already expressible, because `Vendor::events` exists. **Only the shape and the `version` key are the
gap.**

**OpenAI Codex CLI — unverified, and an earlier claim in this repository's own history was wrong.**
The first pass of this research asserted Codex "is not a manifest away." The documentation actually
describes `hooks.<Event> = [ { hooks = [ {handler} ] } ]` — the *same* nested matcher-group shape,
with `type: command` and PascalCase events (`SessionStart`, `PostToolUse`, `SessionEnd`, `Stop`)
close to Claude's vocabulary. So the earlier claim was probably wrong.

**Neither claim is admissible.** D111 is explicit that vendor values are read out of the shipped
binary and not its documentation, because Gemini 0.55.1 contained no occurrence of `PreToolUse` at
all while the docs suggested a mapping. Codex is not installed on this machine: `~/.codex` exists
but its `config.toml` is 14 bytes dated February 2026, and hooks shipped in v0.114 in March.
**Verification requires the binary or the source, and nothing less.** Its hooks are also behind a
`features.hooks` gate, which is enablement `Vendor` has no field for.

### C · No settings file at all — Google ADK

ADK is not a CLI with a hook system; it is a framework whose extension points are in-process Python
[Plugins and callbacks](https://adk.dev/plugins/). There is no file for `amb install` to write, no
event vocabulary to spell, and no session-id environment variable.

**And `amb` already serves it.** **Verified** against the real binary — no `CLAUDE_CODE_SESSION_ID`,
no `GEMINI_SESSION_ID`, no settings file, identity carried only in the payload:

```console
$ printf '%s' '{"hook_event_name":"SessionStart","session_id":"adk-sess-001","cwd":"/tmp/myapp"}' \
    | env -u CLAUDE_CODE_SESSION_ID AMB_PROJECT=myapp amb hook turn
  delivered: #1 [broadcast] from "myapp-peer"
  registered: adk-sess-001 | myapp-adk-se | myapp
```

**The integration inverts.** `amb` does not install itself into ADK; an ADK plugin calls
`amb hook turn`. That is a small Python file and no change to `amb`.

Every lane has an analogue:

| `amb` lane | ADK callback |
|---|---|
| `SessionStart` | `before_run_callback` |
| turn end (`Stop`) | `after_run_callback` |
| `PostToolUse` → claims | tool callbacks — receive `tool` **and `tool_args`** |
| `PostToolUseFailure` → capture | `on_tool_error_callback` |
| `additionalContext` injection | `before_model_callback`, modifying `llm_request` |

Two findings that change the design, both **unverified against a running ADK** and both worth
checking first:

- **The `edit_tools` problem is softer than it first appears.** That field is a *closed* tool
  vocabulary, which holds for a CLI and cannot hold for a framework where tools are user-defined
  functions. But ADK hands the callback `tool_args` directly, so a plugin can read the file path
  rather than infer it from a tool name — sidestepping the assumption instead of hitting it.
- **Identity must be `session.id`, not `invocation_id`.** ADK discussions note that `AgentTool` can
  create a *new* `InvocationContext`, so keying on the invocation would fragment one session into
  many agents on the board — the failure D12 exists to prevent.

## There is no standard to target, and that is a finding rather than a gap

The obvious modern answer — target the emerging cross-vendor standard instead of N schemas — does
not exist. [Agent Plugins 1.0](https://blakecrosley.com/blog/agent-plugins-standard) (Vercel with
Amazon, Cursor, GitHub, Microsoft, OpenAI; Google at launch) standardises the **packaging layer
only** and explicitly excludes *hook behavior*, slash commands, rules and installation. **Anthropic
is absent from the maintainer roster** and keeps its own plugin format.

The wider survey agrees on the shape of the problem: hooks converged *behaviourally* across
vendors — session start, prompt submit, tool use, session end, JSON on stdin, JSON on stdout — while
[their interfaces did not](https://www.speakeasy.com/resources/ai-agent-hooks). Every provider ships
its own config format, event vocabulary and response shape.

So the schema divergence is not a transitional state to wait out. It is fenced off as
client-specific **by design**, which means a shape abstraction inside `amb` is the only route for
category B, and there will be no upstream that makes it unnecessary.

## Recommendation, and why it is deferred rather than staged

**Build the ADK plugin first, and nothing else.** It needs no change to `amb`, it is the host that
was actually asked for, and it is the only way to test the payload contract against a genuinely
non-CLI caller. Evidence before design — the ordering D49's second paragraph exists to record.

**Do not build a schema abstraction yet.** One confirmed-different schema is not enough to design
against. D111's own ordering argument is that the manifest format was designed *after* the second
vendor, not before, "because `find_unread_fields.py` is in the gate and a speculative field is a
field nothing reads" — and that is why there is no unused config language in this codebase. A
`settings_shape` field with one real consumer would be exactly that.

**Read the shipped binary before writing any Codex or Cursor manifest.** Neither is installed here.
This is a prerequisite, not a step.

## D11 does not fire, and it was checked

D11's withdrawal condition is *"a target agent tool with no hook mechanism at all"* — attached to
D11 because such a target would force `amb` to write a file into a repository for the agent to read.
ADK has an extension mechanism; it is in-process rather than subprocess. **The condition does not
fire and D11 stands.** Recorded because the condition names Aider as qualifying-but-unasked, and ADK
is the first host anyone has actually asked for that might have looked like it qualified.

## Two design questions left open

Neither is answerable from research and both want the user:

1. **Where does the plugin live** — `contrib/adk/` in this repository, or a separate package? It is
   Python in a Rust tree. D11 governs what `amb` writes into *other* repositories and says nothing
   about shipping an adapter in its own.
2. **Plugin API or per-agent callbacks?** `Runner(plugins=[…])` is one registration for a whole app;
   per-agent callbacks are finer but must be attached to each agent. Plugin callbacks take
   precedence over object-level ones, which matters if a host app already uses both.

## What would reopen this

A second host whose settings schema is confirmed different *from a binary rather than from docs*.
That is the point at which a shape abstraction has two real consumers and stops being speculative.
Cursor is one candidate today and is unconfirmed at that standard.
