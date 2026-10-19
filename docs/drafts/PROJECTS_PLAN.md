# Projects — one spyc, many project homes (2.3 design)

**TL;DR.** One spyc process holds several **projects**. A project is a home
directory plus its columns, split, pane tabs and agents. One project is active,
meaning drawn and driven by the keyboard, and the others keep running.
With one project open, spyc looks and behaves exactly as it does today.

What approving this doc agreed to (2026-10-12):

- **State (§1).** `AppState`, `Runtime` and `ViewState` each gain a per-project
  part, keyed by a `ProjectId` (a uuid, never an index or a path).
  - **Moves into the project:** columns, split, focus, pane layout, tabs,
    overlays, pagers and config.
  - **Stays global:** the user's toolkit (inventory, marks, histories, graveyard,
    frecency) and the process resources (one git worker, one Lua engine, one MCP
    socket).
  - **Call sites keep working:** the column handles (`cur()`, `col(side)`) become
    project-relative, so the code that uses them doesn't change.
  - **Only the active project is watched and git-polled.** A switch re-lists its
    columns.
  - **Scope claims gain their repository,** so `src/*.rs` in two unrelated repos
    no longer conflicts.
- **Per-project config (§1f, decided).** `~/.config/spyc/projects/<label>.toml`,
  matched by its `home` key and trusted like `~/.spycrc.toml`. It layers over a
  repo's own `.spycrc.toml`. Nothing is written into a project's directory.
- **MCP (§2).** Still one socket per process.
  - A connection already knows its pane (#507), and a pane belongs to one
    project, so a connection knows its project.
  - Read tools answer from that project.
  - Driving tools (`navigate_to`, `pick_files`, …) move that project's columns,
    never the view the user is looking at.
  - A codex session on codex's shared daemon, which has no pane, goes to the
    project whose home contains its cwd. An agent started outside spyc gets no
    tools (#619).
- **Recovery (§3).** One save file per process holds every project. It's a
  version 2 format, and today's files load as one-project saves.
  - `spyc -r` restores every project.
  - Single projects come back from the switcher's recently-closed list.
  - Column `a`'s directory is now saved; today's save drops it.
  - Pane screens still aren't saved: agents replay their own history, as today.
- **Keys (§4).** All on the `Space` leader, or `^a Space` from a pane:

  | keys | does |
  |---|---|
  | `Space j` | project switcher |
  | `Space Space` | last project |
  | `Space 1`–`9` | project N |
  | `Space ]` / `Space [` | next / previous project |
  | `Space n` | new project |
  | `Space x` | close the active project |
  | `Space !`, or `^a !` from a pane | jump to the agent that needs you, in any project |

- **Status bar (§5).** The `project` segment becomes `projects`: each label with
  its attention glyph, as in `spyc ■ · web ● · api`.
  - With several projects open, it takes the session name's place.
  - It shrinks in three steps when the bar is narrow, but never hides a blocked
    agent.
  - The terminal title adds a count of blocked agents.
- **Attention (§6).** The dot, hook and notification machinery is reused
  unchanged; it now walks every project's tabs.
  - Each project rolls up to its most urgent tab: blocked, then unseen done, then
    working, then idle.
  - A notification names its project.
- **3.0 attach (§7).** Every field in §1 is tagged for one of five fates:
  - saved to disk;
  - sent only when a 3.0 client attaches to a running spyc;
  - rebuilt from the attaching terminal;
  - kept by the background process;
  - dropped.

  Pane screens are only ever sent to an attaching client, never saved to disk,
  because the engine's snapshot format is valid only for the same binary.

**Rollout (Sequencing for 2.3).** Eight steps, one PR each, except step 5,
which lands in five stages.

- **Steps 1–2 change no behaviour** and land first. Step 1 introduces
  `Project`, holding today's single set of state. Step 2 tags background results
  with the project they belong to.
- **Steps 3–6 are the release:** per-project MCP, the new save format,
  open/close/switch, and attention.

**Decided at approval** ([Owner decisions at approval](#owner-decisions-at-approval)):

- The saved file is a manifest in this design and the code; the UI keeps
  saying session.
- The attention jump also gets `^a !`.
- Moving agent MCP and hook config to user scope is its own issue,
  [#619](https://github.com/Tripstack-Corp/spyc/issues/619), which then landed
  in 2.2.

---

**Status: approved** (2026-10-12). A 2.2 deliverable
([#492](https://github.com/Tripstack-Corp/spyc/issues/492)); 2.3's implementation
([#99](https://github.com/Tripstack-Corp/spyc/issues/99)) starts from it. It
answers the seven questions in
[`V2_2_PLAN.md`](V2_2_PLAN.md) §7, each with a decision and a reason, against the
tree as of `main` after #518. Facts about today's code
carry a `file:line`; where a decision needs the owner's call rather than an
argument, it is recorded under
[Owner decisions at approval](#owner-decisions-at-approval).

**Owner decisions during review** (2026-10-01):

- Per-project configuration lives under `~/.config/spyc/`, not in the
  project's directory (§1f). The same principle moved the MCP context marker
  out of the working directory, which landed in 2.2 for the one root a spyc
  has today (§2, #525).
- The attention key is `Space !` (§4).

The goal, from [ROADMAP.md](../../ROADMAP.md) → "The 2.3 horizon": stop managing
several terminal windows. One spyc process holds several projects, each with its
own columns, pane tabs and agents; a switcher moves between them; one attention
signal covers every agent in every project; and recovery restores all of them.

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
| `pending_capture`, `capture_spill_dir`, `background_tasks` | global | `!` tasks are the user's shell work. Each records its cwd, and `:fg` works from any project. | D |
| `pager_stream`, `scroll_stream`, `stashed_pager_streams`, `find_picker`, `pending_git_view` | global slots, stashed per project | A switch stashes the active project's open pager with its stream, the way a tab switch already stashes a scrollback pager (`stashed_pager_streams` is keyed by stream id, not by tab). | D |
| `worktree_results` | global, with a change | MCP worktree jobs complete off-thread. The result must name the caller's project, so `create_worktree(open: true)` opens column `b` in the agent's project, not the one the user is looking at. | D |
| `listing_refresh_inflight` / `_dirty`, `preview_results` / `preview_reloading` | global | The watcher only watches the active project (§1e), and a stale result is dropped by project plus generation. | D |
| `agent_status_*`, `codex_pin_*` | global | They scan every tab; the scan now walks every project's tabs. | D |
| `autosave_last_saved_fp`, `autosave_due` | global | One manifest (§3). | D |
| the landing slots (`graveyard_results`, `image_results`, `archive_results`, `file_results`, `clipboard_*`, `inventory_results`) | global | Each result carries what it needs; the ones that touch a column carry the project. | D |
| `picker` (terminal graphics capability) | global | Probed from the terminal. | C |
| `pane_wake_tx`, `next_sink_id`, `next_stream_id`, `pane_scroll_settle` | global | Loop plumbing. `pane_scroll_settle` holds a tab index, which becomes `(ProjectId, index)`. | D |

### 1d. `ViewState` (`src/app/view_state.rs:124`)

| field | goes to | why | 7 |
|---|---|---|---|
| `pager`, `pager_right`, `right_pager`, `displaced_preview`, `scroll_pager`, the help stashes | project | Open views over that project's columns. | A (`right_pager.source_path` is R, as today) |
| `pager_history` | project | `[b` / `gp` means "what I was reading here". Reopening a buffer from another project inside this one is disorienting. | A |
| `cached_rows*`, `right_cached_rows*`, `cached_grid_key`, `right_cached_grid_key`, `pages`, `right_pages` | project | Keyed by each column's `list_generation`. | C |
| `overlay_awaiting_dismiss`, `overlay_auto_dismiss`, `overlay_column` | project | Bookkeeping for `ProjectRuntime`'s overlay slots, so they move with them. | T |
| `pending_pager_return` | project | The pager a `v` edit reopens when the editor exits, into this project's slot. | T |
| `context_path`, `last_context`, `context_dirty` | project | One context file per project home (§2). | D |
| `preview_dirty` | global | The re-kick flag of `preview_reloading` (§1c), which is global. | T |
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
accept thread (`start_socket_server`). What becomes per-project is the context the
socket answers from, which the connection's pane selects.

Why not a socket per project: the socket isn't a security boundary (one uid, see
SECURITY.md), and the routing a second socket would provide already exists. Each
agent pane gets `SPYC_MCP_SOCK` and `SPYC_PANE_ID` (`spawn_pane_tab`), the
proxy names the pane in `initialize` (#507), and the connection binds once to a
live tab (`handle_initialize`). A pane belongs to exactly one project, so a
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
   project's tabs (before step 3 it searched the one list) and
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
6. **Unbound connections pick a project by cwd.** A session codex's shared
   daemon started reaches the socket through marker discovery and has no pane;
   an agent started outside spyc gets no tools at all (#619). Its proxy
   adds its cwd to `initialize` (`_meta["spyc/cwd"]`, beside `spyc/paneId`),
   and the server binds the connection to the open project whose home contains
   it, else the active project. That's the same "attribution, not
   authorization" footing as the pane id. The cwd is a routing hint from a
   process the user runs, and SECURITY.md already says which.
7. **Unattributed fallbacks stay "the focused tab"**, meaning the active tab of
   the active project (`resolve_report_target`).

**The same principle, applied further (#619, landed in 2.2).** spyc no longer
writes agent config into working directories. Since #509 the MCP entry names no
socket and the pane's env decides, so one entry in each agent's *user* scope
serves every pane, and the status hooks went with it: nothing per directory,
no refcounted cleanup, and nothing for Projects to route. An agent started
outside spyc sees no tools (owner decision, 2026-10-12, in
[#619](https://github.com/Tripstack-Corp/spyc/issues/619)).

What doesn't change: MCP entries still name no socket (SPYC-TRAP
`mcp-entry-names-no-socket`), so there is nothing to take over. #509 deleted the
takeover machinery the §7 brief asks about. Wiring consent is per agent, not
per project. The orphan sweep already runs
in the state directory (#525), where dead spycs' context files, sidecars and
sockets now live; step 3 taught it the per-project file name. Its pass over the
launch directory only clears markers that older spycs wrote there.

**Landed in step 3, and what the code changed in this design.**

- **The socket server never read `SPYC_CONTEXT`.** It reads one path, fixed
  when it starts, so item 2's per-pane env can't route reads. Instead
  `McpCommand::Bind`, which replaced `PaneContext`, replies with the bound
  project's context file, and `Caller` keeps it beside the project.
  `McpCommand::CallerContext` answers `get_spyc_context`'s `pane` and `project`
  per call.
- **A context file's `root` is its project's `start_dir`, not its home.** That
  was #525's choice, and step 3 kept it. Item 6 matches a cwd against the same
  root the sidecar lists, so binding and discovery agree.
- **Two matchers parse the context file's name:** the orphan sweep and the
  fs-watch filter that ignores spyc's own writes. Both learned
  `.spyc-context-<pid>-<project>.json`.
- **A report with no pane follows its connection's project.** Item 7's
  "focused tab" is the focused tab of the connection's project when it has one
  (bound by cwd), and the active project's only for a connection bound to none
  (the status hook, `spyc --statusline`). A tab number also counts in the
  connection's project.
- **Driving tools run inside `App::in_project`,** which points every handle at
  the caller's project. Whatever belongs to the user's view stays with it: the
  process cwd, frecency, an open harpoon menu and the flash. A call from a
  project that has closed is refused.

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
- **`MAX_SESSIONS = 20`** (`sessions/mod.rs:12`) stays a cap on manifests, not
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

**Landed in step 4, and what the code changed in this design.**

- **A v1 file's home is its `project_home`, with no fallback to its `cwd`.**
  `:project clear` saves no home, and the restored project keeps none; the
  `cwd` becomes its `start_dir`, which anchors it as before.
- **A one-project row reads as today's:** the path, its tab labels and its
  agents' conversations. Every manifest a step-4 spyc writes has one project,
  and the path says more than a label. The project list takes the path's place
  only when a manifest holds several, and `running` gets its own column only
  when a listed manifest is running.
- **A restore adopts the file only when it carries all of it:** one project,
  and no other live spyc writing it. Anything else restores the active project
  as a copy and leaves the file untouched, so a project the restore didn't open
  is never overwritten. Step 5 widens this to "every project opened".
- **A copy of a running session starts its agents fresh** (owner decision): a
  resume would put a second process in the conversations that spyc still has
  open, and `codex resume --last` would pick one even without an id. The copy
  takes none of its tab ids or scope claims either.
- **`running` needs the pid's start time too.** A crashed spyc's pid soon
  belongs to another process, so a pid alone would mark its manifest running
  forever. The manifest records `process_started` beside `pid`; a spyc's own
  file is never running to itself.
- **Column `a` falls back to the anchor.** A gone `left_cwd`, or one that can't
  be listed, lands on the home (else the start directory) instead of failing
  the restore. The restored `start_dir` is the saved one when it still exists.
- **Refused tabs belong to their project** (`Project::deferred_tabs`), and the
  autosave fingerprint covers every project, column `a`'s directory and the
  start directory.
- **Saved project ids aren't adopted.** A restored project keeps the id it was
  given at launch, which keys its runtime state and its context file; a saved
  id only names the active project within its file.

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
| `Space !`, `^a !` | go to the agent that needs you: the oldest blocked tab in any project, then unseen done ones, switching project and tab and focusing the pane | "urgent"; owner decisions, 2026-10-01 and 2026-10-12 (`^a !`) |

**From the pane, the leader is `^a Space`.** `Space` is literal text to the
agent, so while the pane has the keyboard spyc sees only its prefixes
(`is_spyc_meta_when_pane_focused`). `^a` wakes spyc, and `Space` then enters the
leader, so every key above is `^a Space <key>` from inside an agent:
`^a Space !` is three keys. That bridge is the only way a `Tier::Global` action
reaches the pane, and the tier guard keeps it so. The attention jump is the one
key a user reaches for *from* an agent pane, so it is `Tier::Meta` (the tier
`Help` and `Quit` share), which may bind on both prefixes, making it `^a !` as
well: two keys from an agent instead of three (owner decision, 2026-10-12).

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
PR, except step 5's stages:

1. **One `Project`, behaviour-identical.** Introduce `ProjectId`, `Project`,
   `ProjectRuntime` and `ProjectView` holding today's fields, with one project;
   the handles delegate. Extend `columns_are_addressed_through_handles` so only
   project-level code names `projects[…]`.
2. **Route results by project.** Git worker results carry `(ProjectId, Side)`;
   worktree job results carry the caller's project. A git result for a project
   that isn't active is dropped, since only the active project is polled. A
   worktree job's filesystem follow-ups (refresh, orphan reset, context write)
   still run on the active project, while what the caller asked for (`open`
   into `b`, `W n`'s chdir) lands only if the caller's project is still the
   active one. Until step 3 binds connections to projects, a job's project is
   the one active when it started.
3. **Per-project MCP.** A context file per project (the single-root version
   landed in #525), `PaneContext` returning the project, reads and driving tools
   on the caller's project, the sidecar as a list of homes, and `spyc/cwd` for
   unbound connections. Tests land first, against two
   projects in one test app. Landed; the corrections it made are at the end of
   §2.
4. **Manifest v2.** The v1 reader, `left_cwd`, `pid` and the picker labels.
   Landed; the corrections it made are at the end of §3.
5. **Open, close and switch**, in five stages, plumbing first (owner decision,
   2026-10-18). Each stage is a red PR and then its fix. Nothing in production
   creates a second project until 5e, so 5a–5d are tested through the harness's
   own projects (`add_test_project`, `make_active`).
   - **5a. File-op results land in their project.** Landed; see below.
   - **5b. Archive, image and Lua results land in their project, and the
     pending git view moves into `ProjectRuntime`.** Landed in two PR pairs;
     see below.
   - **5c. Every tab is walked again.** Step 1 (#698) narrowed each loop
     over "all tabs" to the active project's. Landed in two PR pairs, pane
     loops and agent-status loops; see below.
   - **5d. The switch.** `App::switch_project` moves the three active ids
     and brings the arriving project up to date. Landed; see below. Until 5e
     binds a key, it carries an `expect(dead_code)` that the first caller
     turns into an error.
   - **5e. Open, close, the keys and the switcher; `-r` opens every project.**
     - **Opening** writes the project's context file and the sidecar with
       every root, builds its keymap and replays Lua `@map:` entries.
     - **Closing** asks the live-children question, shuts down its tabs,
       removes its file, rewrites the sidecar, and switches away first. Step 3
       already refuses a call from a closed project.
     - **The keys** are §4's, and the switcher follows the session picker.
     - **`-r`'s adoption rule** becomes "every project opened".

   **Found while planning step 5**, by an Opus review (2026-10-18):
   - **Already correct:** the fs-watch and the git poll already follow the
     active project, because both read it through the handles. The git outbox
     needs nothing, since `flush_git_requests` drains only the active project.
   - **Moot:** five of the landings the step-2 inventory listed can't
     interleave with a switch. The `F` finder, `!` capture, the `J` default and
     the image gallery's index are modal while in flight. A preview reload
     installs only on the path it read. The inventory and graveyard refreshes
     re-read global stores.
   - **Missing from that inventory:**
     - a pipe to the pane names no tab;
     - a captured image was silently lost;
     - the narrowed tab loops above.

   **Landed in 5a.** `spawn_file_op` tags each result with the project active
   at the spawn, which is the one whose handler asked. The drain applies a
   result for the active project exactly as before. A result for another open
   project goes through `App::land_in`, which applies it in that project and
   shows any flash it raises, prefixed with that project's label. What would
   act on the view in front of the user is dropped and said so: a pipe to the
   active pane, and a listing read whose project activation will re-list
   anyway. That read still settles the single in-flight refresh. A result for
   a closed project is dropped.

   **Landed in 5b (archives).** `spawn_archive_op`, the archive worker's one
   spawn site, tags each outcome the way 5a tags a file op. An outcome for
   another open project runs there through `land_in`:
   - **Entering a mount** moves that project's column. The mount table itself
     is global, so the mount is always recorded.
   - **A member opened in a pager** opens in that project's pager slot.
   - **An archive op the outcome starts** is spawned in that project, so its
     outcome comes back there too: the re-read after a write, a nested mount,
     a staging clean.
   - **A size question or an editor** would act on the view in front of the
     user, so each is dropped and said so. The arms check `in_background`. An
     effect held back for the mount is dropped the same way.
   - **The project's in-flight flash** ends when its outcome lands.

   The mount table is global, so the columns guarding it are too:
   `every_column_dir` lists every open project's columns. Neither eviction
   nor `:archive unmount` pulls a mount out from under any project's column.
   5e has to decide what happens to a project's in-flight archive work when
   the project closes. A `Written` outcome for a closed project is dropped
   today, which leaves its mount's journal pending.

   **Landed in 5b (images, Lua, git views).** `spawn_image_work`, the image
   worker's one spawn site, tags each outcome the same way. For another open
   project:
   - **A decoded image** is dropped and said so. The overlay covers the screen
     and its verbs act on the project in view.
   - **A captured paste** is kept for its tab in that project. It used to be
     kept only for a tab in the active project, and lost otherwise.
   - **A gallery index** reaches the gallery that asked for it, which matches
     it by tab.
   - **A message** (an open in the external viewer, a failure) reaches the
     user named for the project, never that project's pager, which they can't
     see.

   A Lua run records the project it was started in (`runtime.lua_origin`).
   Only a submit with nothing in flight sets it: a key or an event is refused
   while a script runs, and an `init.lua` reload queued behind one must not
   claim that script's requests. The record outlives the first outcome of an
   event's run of handlers, which `lua_inflight` does not. A finished run from
   a project the user has left has its requests dropped and said so, and its
   error named for that project. Its registrations still install; 5e replays
   `@map:` entries into each project's keymap.

   The pending git view lives on `ProjectRuntime`, and the drain reads only
   the active project's, so a diff opened in one project mounts in that
   project's pager when it is next in view.

   **Landed in 5c (pane loops).** `Runtime::every_tab_set` /
   `every_tab_set_mut` walk every open project's tabs. Through them:
   - **The pane drain** stamps, flags and marks exited every project's tabs.
     Only the project in view asks for a redraw, since nothing on screen shows
     another project's tabs.
   - **Teardown** shuts down every project's tabs, and **the quit confirm**
     counts them.
   - **The activity monitor's `panes:` cell** counts every project's tabs.
   - **A terminal resize** resizes each project's tabs to that project's own
     layout (`in_project`), so a child draws at the size it will be shown at.
   - **A restored tab's `/resume`** is sent and its deadlines armed in every
     project, ready for `-r` opening every project (5e).

   The top overlays stay with the project in view: nothing reads them while
   it is hidden. `git_worker_available` is now one flag on `AppState`, since
   one worker serves every project's columns; it used to be per column, set
   only on the first project's `a`, so any other project walked git on the UI
   thread. The harness's `add_test_project` keeps a project's context file
   where production does (`process_context_path`).

   **Landed in 5c (agent-status loops).** The same walk serves an agent in a
   project the user isn't in:
   - **The activity settle** settles every project's tabs (`ActivityScan`
     carries one tab's work). Only the project in view's active tab can be
     the focused one, each project's tabs notify under that project's own
     `[notify]`, and `codex_recovery` is kept for any project's open tab.
   - **The scrape-quiet scan** scans every project's quiet agents.
   - **A shell tab's looks** run in every project, and a change applies in
     its tab's project (`land_in`), since a tab index means nothing across
     projects.
   - **The codex pin** treats every project's tabs as one pool, so a session
     one project's tab holds is never handed to another's.
   - **Wiring** flags every project's tabs for a restart, and `:hooks on!`
     restarts its tab in whichever project holds it.

   The notification still names the tab, not its project: that wording is
   step 6's.

   **Landed in 5d.** `App::switch_project` (`app/project_switch.rs`) moves the
   three active ids and brings the arriving project up to date, since neither
   the watcher nor the git poll looked at it while it was out of view:
   - `AppState::relist_columns` flags every column for a re-walk, which an
     immediate `refresh_git_state` makes, and hands its directory reads to the
     file-op worker (`FileOp::RelistColumns`), since a huge directory or a
     slow mount would stall the keypress. A walk dropped while the project was
     inactive spent its force flag and stashed `git_poll_cache`, so with the
     index and `HEAD` mtimes unchanged the poll alone would reuse the stale
     status. The switch's read lands only in a column still in the
     directory it read: a git result landing first bumps `list_generation`,
     so the generation can't be the check. Installing it bumps the generation,
     so a watcher read started before the switch is either replaced (it landed
     first) or dropped (it lands after).
   - The project left behind has its context file written at once if it is
     behind: its agents read it while the user is away, and the loop writes
     only the active project's, after a typing-burst wait a switch can beat.
   - The split preview reloads, the theme is rebuilt from the arriving
     project's `[colors]` (the user's colours-off toggle is kept), and a
     scrollback snapshot armed in the project left behind is abandoned, as a
     tab switch abandons one.

   What changed in the design:
   - **Nothing is stashed.** The pager stream, the scroll stream and the
     parked streams moved onto `ProjectRuntime`, so a project's streams are
     its own: the drain reads the active project's, and a stream left running
     waits in its channel until its project is next in view. Parked streams
     are pruned against their own project's tabs; before, a tab closing in
     one project dropped a stream parked on another's.
   - **Lua's `dir_changed` and `project_changed` fire behind one busy
     check.** A switch changes both in one settle, and the worker starting
     the first job dropped the second.
   - **A resize lays every project out again**, not only its tabs (5c): its
     top overlay, and the pagers baked for a width (help, about, a git view),
     each in its own project. The split preview's reload slot serves every
     project, so a resize reloads the project in view's and a switch reloads
     the arriving one's.

   **Landed in 5e, before any key opens a project** (#727–#732, then pair
   1a). A repo's `.spycrc.toml` may not name the `[clipboard] command` a yank
   runs, nor give a scan pattern a `url` other than an http(s) page, since
   `Space n` lets a user open any directory; spyc started in `$HOME` reads its
   rc once. Then what has to hold once a project can close:
   - **Homes are unique.** `AppState::home_taken` compares a directory with
     every other open project's home, else its `start_dir`. Each way a
     project's anchor moves checks it: `Space P`, `gP` and `:project <dir>`
     (`set_project_home`), `:project clear`, which falls back to the start
     directory (`clear_project_home`), and for a homeless project `gS` and
     `:startdir` (`move_start_dir`). A start dir is the root an agent's cwd
     binds by, so `move_start_dir` refuses another project's for any
     project. Session restore sets a home unchecked; pair 3's restore of
     every project checks it.
   - **A closed project's archive work.** The mount table is the process's,
     so `apply_archive_outcome_orphaned` applies what an op did to it as it
     would anywhere, and drops what would act on a project. A mount is
     recorded and entered nowhere; an extracted member is recorded as
     spyc's own, so it never reads as an edit; a write clears the journal and
     has the archive read again from clean staging (`request_remount`), that
     read spawned for the closed project so it lands here too
     (`spawn_archive_op_from`); a failed write says so. Two reads of one
     archive never overlap, so a request from another project while one runs
     joins it (`archive/reads.rs`); a re-read in the background never takes
     `:archive cancel` from the user's read (`mount_cancel`), and what it
     says is named for the closed project (`App::land_closed`,
     `AppState::closed_labels`). This answers what step 5's notes left open.
   - **A closed project's agents.** A connection bound to a pane lost its
     tab with the project: once the project's context file is gone, every
     tool and resource answers "the project this agent belongs to has
     closed", where it read from spyc's process cwd, another project's tree.
     A connection bound to no tab, by cwd (a session codex's shared daemon
     runs) or to the active project, outlives the close: it binds again as
     at `initialize`, to the project holding its cwd, else the active one.
   - **Config and Lua for every project (pair 1b).** `reload_config`
     reloads every open project from its own start dir's rc, the one in view
     last, so its theme is in force (another's colours wait for the switch);
     a problem in another project's rc is the flash, named for it. The
     watcher's candidates name every project's rc (`config_parents`), and
     `sync_watch` sends `WatchCommand::SyncConfigParents` when they change,
     as a start dir moves; the worker puts back a config watch a column's
     watch took with it. A `spyc.map` key binds in every open project's
     keymap, and forgetting the registrations (`:lua reload`, `:lua off`)
     strips them from every keymap, so a reload no longer doubles them.
   - **Opening and moving between projects (pair 2a).** `Space n` asks for a
     directory and opens it (`App::open_project`): `create_project` reads its
     rc, binds `init.lua`'s maps (`install_lua_maps`), builds column `a`
     unread with its repo root and branch resolved, and the switch reads it on
     the worker; then its context file is written and its startup tabs start.
     A directory an open project is anchored at or started in is gone to
     instead. `Space Space` goes to `AppState::last_project` (recorded by
     `switch_project`, never by `in_project`), `Space 1-9` to the Nth,
     `Space ]` / `Space [` through them, wrapping; with one project open they
     do nothing. The project keys are `Action::Project(ProjectAction)`, with
     `Space p` / `Space P` moved in. Closing (`Space x`) is pair 2b.
6. **Attention.** The roll-up, the projects segment and its degradation ladder,
   the title, notification text, `Space !` and `^a !`.
7. **The per-project config files** under `~/.config/spyc/projects/` (§1f).
8. **Recently-closed projects in the switcher.**

Steps 1 and 2 are invisible and land first. Steps 3-6 are the release.

---

## Owner decisions at approval

The three questions review left open, answered on 2026-10-12.

1. **The saved thing's name: manifest, and the UI keeps saying session.** The
   3.0 decisions log reserves "session" for agent conversations, but spyc's UI
   already says session for the spice-named save (`-r`'s "session picker",
   `:name`, the `session` segment, `Space s`). This design and the code call
   the file a manifest, and the UI words stay as they are. Renaming words daily
   users already know would be its own change, and nothing in 2.3 needs it.
2. **`^a !` for the attention jump: yes.** `Space !` was already decided. The
   jump is also `Tier::Meta`, so `^a !` reaches it in two keys from an agent
   pane instead of `^a Space !`'s three (§4). Meta otherwise holds help, quit
   and display toggles; the jump joins them because it is the one project key a
   user reaches for from inside an agent.
3. **Agent config in user scope: its own issue** (#619, which then landed in
   2.2). The follow-up in
   §2, one MCP entry and one set of status hooks per agent's user scope instead
   of per directory, is
   [#619](https://github.com/Tripstack-Corp/spyc/issues/619). It changes
   what an agent started outside spyc sees, so it gets a decision of its own and
   isn't part of Projects.
