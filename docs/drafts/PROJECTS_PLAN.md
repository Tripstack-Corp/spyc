# Projects — one spyc, many project homes (2.3 design)

**Status: draft for review.** A 2.2 deliverable
([#492](https://github.com/Tripstack-Corp/spyc/issues/492)); 2.3's implementation
([#99](https://github.com/Tripstack-Corp/spyc/issues/99)) starts once this doc is
approved. It answers the seven questions in
[`V2_2_PLAN.md`](V2_2_PLAN.md) §7, each with a decision and a reason, against the
tree as of `main` after #518. Facts about today's code
carry a `file:line`; where a decision needs the owner's call rather than an
argument, it is listed under [Open questions](#open-questions-for-review).

**Owner decisions so far** (2026-10-01; the rest of the doc is still in review):

- Per-project configuration lives under `~/.config/spyc/`, not in the
  project's directory (§1f). The same principle moved the MCP context marker
  out of the working directory, which landed in 2.2 for the one root a spyc
  has today (§2, #525).
- The attention key is `Space !` (§4).

The goal, from [ROADMAP.md](../../ROADMAP.md) → "The 2.3 horizon": stop managing
several terminal windows. One spyc process holds several projects, each with its
own columns, pane tabs and agents; a switcher moves between them; one attention
signal covers every agent in every project; and recovery restores all of them.

What stays out, permanently: peer discovery, frame mirroring, input forwarding,
headless peers, and any CounterTop revival. Two spyc processes remain two
separate things, which #509 made safe; one spyc never finds, adopts or talks to
another.

---

## 0. What a project is

A **project** is a home directory and everything the user is doing under it:
the file columns (`a`, and `b` when split), the split's shape, the pane tabs and
the agents in them, the pane layout, and where the keyboard was. Today all of
that exists exactly once, in `AppState` and `Runtime`; a project is that set, made
plural.

- **Identity is a `ProjectId` (a uuid), never an index and never a path.** An
  index is the tab-numbering drift #151 fixed, waiting to happen again when a
  project closes. A path would change identity on `Space P` (re-home). The
  switcher's order is presentation; the id is what state is keyed by.
- **One project is active.** It is what the frame draws and what the keyboard
  drives. The others keep running: their agents work, their pty output is
  parsed, their dots update, and MCP calls from their agents act on them.
- **A single-project spyc is today's spyc.** Launching creates one project at the
  launch directory, which is exactly today's `PROJECT_HOME` (always set to the
  launch cwd, `bootstrap.rs:87-96`). No project UI appears until a second project
  exists, so nobody who never opens one sees a change.
- **Homes are unique.** Opening a home that's already open switches to it.
  Re-homing onto another open project's home is refused.

The prep refactor already landed: #40 (#504) made every column access go through
a handle (`cur()`, `col(side)`, `active_sides()`), guarded by
`columns_are_addressed_through_handles`. Those handles become project-relative —
`cur()` is the active project's focused column — so the hundreds of call sites
that already say "the focused column" keep meaning it. Code that must act on a
project other than the active one (an MCP call from a background agent, a status
report, a git result) asks for it by id.

---

## 1. Per-project state inventory (question 1)

**The rule.** A field lifts into `Project` when it describes what the user is
doing in that project, so it would be wrong if it followed them to another. It
stays global when it's the user's own toolkit (yank bin, histories, marks), a
process resource (one socket, one git worker, one Lua engine), or transient UI
that can't outlive a mode (a half-typed chord, an open prompt).

The shape:

```rust
struct AppState {
    projects: Vec<Project>,     // switcher order; looked up by id
    active: ProjectId,
    // …the global fields below…
}

struct Project {
    id: ProjectId,
    home: PathBuf,              // today's `project_home`, no longer optional
    label: String,              // defaults to the home's basename
    left: Commander,
    right: Option<Commander>,
    vsplit: Option<VSplit>,
    focus: Focus,
    pane: PaneLayout,
    start_dir: PathBuf,
    prev_dir: Option<PathBuf>,
    config: Arc<Config>,        // $HOME layer + this home's .spycrc.toml
    // …
}

struct Runtime {
    projects: HashMap<ProjectId, ProjectRuntime>,  // pane tabs, overlays
    // …the global handles below…
}
```

`ViewState` gains the same split: a `HashMap<ProjectId, ProjectView>` for the
per-project pagers and row caches, and the rest stays flat.

The last column of each table answers **question 7** for that field, in §7's
codes: **R** recovery manifest (at rest), **A** attach snapshot only (in flight),
**C** rebuilt on the client from its terminal, **D** daemon-only (OS handles,
never serialized), **T** transient (dropped).

### 1a. `AppState` (`src/app/state/mod.rs:659`)

| field | goes to | why | 7 |
|---|---|---|---|
| `left`, `right` (`Commander`) | project | The columns are the project. Each carries its listing, picks, filter, sort, cursor, masks, view, git state and harpoon; see 1b. | R/A |
| `vsplit` | project | The shape of that project's columns. | R |
| `focus` | project | Switching back lands the keyboard where it was, list or pane. | R |
| `pane` (`PaneLayout`) | project | Height, zoom, hidden and `vsplit_left_was_pane` describe that project's split. `pending_images` is keyed by tab id, whose tabs are the project's. `pane_prompt_buf` / `last_pane_prompt` track the focused pane, so they're the project's too. | R (height), A (rest), T (`pane_snapshot`) |
| `project_home` | project, as `home` | It is the project's identity field now, and required: a project without a home has nothing to name it. `:project clear` goes away; `:project <path>` re-homes. | R |
| `start_dir` | project | The `` ` `` target, and today the anchor for the project-local config layer and the context file (`bootstrap.rs:210`). All three are per-project meanings. Defaults to the home. | R |
| `prev_dir` | project | `''` means "back where this column was", not "wherever I was in another project". | A |
| `config` | project (`Arc`) | Each project merges the `$HOME` layer (shared by reference) with its own per-project file under `~/.config/spyc/projects/` (§1f). | C (rebuilt from files) |
| `user_keymap` | project | Derived from that project's `config.bindings`. | C |
| `scope_registry` | **global**, with a change | Claims coordinate merges, and two projects can be worktrees of one repo. That is precisely the case coordination exists for. But a claim's `paths` are raw strings resolved against nothing (`scope_registry.rs:44-56`), so `src/*.rs` in one repo conflicts with `src/*.rs` in an unrelated one. 2.3 adds the claimant's repository (the common gitdir) to `ScopeClaim`, and `conflicts` compares within one repository only. | R |
| `mounts` | global | Keyed by archive path. Two projects browsing one archive share its journal, which is the consistent answer: there is one container on disk. | A |
| `inventory` | global | The yank bin is how a file travels between projects. Per-project inventories would remove the one cross-project carry the user has. | own file |
| `marks` | global | Letters are a user namespace with absolute paths, like vim's uppercase marks. A jump navigates the active project's focused column. | own file |
| `graveyard` | global | A disk store; restore puts a file back at its own path, whichever project is active. | own file |
| `frecency` | global | `J` across everything you've visited is the feature. | own file |
| `history`, `pane_history`, `pane_cwd_history`, `jump_history`, `command_history` | global | Typing history is the user's muscle memory, not the project's. | own files |
| `last_search`, `last_captured_cmd` | global | The user's last `/` and `!`, like the histories. | A |
| `session_name`, `session_id` | global | They name the recovery manifest, which covers every project (§3). | R |
| `mode`, `flash`, `resolver` | global | Modal UI. A project switch happens from Normal mode, so no prompt or chord spans two projects. | T |
| `pending_new_tab_cmd`, `pending_worktrees`, `pending_sessions`, `pending_delete_preview` | global | Transient picker and prompt state, closed before any switch. | T |
| `mouse_capture_override` | global | A property of the terminal, not of a project. | C |
| `should_quit`, `quit_pending` | global | Quit quits spyc. The live-children confirm counts every project's tabs. | T |
| `user_host` | global | Process constant. | C |

### 1b. `Commander` (per column, `mod.rs:562`) — moves with its project

`listing` (R: the directory; A: the entries are re-read), `picks`,
`temp_filter`, `sort_order`, `sort_reversed`, `view`, `cursor`, `masks` (A: live
state worth keeping across a detach, not across a restart), `rows`,
`grid_dims`, `list_generation`, `harpoon_filter_set`, `pending_ghosts` (C or T:
derived from the listing and the terminal's geometry), `git` and `git_cache`
(rebuilt: re-read from the repository, never serialized), `harpoon` (its own
file, keyed by root hash, unchanged).

**What 1a/1b change beyond the move.** Two facts in today's save path become
visible once a project is a first-class record. Column `a`'s directory isn't
saved at all: restore puts it at `Session.cwd`, which is `project_home`, not
where the column was (`session.rs:189-193`). And picks, filter and sort aren't
saved either. The manifest in §3 saves `a`'s directory, since "restores every
project" means restoring where each one was. Picks, filter and sort stay out of
the at-rest manifest (A, not R) because they are the least stable state spyc
has: restoring a stale pick set is worse than restoring none.

### 1c. `Runtime` (`src/app/runtime.rs:37`)

| field | goes to | why | 7 |
|---|---|---|---|
| `pane_tabs` | project (`ProjectRuntime`) | A project's tabs are its agents. Every `PaneTabs` operation today is "the tab list"; it becomes "this project's". | D (the ptys); the tabs' commands, cwds and session ids are R |
| `top_overlay`, `top_overlay_right` | project | The `V` / `D` / `;` overlays belong to a column. | D |
| `git_worker_tx`, `git_result_rx` | global, with a change | One worker suffices. Results are routed by `Side` alone today (`git_state.rs:75-87`), so they must carry `(ProjectId, Side)`, or a background project's result lands in the active project's column. The per-column generation gate stays. | D |
| `mcp_cmd_rx` | global | One socket per process (§2). | D |
| `lua`, `lua_registry`, `lua_inflight` | global | One engine. `lua_events` gains the project: `project_changed` fires on a switch as well as on a re-home. | D |
| `scope_waiters` | global | Parked against claims, which are global. | D |
| `mcp_config_dirs` | global | `dir_owners` refcounts by pid (`dir_owners.rs:26-41`), and a directory can be used by tabs in two projects. Releasing on project close could remove an entry another project in this process still relies on, so claims stay process-lifetime, as today. The entries are socket-free, so one lingering is harmless. | D |
| `pending_capture`, `capture_spill_dir`, `background_tasks` | global | `!` tasks are the user's shell work. Each records its cwd, and `:fg` works from any project. | D |
| `pager_stream`, `scroll_stream`, `stashed_pager_streams`, `find_picker`, `pending_git_view` | global slots, stashed per project | A switch stashes the active project's open pager with its stream, the way a tab switch already stashes a scrollback pager (`stashed_pager_streams` is keyed by stream id, not by tab). | D |
| `worktree_results` | global, with a change | MCP worktree jobs complete off-thread. The result must name the caller's project, so `create_worktree(open: true)` opens column `b` in the agent's project, not the one the user is looking at. | D |
| `listing_refresh_inflight` / `_dirty`, `preview_results` / `preview_reloading` | global | The watcher only watches the active project (§1e), and a stale result is dropped by project plus generation. | D |
| `agent_status_*`, `codex_pin_*` | global | They scan every tab; the scan now walks every project's tabs. | D |
| `autosave_last_saved_fp`, `autosave_due` | global | One manifest (§3). | D |
| the landing slots (`graveyard_results`, `image_results`, `archive_results`, `file_results`, `clipboard_*`, `inventory_results`) | global | Each result carries what it needs; the ones that touch a column carry the project. | D |
| `picker` (terminal graphics capability) | global | Probed from the terminal. | C |
| `pane_wake_tx`, `next_sink_id`, `next_stream_id`, `pane_scroll_settle`, `hook_recheck_at` | global | Loop plumbing. `pane_scroll_settle` holds a tab index, which becomes `(ProjectId, index)`. | D |

### 1d. `ViewState` (`src/app/view_state.rs:124`)

| field | goes to | why | 7 |
|---|---|---|---|
| `pager`, `pager_right`, `right_pager`, `displaced_preview`, `scroll_pager`, the help stashes | project | Open views over that project's columns. | A (`right_pager.source_path` is R, as today) |
| `pager_history` | project | `[b` / `gp` means "what I was reading here". Reopening a buffer from another project inside this one is disorienting. | A |
| `cached_rows*`, `right_cached_rows*` | project | Keyed by each column's `list_generation`. | C |
| `context_path`, `last_context`, `context_dirty` | project | One context file per project home (§2). | D |
| `pager_positions` | global | Keyed by file. | own file |
| `theme` | global, recomputed on switch | Follows the active project's config. | C |
| `term_size`, `color_depth`, `is_ssh`, `hud_*`, `last_term_title`, `chrome_rows` | global | Facts about the terminal and the frame drawn on it. | C |
| `activity`, `show_activity`, `activity_style`, `started_at` | global | Process-wide monitor; the HUD and `:activity dump` group by project (§6). | A |
| `visual_bell`, `agent_anim_frame` | global | One frame, one pulse. | T |
| `chord_hint*`, `quick_select`, `harpoon_menu`, `image_gallery`, `image_view`, `tab_state`, selections, the pending-`g`/`z`/bracket flags, `focus_chord_completed`, `scroll_last`, `pane_scroll_streak`, `pane_view_sent`, `mouse_*` | global | Transient UI on the active project, closed or dropped by a switch. `image_view` and `image_gallery` re-encode from source bytes on the client (§7). | T / C |
| `transcript_show_tool_calls`, `scroll_source_override`, `dim_inactive` | global | Viewing preferences; `scroll_source_override` is keyed by tab id already. | A |
| `agent_status_cache` | global | The active tab's. | T |
| `mcp_running` | global | One socket. | D |

### 1e. Process-global resources and how each follows the active project

- **Process cwd.** `settle_process_cwd` compares `cur()`'s directory against
  `getcwd` (`process_cwd.rs:36`). With `cur()` project-relative, it follows the
  active project with no change.
- **fs-watch.** One watcher with fixed slots (`a`, `b`, preview, config parents),
  addressed by field in `sync_watch` (`watch.rs:78`). It watches the **active
  project only**. An inactive project's columns can go stale, so activation
  re-lists them and invalidates their git caches: the first frame after a
  switch pays for one refresh, which beats N projects' worth of recursive
  watches and their inotify budget.
- **Git polling.** The 1 Hz mtime poll runs for the active project's columns
  only, for the same reason, and activation forces a poll.
- **Pty readers.** Unchanged. Every tab already has its reader thread and its
  output drained, which is how background tabs light their dots today. An
  inactive project's tabs are just more background tabs.
- **Terminal title.** Shows the active project (§5).
- **Memory.** Each tab's engine keeps its scrollback row budget
  (`spyc-vt-sys` `scrollback.rs`); N projects times M tabs is N×M budgets. That's
  the one cost that scales with projects rather than with what's on screen, and
  the activity HUD should show the total.

### 1f. Per-project configuration lives under `~/.config` (owner decision)

A project's own settings go in a file the user owns, beside the rest of spyc's
config, never in the project's directory:

```
~/.config/spyc/projects/<label>.toml        # config_root(), i.e. $XDG_CONFIG_HOME/spyc
```

```toml
home = "~/src/spyc"          # which project this file configures

[pane]
tabs = ["claude", "zsh"]     # startup tabs when this project opens

[colors]
dir = "#88c0d0"              # e.g. tint each project so a glance says which one
```

- **Matching is by `home`, not by file name.** The file name is the project's
  default label, and a home claimed by two files is a load warning naming both.
  `Space n` on a directory with no file creates nothing: a project needs no file
  until the user wants to configure it.
- **It is trusted.** The file sits in the user's own config directory, as
  `~/.spycrc.toml` and `init.lua` do (`state::config_root`, `state/mod.rs:84`),
  so it is `Trust::Trusted`. Executing bindings, `[prompts]` and startup tabs work
  from it without the consent a repo's file needs. That consent exists because a
  repo is someone else's text; this file is the user's.
- **Layering**, lowest first: `~/.spycrc.toml`, then the repo's `.spycrc.toml` if
  one exists (`Trust::Project`, unchanged, for repos that ship one), then the
  per-project file. The user's own per-project choice wins over the repo's.
- **The schema is `.spycrc.toml`'s,** plus `home`. Reload watches the active
  project's file along with `~/.spycrc.toml`. Nothing new is written into any
  working directory.

---

## 2. MCP socket topology (question 2)

**Decision: one socket per process, as today.** `<state>/mcp-<pid>.sock`, one
accept thread (`server.rs:421-489`). What becomes per-project is the context the
socket answers from, which the connection's pane selects.

Why not a socket per project: the socket isn't a security boundary (one uid, see
SECURITY.md), and the routing a second socket would provide already exists. Each
agent pane gets `SPYC_MCP_SOCK` and `SPYC_PANE_ID` (`pane_tabs.rs:175-188`), the
proxy names the pane in `initialize` (#507), and the connection binds once to a
live tab (`protocol.rs:211-242`). A pane belongs to exactly one project, so a
bound connection already knows its project. That is the lookup the
pane-identity proposal promised: "which pane called" and "which project it
belongs to" are one lookup.

**Already landed (2.2, #525, closing #523).** Items 2 and 4 below shipped
for the one root a spyc has today, because the launch-anchored marker was a
bug before it was a projects problem: `spyc -r` from another directory left
the trusted root behind.

- The context file lives in the state directory,
  `<state>/.spyc-context-<pid>.json`, and nothing is written into the working
  directory for it.
- `SpycContext::root` carries the root (`start_dir`, which restore moves).
  `allowed_roots` trusts that root, not the file's location.
- `write_context` rewrites the sidecar when the root moves.
- Discovery reads only the sidecars.

What 2.3 adds is the plural: a file per project, and a sidecar line per
home.

What changes:

1. **Binding resolves to a project.** `McpCommand::PaneContext` searches every
   project's tabs (today it searches the one list, `app/mcp.rs:239-259`) and
   returns the project id with the tab. `Caller` stores it.
2. **One context file per project, in the state directory.** Since #525 a spyc
   keeps one file, `<state>/.spyc-context-<pid>.json`, whose `root` is the
   session's. With projects, each project gets its own,
   `<state>/.spyc-context-<pid>-<project id>.json`, with that project's home as
   `root`. Each is written from its project's focused column and removed when
   the project closes. Each pane's `SPYC_CONTEXT` names its own project's file.
3. **Read tools answer from the caller's project.** They run on the socket
   thread from the context file, with no `App` (`ARCHITECTURE.md`, "MCP
   server"), so the bound project decides which file. `search_root`, the `root`
   override and `allowed_roots` are all computed from that project's file.
   `allowed_roots` already trusts the root a context names (#525), so a
   project's home is its trusted root with no further change, and its worktrees
   and focused column's chain extend it.
4. **The trusted-root sidecar lists every home.** Since #525 the sidecar is
   discovery's index: `collect_project_pids_in` reads the sidecars and nothing
   in the caller's tree, takes the live pids whose root contains the agent's
   cwd, and keeps the nearest root. So the planted-marker attack can't happen
   at all. `mcp-<pid>.root` holds one root. With projects it becomes one line
   per open project, rewritten atomically on open, close and re-home, and
   discovery matches against every line.
5. **Driving tools act on the caller's project, not the user's view.**
   `navigate_to`, `pick_files`, `set_filter` and `open_worktree` move the
   columns of the agent's own project. An agent in a background project must not
   yank the user's view in the one they're looking at, and under the 3.0 daemon
   there may be nobody looking at all. `get_spyc_context` answers for the
   caller's project, and adds `project: {id, label, home, active}`, so an agent
   can tell whether the user is currently in its project.
6. **Unbound connections pick a project by cwd.** An agent started outside
   spyc reaches the socket through marker discovery and has no pane. Its proxy
   adds its cwd to `initialize` (`_meta["spyc/cwd"]`, beside `spyc/paneId`),
   and the server binds the connection to the open project whose home contains
   it, else the active project. That's the same "attribution, not
   authorization" footing as the pane id. The cwd is a routing hint from a
   process the user runs, and SECURITY.md already says which.
7. **Unattributed fallbacks stay "the focused tab"**, meaning the active tab of
   the active project (`resolve_report_target`, `app/mcp.rs:819-836`).

**The same principle, applied further (a follow-up, not 2.3 scope).** spyc
still writes agent config into working directories: `.mcp.json`,
`.codex/config.toml`, `.agents/mcp_config.json` and the status hooks. They're
there because each agent reads its project scope from there. But since #509 the
MCP entry names no socket and the pane's env decides, so one entry in each
agent's *user* scope would serve every pane. An org-deployed
`managed-mcp.json` with that shape already works for claude (decisions log,
2026-09-30). Moving them would stop
the per-directory writes and the refcounted cleanup entirely. It changes what an
agent started outside spyc sees: a spyc server that runs read-only, or none.
That is why it wants its own issue and its own decision.

What doesn't change: MCP entries still name no socket (SPYC-TRAP
`mcp-entry-names-no-socket`), so there is nothing to take over. #509 deleted the
takeover machinery the §7 brief asks about. `dir_owners` stays keyed by pid.
Status-hook consent stays keyed by project root. The orphan sweep already runs
in the state directory (#525), where dead spycs' context files, sidecars and
sockets now live, so it needs nothing per project. Its pass over the launch
directory only clears markers that older spycs wrote there.

---

## 3. Recovery manifest (question 3)

**Decision: one manifest per process, holding every project.** The file, the
debounce and the atomic write stay as they are: `<state>/sessions/<id>.json`,
`fs::write_atomic`, `AUTOSAVE_DEBOUNCE` of 2 s, a stable per-process id, and a
fingerprint that arms only while dirty (`session.rs:30-60`, `304-365`). What
changes is the shape inside:

```json
{
  "version": 2,
  "id": 1727700000000,
  "pid": 41234,
  "name": "SAFFRON_CUMIN",
  "saved_at": "…", "epoch_secs": 1727700000,
  "active_project": "0192…",
  "projects": [
    {
      "id": "0192…",
      "home": "/Users/…/src/spyc",
      "label": "spyc",
      "start_dir": "/Users/…/src/spyc",
      "left_cwd": "/Users/…/src/spyc/src/app",
      "vsplit": { "…": "today's SavedVsplit" },
      "pane_height_pct": 70,
      "pane_focused": true,
      "tabs": [ { "…": "today's SavedTab" } ],
      "active_tab": 0
    }
  ],
  "scope_claims": [ "…today's ScopeClaim, plus its repository…" ]
}
```

- **Reads both shapes.** A file with no `version` is today's session: it loads
  as a one-project manifest whose home is its `project_home` (else its `cwd`).
  Every saved session keeps working. A downgraded spyc can't parse a v2 file and
  skips it in the picker, which is the honest failure: it can't restore several
  projects anyway.
- **The fingerprint covers every project**: each project's tabs, cwds, home,
  pane height, focus and split, plus the active project. An agent working in a
  background project doesn't dirty it, just as today; a tab opening or closing
  there does.
- **`pid` makes a live manifest visible.** Today two processes restoring the
  same session both overwrite one file. The picker marks a manifest whose pid is
  alive as "running", and restoring one gives the new process a fresh id rather
  than adopting the running one's file.
- **`MAX_SESSIONS = 20`** (`sessions/mod.rs:11`) stays a cap on manifests, not
  projects.

**What `spyc -r` offers.** The picker keeps its shape and keys
(`session.rs:367-435`, `pickers.rs:376-431`). Each row is one manifest, and its
project list replaces today's single cwd:

```
  [1]  SAFFRON_CUMIN          2h ago         spyc (3 tabs) · web (1) · api
  [2]  NUTMEG_SUMAC  running  5m ago         spyc (2 tabs)
```

Enter restores every project in the manifest, active project included. Getting
back **one** project isn't a picker mode: the project switcher (§4) lists
recently-closed projects under the open ones, drawn from the saved manifests,
and choosing one reopens it with its tabs into the running spyc. That answers
"I only want that one back" without making `-r` a two-level menu.

**Engine snapshots are not part of the 2.3 manifest.** The snapshot API
restores pane screens with 100% fidelity, but the engine spike rules it out at
rest (§7). A manifest has to survive a `brew upgrade` between quit and `-r`.
The agents already replay their own history on resume, and HARNESS.md §4
documents how each one does it. Shell tabs lose their scrollback on restart,
exactly as they do today. The daemon is the answer to that (3.0), not the
manifest.

**Found while answering this, and since fixed:** `TabInfo.restore_fallback`
was never set, so the `ClaudeCrashRecover` prompt couldn't fire. It guarded a
`claude --resume` mount crash that restore stopped risking in v1.17.9. The
dead machinery was deleted in 2.2 (#524, closing #522), so 2.3 builds on a
restore path without it.

---

## 4. Keymap-tier placement (question 4)

Project operations are workspace operations: `Tier::Global`, under the leader,
reachable as `Space …` from the list and `^a Space …` from the pane. The guard
`leader_and_pane_namespaces_respect_tiers` (`resolver/tests/prefixes.rs:554-609`)
enforces the tier, and the which-key completeness guard requires every key below
to appear in the leader's hint list.

The leader today holds `w` (worktree submenu), `p`, `P`, `s`/`S`, `?` and `a`
(`resolver/mod.rs:368-386`). The rest is free.

| keys | action | why this key |
|---|---|---|
| `Space j` | project switcher: open projects with their attention glyphs, then recently-closed ones; `1`-`9` / Enter pick | `j` is jump, as in `J`, jump to a path |
| `Space Space` | last project | the leader's own key, as `^a ^a` is last tab and tmux's prefix-twice is last window |
| `Space 1` … `Space 9` | project N | as `^a 1`…`9` is tab N |
| `Space ]` / `Space [` | next / previous project | the bracket pair spyc uses for next/previous everywhere |
| `Space n` | new project: prompt for a directory, defaulting to the focused column's worktree root; switches if that home is open | `n` is new, as in `Space w n` |
| `Space x` | close the active project, with the live-children confirm `^a x` uses (`needs_live_child_confirm`) | `x` is close, as in `^a x` |
| `Space !` | go to the agent that needs you: the oldest blocked tab in any project, then unseen done ones, switching project and tab and focusing the pane | "urgent"; owner decision, 2026-10-01 |

**From the pane, the leader is `^a Space`.** `Space` is literal text to the
agent, so while the pane has the keyboard spyc sees only its prefixes
(`is_spyc_meta_when_pane_focused`). `^a` wakes spyc, and `Space` then enters the
leader, so every key above is `^a Space <key>` from inside an agent:
`^a Space !` is three keys. That bridge is the only way a `Tier::Global` action
reaches the pane, and the tier guard keeps it so. The attention jump is the one
key a user reaches for *from* an agent pane, so it may earn `Tier::Meta` (the
tier `Help` and `Quit` share), which may bind on both prefixes, making it
`^a !` as well. See open question 2.

Outside spyc — another terminal window, another app — no key reaches it. A
terminal program can't register a system-wide hotkey. The signals that do
reach the user there are the terminal title (§5) and the desktop notification
(§6), whose job is to bring them back.

**`Space p` and `Space P` keep their meanings,** scoped to the active project.
`Space p` jumps the focused column to the project's home. `Space P` (and `gP`)
re-homes the active project to the current directory, refused when another open
project has that home. Neither needs a new key, and neither changes for anyone
with one project.

Renaming a project's label stays a `:` command (`:project name <label>`),
following the policy of keeping rare operations off default keys.

---

## 5. The `projects` status-bar segment (question 5)

Today: `🌶️ | project | session | path | git | agent | suffix`
(`ui/status.rs:121-173`). The brief's list omits the agent segment; it exists,
and it shows the active tab. Every segment draws at full width, only the path
truncates (middle, `status.rs:239-254`), and nothing is dropped.

**Decision: the `project` segment becomes the `projects` segment,** and with
more than one project open it displaces the `session` segment.

- **One project open:** unchanged. The segment shows the home's basename.
- **Several:** each project's label with its attention glyph, the active one in
  the segment's bold style, the others dim:
  `spyc ■ · web ● · api`. The glyphs are the tab dots' own (§6): a red `■` means
  something there is blocked, a teal `■` means an unseen done, and a pulsing `●`
  means working.
- **The session name leaves the bar** when several projects are open. It names
  the manifest, rarely changes, and stays in the terminal title and in `Space s`.
  That returns its width to the projects segment.

**At narrow widths the segment gets a budget** (a third of the bar) and degrades
in steps, each one only if the previous still doesn't fit:

1. drop inactive projects with no attention, as `+N`: `spyc ■ · web ● +2`;
2. collapse the others to a count and their most urgent glyph: `spyc 2/4 ■`;
3. the active label alone, with a `■` if any other project is blocked.

A blocked agent in another project is never dropped from the bar entirely. It
is the one thing the segment is for.

**The terminal title** keeps `🌶️: <active project> · <name>` and adds a blocked
count when another project has one: `🌶️: spyc (■2) · SAFFRON_CUMIN`. The title is
what the user sees from another terminal tab, which is exactly when they aren't
looking at spyc.

---

## 6. Attention and notification aggregation (question 6)

**Reused unchanged:**
- each tab's `AgentActivity`, and its precedence: a self-report (`report_status`
  and the per-agent status hooks) beats the screen scrape, which beats output
  timing (`agent_status.rs:415-443`);
- the latched `Blocked`;
- the `Blocked` / `Done` edge in `notification_for_transition`;
- `Effect::Notify` with its three channels;
- the `[notify]` gates.

None of it knows about projects, and none of it has to.

**Changed:**

- **`settle_agent_activity` walks every project's tabs,** not just the active
  one's (`agent_status.rs:544-712`). An agent in a background project
  transitions and notifies like a background tab does today.
- **"Focused" means the active tab of the active project, with the pane
  focused.** That is the one input `suppress_focused_tab` reads
  (`agent_status.rs:630`). Every tab in an inactive project is unfocused by
  definition.
- **Notification text names the project** once more than one is open:
  "claude in web needs you", "web: tab 2 is blocked". The tab number alone is
  ambiguous across projects.
- **`has_activity` (a shell tab's `+`)** is set for every tab that isn't the
  active project's active tab (`streaming.rs:383-387`).
- **The latched `Blocked` still clears only on an answer to the active pane**
  (`effect.rs:714-740`). A blocked agent in a background project stays red
  until you go there and answer it, which is correct: switching projects isn't
  an answer.

**New:**

- **A per-project roll-up.** `Project::attention()` returns the most urgent of
  its tabs, in the order blocked > unseen done > working > idle. "Unseen done"
  is a `Done` since the user last had that tab active. The flag clears on
  activation, as `has_activity` does for a tab. The roll-up feeds the projects
  segment, the switcher and the title (§5).
- **`Space !`**, the answer to "which agent, in which project, needs me": it
  goes there (§4).
- **The visual bell** stays a whole-frame pulse (`overlays.rs:19-52`), because it
  means "look up", not "look here". When the transition is in another project,
  the flash names it.
- **The activity HUD, `:activity dump` and `:agent list` group by project.**
  Each MCP connection is listed under its bound project, as #508 lists it under
  its tab.

---

## 7. Attach-awareness of the inventory (question 7)

The recovery manifest (§3) and a 3.0 attach snapshot draw on one inventory, §1,
through two consumers. The codes used in §1's tables:

- **R — recovery manifest.** At rest, so it must survive a restart and an
  upgrade. Plain serde of semantic state (homes, directories, tab commands, cwds,
  agent session ids, split shape, claims), tolerant of fields it doesn't know.
- **A — attach snapshot only.** In flight, from a live daemon to a client. It is
  a superset of R: it adds live state worth keeping across a detach but not
  across a restart (picks, cursor, filters, open pagers and their positions,
  pane screens).
- **C — rebuilt on the client.** Anything that is a function of the attaching
  terminal: size, colour depth, graphics protocol, title, the host's mouse and
  paste modes, and the layout and row caches derived from them. Cross-emulator
  reattach, the bug class the ROADMAP says 3.0 prices first, lives entirely
  here. So these fields must never be serialized, or an attach from a different
  terminal inherits the old terminal's answers.
- **D — daemon-only.** OS handles and workers: ptys, channels, threads, the
  socket. They never cross a wire, and in 3.0 they never need to, because the
  daemon keeps them.
- **T — transient.** Chords, prompts, selections, pending flags. Dropped on
  detach.

**Pane screens are A, never R.** The engine's snapshot API
(`ghostty_snapshot_encode` / `_decoder_*`) reconstructs a pane with 100%
fidelity, scrollback included (`VT_ENGINE_SPIKE.md`, "Mechanism B"). But its
format is version 1, without a compatibility guarantee, so, in the spike's
words: **snapshots are transport-only — same binary, same pin — and never
at-rest persistence across an upgrade.** A daemon handing a live pane to a
client it spawned is the same binary. A manifest read after `brew upgrade` is
not. The snapshot's magic and version make a stale one detectable, so a
mismatch discards it rather than misparsing it. The spike also records the
constraint a daemon inherits: continuation tracking must be on *before* the
input that leaves the parser mid-sequence, so a daemon that intends to snapshot
pays for tracking continuously.

**In-pane graphics are C, and there is nothing of the child's to reconstruct.**
The spike left this unpriced, as a "serializes or rebuilt" question for this
doc. Today spyc doesn't pass a child's kitty or sixel output through to the
host: nothing under `src/pane/` handles it. What spyc *does* draw — the image
viewer, the `^a g` gallery and mermaid renders — it draws from source bytes
through a graphics `Picker` probed from the terminal (`runtime.picker`). On
attach, the client probes its own terminal and re-encodes from those bytes. If
graphics passthrough is ever added, it arrives as a C field with the same rule.

**Two things this classification leaves to 3.0,** because they're the client
protocol's decisions rather than the inventory's:

1. **Which side draws.** If the 3.0 client renders from Model state, A is the
   snapshot it receives. If the daemon renders and the client relays its
   terminal, as dtach does, A collapses to nothing and only C remains. The
   classification still says which fields depend on the client's terminal,
   which is the part cross-emulator reattach exposes either way.
2. **Build skew.** Attach over SSH, the owner's stated direction, runs the client
   on the daemon's host (`ssh host spyc -a`). So client and daemon share an
   install, and "same binary" holds unless an upgrade lands between the daemon's
   start and the attach. The attach handshake compares build identities and
   refuses on a mismatch. A snapshot has no safe cross-build fallback, by the
   rule above.

---

## Sequencing for 2.3

Each step is a behaviour-preserving refactor or a guarded feature, so each is one
PR:

1. **One `Project`, behaviour-identical.** Introduce `ProjectId`, `Project`,
   `ProjectRuntime` and `ProjectView` holding today's fields, with one project;
   the handles delegate. Extend `columns_are_addressed_through_handles` so only
   project-level code names `projects[…]`.
2. **Route results by project.** Git worker results carry `(ProjectId, Side)`;
   worktree job results carry the caller's project.
3. **Per-project MCP.** A context file per project (the single-root version
   landed in #525), `PaneContext` returning the project, reads and driving tools
   on the caller's project, the sidecar as a list of homes, and `spyc/cwd` for
   unbound connections. Tests land first, against two
   projects in one test app.
4. **Manifest v2.** The v1 reader, `left_cwd`, `pid` and the picker labels.
5. **Open, close and switch.** The `Space` keys, the switcher, stashing on a
   switch, watch and poll on the active project only, and refresh on
   activation.
6. **Attention.** The roll-up, the projects segment and its degradation ladder,
   the title, notification text and `Space !`.
7. **The per-project config files** under `~/.config/spyc/projects/` (§1f).
8. **Recently-closed projects in the switcher.**

Steps 1 and 2 are invisible and can land as soon as this doc is approved. Steps
3-6 are the release.

---

## Open questions for review

1. **The saved thing's name.** The 3.0 decisions log reserves "session" for
   agent conversations, but spyc's UI already says session for the spice-named
   save (`-r`'s "session picker", `:name`, the `session` segment, `Space s`).
   This doc calls the file a manifest and leaves the UI words alone. Renaming
   user-facing terms is its own change, and it wants the owner's word.
2. **`^a !` for the attention jump.** `Space !` is decided. Whether the jump
   also earns `Tier::Meta`, so `^a !` reaches it in two keys from an agent pane
   instead of `^a Space !`'s three, is a taxonomy call: Meta is today reserved
   for help, quit and display toggles (§4).
3. **Agent config in user scope.** Whether the follow-up in §2, one MCP entry
   and one set of status hooks per agent's user scope instead of per directory,
   gets an issue.
