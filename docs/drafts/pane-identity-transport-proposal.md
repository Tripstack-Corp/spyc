# Pane identity for MCP tool dispatch — design proposal (C7)

**Status:** proposal. No code written.
**Measured against:** `6e087b3`.

## What's missing and why it matters

The F1 decisions-log entry names per-pane root validation as the target design:
check an MCP `root` override against *the calling pane's own worktree* rather
than a session-wide allowed set. It can't be built today because **read-tool
dispatch has no idea which pane is calling**.

Three facts define the gap:

- `SPYC_PANE_ID` reaches the agent pane's environment at spawn
  (`app/pane_tabs.rs`), and the `spyc --mcp` proxy is re-exec'd *by the agent*,
  so the proxy already inherits it.
- The proxy forwards JSONL **verbatim** (`mcp::run` → `run_proxy`). It reads the
  env only to find the socket; it adds nothing to the stream.
- Read tools resolve context from the `.spyc-context-<pid>.json` file, which
  carries `cwd` / `project_home` / `search_root` and **no pane identity**.

`report_status` looks like a counter-example but isn't: it takes `pane_id` as a
*tool argument*, supplied by the status hook from `$SPYC_PANE_ID`. That's
agent-asserted data travelling in the payload, not identity derived from the
transport.

The cost of the gap is visible in F1's own shape. Its **cursor-independence
invariant** — the allowed set must never reject the agent's own working root —
exists *only* because there is nothing to attribute a call to. With attribution
the rule collapses to "the pane's own worktree", which is both tighter and
obvious.

## Candidates

### A. One socket per pane

spyc listens on `mcp-<pid>-<paneid>.sock` per agent pane; each pane's
`SPYC_MCP_SOCK` points at its own.

Identity becomes structural: whichever socket accepted the connection *is* the
answer, with nothing to assert.

- **Cost:** high. N listeners per instance, lifecycle bound to tab open/close,
  and the orphan-sweep / takeover / trusted-root-sidecar logic all multiply by
  pane. The socket-per-PID model is load-bearing in `mcp/config.rs`'s takeover
  detection and `server.rs`'s discovery.
- **Migration:** breaking. `SPYC_MCP_SOCK` changes meaning; existing `.mcp.json`
  / `.codex/config.toml` files point at the instance socket and must keep
  working (accepted as "unattributed") through at least one release.

### B. Pane id in the `initialize` handshake

The proxy reads `$SPYC_PANE_ID` from its own environment and sends it in
`initialize` (`clientInfo` or `_meta`); the server binds it to that connection
for the connection's lifetime.

- **Cost:** low. One handshake field, one per-connection field server-side, zero
  change to tool schemas or to what agents must remember.
- **Migration:** trivial and non-breaking. An older proxy omits the field; the
  server treats that connection as unattributed and behaves exactly as today.

### C. Pane id on every tool call

Generalize the `report_status` pattern: add an optional `pane_id` to every tool.

- **Cost:** every schema grows a parameter, and correctness depends on the agent
  *remembering* to send it. A forgetful agent silently loses attribution, which
  is the worst failure mode of the three — it degrades quietly.
- **Migration:** trivial, but it makes attribution a per-call agent
  responsibility forever.

## The trust question, stated plainly

The brief asks what an env-supplied pane id does to the trust model. The honest
answer is broader than that, and it changes the recommendation:

**No pane-attribution mechanism can be an authorization boundary against a
same-user agent — including option A.**

- B and C are forgeable directly: the agent controls its own environment, so it
  can set `SPYC_PANE_ID` to a sibling's value before the proxy starts.
- A is forgeable too, just less obviously: the sockets live in a directory the
  agent can list, and they are 0600 owned by *the same user the agent runs as*.
  Nothing stops it connecting to another pane's socket.

This is the same conclusion SECURITY.md already reaches about the MCP surface,
and it is worth not quietly forgetting when per-pane roots make the boundary
*look* stronger. F1's threat model is the harness permission asymmetry — MCP
auto-approved while shell execution is gated — and a prompt-injected agent under
that model is exactly the actor who would forge an id. Per-pane roots built on
any of these three would stop an *accident*, not an *attempt*.

Real containment needs OS-level isolation (a separate uid, a container). That's
outside spyc's scope, and pretending otherwise in a doc is how F1's original
"the check is decorative" problem happened in the first place.

So the mechanism should be chosen for **cost and correctness**, not for
strength — because none of them buys strength.

## What attribution unlocks

1. **Per-pane roots (F1's target).** Validate `root` against the calling pane's
   own worktree. Retires the cursor-independence workaround.
2. **`get_spyc_context` answers for the caller.** Today it returns the focused
   column's cwd, so an agent in worktree X gets told about worktree Y whenever
   the user browses. That's the single most confusing thing about the current
   tool surface.
3. **Scope-registry ownership (P2).** `register_scope` claims are advisory and
   owner-labelled by convention. Attribution lets a claim bind to a pane, and
   `release_scope` refuse a claim the caller doesn't own.
4. **Audit.** "Which pane read that file" is currently unanswerable. One
   connection-scoped field makes the existing `mcp_log` useful for it.

(2) is the one users would notice tomorrow; (1) is the one F1 asked for.

## Recommendation

**Option B — pane id in the `initialize` handshake.**

It is the cheapest by a wide margin, non-breaking, requires nothing of agents,
and unlocks all four capabilities above. Since no option provides authorization,
paying option A's cost — multiplying the socket lifecycle, takeover detection,
and orphan sweeping by pane count — buys ergonomics only, and buys them worse
than B does.

Reject C: making every call carry its own identity means attribution fails
silently whenever an agent forgets, and it spreads the concern across every
schema instead of confining it to one handshake.

**Conditions on B, if it is built:**

- Bind the id to the *connection*, never re-read it per call. A per-call field
  reintroduces C's failure mode.
- Validate the id against live tabs on receipt and drop it if unknown, so a
  stale id from a closed tab degrades to unattributed rather than mis-attributing.
- Keep every unattributed path working. The session-wide allowed set from F1
  stays as the fallback — per-pane roots *narrow* it, and must never be the only
  thing standing between an agent and its own worktree.
- Say in SECURITY.md that this is attribution, not authorization, in the same
  paragraph that describes what it enables.

## Not recommended

Threading identity through the context file. It is PID-scoped, one per spyc
instance, and rewritten on every state change — making it per-pane would either
multiply the files or add a pane table that every reader has to index, for no
gain over a handshake field.
