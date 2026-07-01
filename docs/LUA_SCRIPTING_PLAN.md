# Lua Scripting Plan (mlua)

> **Status: PROPOSED — awaiting review (2026-06-30). Not started.**
> Charter for embedding Lua scripting in spyc. Scope, build-dep, and security
> decisions are locked (see *Context*); the execution model and PR sequence
> below are the proposal under review.

## Context

spyc's only extensibility today is the declarative `.spycrc.toml` DSL (`map KEY` →
a built-in `Action`, a `unix` shell template, `patternpick`/`jump`, or a `:`
command). It can't branch, loop, read state to decide, define new commands, or
react to events. The ask: embed real Lua scripting so a key/command can run
*logic*, and (phase 2) so spyc can run user code *in reaction to events* (an
agent pane going `blocked`, a dir change) — the natural capstone of the
agent-awareness work.

**Locked decisions:**
- **Scope: Tier A + B now; Tier C (event hooks) designed-for but deferred to a phase-2.**
  - A: programmable keybindings — `map KEY lua <name>`.
  - B: a `~/.config/spyc/init.lua` config platform — `spyc.map` / `spyc.command` + a `spyc.*` API.
  - C (design seam only, NOT wired): `spyc.on('startup'|'dir_changed'|'project_changed'|'agent_status', fn)`.
- **Build dep accepted unconditionally:** `mlua` with `lua54` + `vendored`, **no cargo feature gate**. A C compiler becomes a hard build requirement everywhere (incl. musl static). The "zero C deps / clean static musl" comments in `Cargo.toml` get honestly rewritten.
- **Security: `$HOME`/XDG only.** Lua loads only from `~/.config/spyc/init.lua` and `~/.config/spyc/lua/`. Project-local config can NEVER run Lua — enforced for free by marking `BoundAction::Lua` as `is_executing()` (the existing `Trust::Project` gate drops executing bindings).

## Execution model — the heart of the design

**Lua runs on a single, persistent worker thread with a serial FIFO job queue**
— NOT a thread per script. This is mandatory, not incidental: the interactive
runaway prompt is impossible if the main loop is blocked inside `lua.call()`.
Off-thread keeps the UI responsive (renders, input, pane dots all keep going)
while a script runs, which is the *only* way to offer "still running — keep
waiting? [y/N]".

It also reuses spyc's two existing async idioms:
1. **Worker-result idiom** (graveyard/mermaid/file ops): Effect → detached thread → `Arc<Mutex>` landing slot → payloadless `Message` wake → pre-recv drain.
2. **Request/reply idiom** (the MCP socket server, `src/mcp_cmd.rs`): an off-thread client sends `McpRequest { command, reply: mpsc::Sender<McpResponse> }`, blocks for the reply, and the main loop executes on its own thread via `execute_mcp_command`. **The Lua worker is, internally, exactly another MCP-style client** — so its mutation API *reuses `execute_mcp_command` verbatim* where it overlaps.

### Job lifecycle (per Lua invocation)
1. A trigger (a `BoundAction::Lua`, a `spyc.map` key, a Lua `:`-command, or — phase 2 — an event) hands the main loop a job: `{ fn_id, context_snapshot }`.
2. Main records `lua_job: { id, name, started_at }`, sends the job to the worker over `lua_job_tx`, and returns immediately (`Vec<Effect>` empty for now). Main keeps looping.
3. Worker looks up the stored `Function` by `fn_id` (a `RegistryKey` held worker-side), builds the `spyc` context table from the snapshot, installs the instruction hook, and `call()`s it.
4. Inside the call, the script's `spyc.*` functions either (a) read the snapshot locally (cheap context: cwd/cursor/picks/filter/git_branch/project_home/session) or (b) **round-trip** a request to main (mutations, and heavy reads like worktree-list/git/file-content) via a reply channel, reusing `execute_mcp_command`. Each round-trip is a quick `Message` the main loop handles between renders — so the loop never blocks and stays interruptible.
5. On return/error, the worker sends `Message::LuaDone { id, result }`; main clears `lua_job`, flashes any error, and applies any registration deltas.

**`mlua::Lua` never crosses threads** — created on the worker, all
`Function`/`RegistryKey` use is worker-local; main holds only channel endpoints
+ an `Arc<AtomicBool>` abort flag. So the `send` feature is **not** needed.

### Why not on-thread?
On-thread is simpler (a synchronous `Rc<RefCell>` bridge), but a slow/infinite
script freezes the whole UI until an instruction-budget hard-kill fires — no
interactive choice, and a generous budget means a long freeze before the kill.
Off-thread is the price of the control surface we want, and it's the idiomatic
spyc shape anyway.

## Kill switches & loop protection (the full set)

1. **Instruction-count hook (core watchdog).** The worker installs `lua.set_hook` with an every-Nth-instruction trigger (e.g. every 100k). The hook returns `Err(..)` — which unwinds the running `call()` — when **either**:
   - the abort flag (`Arc<AtomicBool>`, set by main on a user "kill") is true, or
   - an absolute wall-clock/instruction **hard ceiling** is exceeded (e.g. 30s — a backstop against held-down `y`).
   A pure-Lua `while true do end` executes instructions, so it *is* caught and killable.
2. **Soft-threshold → interactive prompt.** Main's scheduler (`src/app/scheduler.rs`, which already manages deadlines) arms a wakeup `soft_ms` after a job starts (e.g. 750ms). If the job is still in-flight, raise a modal confirm: **"Lua script `<name>` has run for Ns — keep waiting? [y/N]"**.
   - `N`/`Esc` → set the abort flag → hook unwinds → flash "`<name>` aborted".
   - `y` → reset the soft timer (re-arm), keep running.
   This prompt works *because* main isn't blocked.
3. **Single-worker serialization + busy guard.** One job at a time. A new trigger while a job runs is rejected with a flash ("Lua busy — `<name>` running") rather than queuing unboundedly (prevents a script-fork-bomb). (A small bounded queue is a possible later refinement.)
4. **Error containment.** Every `mlua::Error` (syntax, runtime, abort) is caught at the worker boundary, logged to `~/.../lua.log`, and surfaced via `flash_error` on main. **Never panics, never unwraps.** A broken `init.lua` flashes an error and leaves spyc fully usable with Lua inert — exactly like a broken `.spycrc.toml` today.
5. **Startup safety.** `init.lua` is loaded *as a job through the same path*, so an infinite loop at load is subject to the same watchdog + prompt — a bad config can't brick startup.
6. **Blocking-C caveat + stdlib policy.** The instruction hook cannot interrupt a script blocked *inside* a C function (e.g. `io.read` on stdin, a blocking `os.execute`). Mitigation: **do not expose blocking primitives** — strip `io.read`/stdin and steer users to spyc's own async, cancelable `spyc.cmd(':!...')` / `unix` round-trips instead of `os.execute`. Decision to confirm at build: load the standard libs but remove the stdin/blocking surface; everything that can block goes through a cancelable spyc round-trip. Document the residual caveat.
7. **Global off switch.** `:lua off` (and a `--no-lua` flag) disables the worker entirely — a panic button if a user's own config wedges things. `:lua reload` re-runs `init.lua`; `:lua status` shows worker/job state.
8. **Idle stays 0 dps.** The watchdog only arms a scheduler deadline *while a job is in flight*; idle wakeups are unchanged.

## Module layout (all new code in dedicated modules; nothing into `app/mod.rs`)

- `src/lua/mod.rs` — module root: the `LuaWorker` handle (channels + abort flag held in `Runtime`), `LuaJob`, `LuaResult`, `fn_id` types, public `spawn_worker()` / `submit()` / `request_abort()`.
- `src/lua/worker.rs` — the worker thread body: owns `mlua::Lua`, the `fn_id → RegistryKey` map, the job loop, hook installation, error capture.
- `src/lua/api.rs` — registers the `spyc` global table (the API surface below); pure-ish setup, all closures capture the round-trip `Sender` + snapshot `Rc<RefCell>`.
- `src/lua/bridge.rs` — `LuaRequest` enum + the round-trip reply plumbing + snapshot→Lua-table and Lua-value→request conversions; the pure `lua_request_to_effects()` translation (unit-tested).
- `src/lua/registry.rs` — the runtime registries main consults: `key → fn_id` (Lua maps) and `command_name → fn_id` (Lua `:`-commands); plus (phase-2 seam) `event → Vec<fn_id>`.
- `src/app/lua.rs` — the App-layer handlers: `apply_lua_binding`, `dispatch_lua_command` (the COMMAND_TABLE fallthrough), `handle_lua_done`, `handle_lua_request` (executes a round-trip via `execute_mcp_command` + Lua-only commands), the watchdog deadline check, the `:lua` subcommands. Reads `App` private fields per the descendant-module rule.

`Runtime` (`src/app/mod.rs:~442`) gains one field: `lua: Option<LuaWorker>`
(handle/channels/abort flag — an OS-ish resource, correctly in Runtime, never in
the Model). The non-`Send` `mlua::Lua` lives ONLY on the worker thread; gate it
with a `// SPYC-TRAP(lua-handle-thread-local)` anchor + an ARCHITECTURE.md
rationale (its failure — moving a handle to main — is a silent thread-safety
hazard). Add `src/lua` to AGENTS.md's module index
(`every_app_module_is_in_the_agents_index` guard) and `src/app/lua.rs` to the app
index. All new logic lives in these modules — `app/mod.rs` is ceiling-guarded at
1500 (`mod_rs_stays_decomposed`), so nothing lands there.

## Lua API surface (Tier A + B)

A single `spyc` global table.

**Reads (from the cheap snapshot — no round-trip):**
`spyc.context()` → table {cwd, cursor_file, picks, inventory, filter, git_branch, project_home, session_name, version}; plus sugar `spyc.cwd()`, `spyc.cursor()`, `spyc.picks()`, `spyc.git_branch()`. Backed by `SpycContext` (`src/context.rs`, already `Clone`/`Serialize`).

**Reads (round-trip, reuse MCP read logic — phase 4):**
`spyc.worktrees()`, `spyc.git_status()`, `spyc.git_log{...}`, `spyc.read(path)`, `spyc.search_paths(q)`, `spyc.search_content(re)`.

**Mutations / actions (round-trip; reuse `McpCommand`/`execute_mcp_command` where overlapping):**
- `spyc.navigate(path)`, `spyc.pick(globs)`, `spyc.clear_picks()`, `spyc.filter(glob)`, `spyc.report_status(s)` → existing `McpCommand` variants.
- `spyc.action(name, count?)` → run **any existing `Action` by its canonical snake_case name** (the full vocabulary, via `keymap::action::{canonical_name,action_from_name}` — an exhaustive name↔`Action` layer whose completeness a `strum::EnumIter` round-trip guard test enforces; `run_lua_action` falls back to the `.spycrc` DSL verb table in `src/config/dsl.rs::parse_action` only for aliases like `enter`/`nextfile`). `set_mark`/`jump_mark` are excluded (no default mark letter). New `LuaRequest::RunAction`.
- `spyc.cmd(line)` → run a `:` command line (e.g. `:grep foo`) — reuses the `:`-dispatch. New `LuaRequest::RunColon`.
- `spyc.notify(msg)` / `spyc.warn(msg)` → flash line.

**Registration (Tier B, evaluated at `init.lua` load):**
- `spyc.map(key, fn)` → registers `key → fn_id` (key string parsed by the existing `dsl::parse_key`).
- `spyc.command(name, fn)` → registers a runtime `:`-command.
- `spyc.on(event, fn)` → **stub that records the registration but is inert in phase 1** (the Tier-C seam; documented as "coming in phase 2" — but per the comment-rot guard, the code comment states only what IS: "event hooks are recorded but not yet dispatched").

## Dispatch paths (all stay MVU-pure: trigger → job → requests-as-data → effects)

- **(A) `BoundAction::Lua(name)` from a key:** add the variant to `src/keymap/user.rs`; mark `is_executing()`; add the `lua` verb to `src/config/dsl.rs::parse_action` (`tail` is the script name). Resolver already yields `ResolverOutcome::User(BoundAction)`. `App::apply_user` lives at **`src/app/key_dispatch/mod.rs:606`** (where `UnixCmd`→`sh_c` and `Command`→`self.dispatch_command`) — that's where the `BoundAction::Lua(name)` arm goes → `self.apply_lua_binding(name)` (`src/app/lua.rs`) → look up `fn_id` (named scripts in `<config_root>/lua/<name>.lua`, or a function registered under that name) → submit job.
- **(B) Lua-registered `spyc.map` key:** adopt the **synthetic-binding trick** — `spyc.map(key, fn)` stores the fn at `fn_id` and appends a `BoundAction::Lua("@map:<idx>")` (the `@map:` sentinel names the registered fn, not a file) to the live `UserKeymap`. This needs **zero resolver changes** and inherits the existing "later binding wins" + the trust gate for free.
- **(C) Lua-registered `:`-command:** the pure half `AppState::dispatch_command` (`state/dispatch.rs`) returns `NotHandled` for any non-`COMMAND_TABLE` name, so a Lua command name reaches the App layer. The App-layer fallthrough is at **`src/app/commands.rs:135-144`** (table `lookup` → else "unknown command" flash): insert a check of `registry::command_lookup(name)` **between** the table miss and the flash → submit job. `COMMAND_TABLE` is static + guard-tested — **untouched**, so `command_table_*` stays green. Tab-completion (`completion_command_names`) gets the Lua names appended at the call site, not in the table.

## init.lua loader + live reload
- **Load:** in `src/app/bootstrap.rs`, after `Runtime` is built, if `<config_root>/init.lua` exists, spawn the worker and submit a "load" job (worker `Lua::new()` → `api::setup()` → `dofile(init.lua)`; registration calls accumulate and return as a delta applied to the registries on main).
- **XDG config dir (lands first, as PR0):** there is no config-dir helper today (`src/paths.rs` is only `~`/`$VAR` expansion; the only XDG resolver is `state_root()` at `src/state/mod.rs:53` = `$XDG_STATE_HOME/spyc` else `~/.local/state/spyc`, with a thread-local override for parallel-safe tests). Add a `config_root()` resolver = `$XDG_CONFIG_HOME/spyc` else `~/.config/spyc`, mirroring `state_root()`'s shape **including its thread-local test override** so VM/config tests stay chdir-free. Lowest-risk home is alongside `state_root()` in `src/state/mod.rs` (reuses the override machinery); a `paths::` home is the cleaner long-term choice if the override is factored out — decide at implementation. `init.lua` and `<config_root>/lua/` are the ONLY load locations. This helper is dependency-free and useful on its own, so it ships **before** any mlua work.
- **Reload:** extend `src/app/config.rs::reload_config` (already triggered by `^R` + the file watcher) to submit a worker "reload" job: drop the old `Lua` (frees all `RegistryKey`s), recreate, re-run `init.lua`, replace the registries atomically. Add `init.lua` + `~/.config/spyc/lua/` to the config watcher's tracked set (`src/app/watch.rs`).

## Tier-C event seam (DESIGN ONLY — do not wire dispatch in phase 1)
- `registry.rs` already carries `event → Vec<fn_id>` (populated by the inert `spyc.on`).
- Phase 2 adds: an `Event` enum (`Startup`, `DirChanged{cwd}`, `ProjectChanged{root}`, `AgentStatus{pane_id,state}`), a main-side `fire_event(ev)` that snapshots context + submits a job per registered `fn_id`, and **a handful of named dispatch sites**: startup (end of `bootstrap`), dir-change (`src/app/navigate.rs` / where cwd changes), project-change (PROJECT_HOME resolve), agent-status (`src/app/agent_status.rs` where `reported`/activity changes). **Low-frequency events only first**; explicitly exclude `cursor_moved`/`pane_output` until debounced — they fire at key-repeat/firehose speed and would blow the repaint budget. The serialization + watchdog already protect against a slow event handler.

## Cargo.toml / build / deny
- Add: `mlua = { version = "0.10", features = ["lua54", "vendored"] }` (pin the current release at implementation).
- **Honestly rewrite** the `Cargo.toml` header comments that advertise "Pure-Rust … no C, no *-sys, so static musl builds stay clean" / "All pure-Rust, zero C deps" — they're no longer true; note Lua 5.4 is vendored C and a C compiler is now a build requirement.
- Check `deny.toml` for new transitive advisories/licenses from mlua (Lua is MIT — fine; verify `cargo deny check` stays green, like the recent ttf-parser episode).
- The `release-static` musl build now needs a C cross-compiler available; document in `INSTALL.md` / the Makefile.

## Security enforcement (verify, don't reinvent)
- `BoundAction::Lua` → `is_executing()` true (`src/keymap/user.rs:148`). The existing `Trust::Project` gate (`src/config/mod.rs:~543`) drops it from project-local config. Add a test mirroring `command_verb_is_executing`.
- The `lua` DSL verb is rejected (dropped) from a project `.spycrc.toml`; only `~/.spycrc.toml` may bind it. `init.lua` itself is loaded only from `~/.config/spyc/` — never from cwd.

## Test plan (chdir-free, per the unit-test rule)
- **Pure, no Lua runtime:** DSL parses `map K lua foo` → `BoundAction::Lua("foo")`; `is_executing()` true; trust gate drops a project-local `lua` binding; `lua_request_to_effects()` maps each `LuaRequest` to the right `Effect`/`Action` (assert via the `effect.rs` test matchers, not struct destructuring); the action-name table in `bridge.rs` round-trips against `dsl::parse_action`.
- **With the worker (integration):** submit a trivial script returning a known request batch → assert the effects; an infinite-loop script → assert the hook aborts within the ceiling and the worker reports the error (no hang — bound the test with a timeout); a syntax-error `init.lua` → worker reports error, registries stay empty, spyc usable.
- **Watchdog logic** is a pure decision (`elapsed > soft_ms && job_in_flight && !prompted → ShowPrompt`) — extract + unit-test like `route.rs`/`focus.rs`.

## Docs + version (same commit as code, per the sync rule)
`README.md`, `FEATURES.md`, `AGENTS.md` (feature line + **module index** for `src/lua` and `src/app/lua.rs`), `src/ui/help.rs`, `DESIGN.md` (the `:lua` / runaway-prompt UI language), `src/config/default.spycrc.toml` (+ `--print-config`: a `lua` verb example + a commented init.lua pointer), `CHANGELOG` via the commit subject, version bump in `Cargo.toml` (minor — feature) + `cargo update -p spyc`.

## Phased PR breakdown (one shippable shape each, version-bumped)
0. **PR0 — XDG `config_root()` helper (standalone, dependency-free).** Add the `$XDG_CONFIG_HOME/spyc` else `~/.config/spyc` resolver mirroring `state_root()` + its thread-local test override; unit tests. No mlua, no C dep — lands and verifies on its own, de-risking the engine PR. Ships first.
1. **PR1 — Lua worker + Tier A keybindings + core safety.** mlua dep + Cargo comment rewrite; `src/lua/` worker/bridge/api; `Runtime.lua`; `BoundAction::Lua` + DSL `lua` verb + trust gate + tests; snapshot-in / batch-out (`action`/`cmd`/`navigate`/`pick`/`filter`/`report_status`/`notify`); **instruction hook + abort flag + hard ceiling + error containment** (auto-kill, flash-only — no modal yet); `:lua off`/`--no-lua`. Smallest end-to-end slice, safe from day one.
2. **PR2 — Tier B: init.lua platform.** Loader in bootstrap + reload + watcher tracking; `spyc.map` / `spyc.command` + the COMMAND_TABLE fallthrough + completion merge; the inert `spyc.on` stub; `:lua reload`/`:lua status`.
3. **PR3 — Interactive runaway control.** The soft-threshold scheduler deadline + the "keep waiting? [y/N]" modal (new confirm variant in `modal.rs`/confirm handlers) wired to the abort flag; pure watchdog-decision tests.
4. **PR4 — Round-trip live reads.** `spyc.worktrees`/`git_status`/`git_log`/`read`/`search_*` reusing the MCP read logic; richer `context`.
5. **PR5 (phase 2, deferred) — Tier C events.** `Event` enum + `fire_event` + the low-frequency dispatch sites; activate `spyc.on`.

## Top risks & mitigations
1. **A runaway script wedging the UI.** Mitigated structurally by off-thread execution + the layered kill switches (hook abort flag, hard ceiling, interactive prompt, single-worker serialization, global off switch). Residual: a script blocked inside a C call — mitigated by not exposing blocking primitives and routing all blocking work through cancelable spyc round-trips.
2. **MVU/thread-safety erosion.** `mlua::Lua` is non-Send and lives only on the worker; main holds only `Send` channels + the abort flag; Lua produces requests-as-data applied by `run_effects`. No OS in the Model, no handle in the Model. The round-trip reuses the proven MCP request/reply path. Guards (`mod_rs_stays_decomposed`, the render purity scan, the AGENTS index) all stay green because new code lives in dedicated modules.
3. **The C-dependency reversal.** Accepted deliberately; the honest fix is updating the Cargo.toml claims, verifying `cargo deny`, and documenting the musl/C-compiler requirement in INSTALL — so the repo stops advertising a property it no longer has.

## Verification (end-to-end, from inside spyc)
- `make check` green (fmt + clippy + test + deny).
- `make install`; create `~/.config/spyc/lua/hello.lua` returning a navigate+notify; `map g h lua hello` in `~/.spycrc.toml`; press `g h` → observe navigation + flash.
- `~/.config/spyc/init.lua` with `spyc.command('blame', function() spyc.action('git_blame') end)`; run `:blame` → blame view.
- Runaway test: a script with `while true do end` → UI stays responsive, the "[y/N]" prompt appears after the soft threshold, `N` kills it with a flash; confirm spyc is fully usable afterward.
- Confirm a project-local `.spycrc.toml` with a `lua` binding is silently dropped (only `~/.spycrc.toml` honored), and a project `init.lua` is never loaded.
