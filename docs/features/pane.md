# The agent pane

Part of spyc's [feature reference](../../FEATURES.md). The terminal below the file list, its tabs, and the ways text moves between it and spyc.

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
spyc's workflow: browse files above, talk to Claude below.

The pane is a terminal in its own right, not a pass-through to the host. A
program that redraws inside a synchronized update (DEC mode 2026) is shown only
once the update is finished, so brew's download list doesn't flicker. A program
that asks before choosing what to emit gets answers about the pane rather than
the host: device attributes, mode reports, cursor position, and an XTVERSION
naming `spyc`. That is how claude learns it can use synchronized output in any
host terminal. The pane answers nothing it can't back: no kitty keyboard
protocol, no image protocols, no OSC 52 clipboard.

- **^\\ / F10** toggle the pane open/closed
- **F9** open pane with `claude --resume`
- **^a j / ^a k** switch focus between the file list and the pane
  (`^w` also works as an alias for `^a`)
- **^a s** send the current selection (file paths) to the pane as stdin,
  anchored on that pane's working directory, read at the moment you send.
  A path under it goes out relative, so an agent working in its own
  worktree can open it as typed. Anything else goes out absolute, never
  `~`-collapsed. If the pane's directory can't be read, every path goes out
  absolute. A directory goes out with a trailing `/` (`src/`, `./`); a
  symlink to one goes out without it.
- **^a ↓** send a literal `^a` to the pane — the prefix is otherwise
  unreachable by the child, but Claude binds `^a` (e.g. to expand notes),
  so this is the tmux-style "send-prefix" escape hatch
- **^a P** pipe file contents of the selection to the pane
- **^a i** pipe inventory file contents to the pane
- **^z** suspend/resume the pane's child — an agent (claude/codex/…) or any
  other command (`htop`, `vim`, a long build). spyc sends `SIGSTOP` to the
  pane's process group itself and shows 💤 on the divider; press `^z` again to
  `SIGCONT` it back. (It manages the stop rather than forwarding the key: a
  full-screen program runs its tty with signal generation off, so a forwarded
  `^z` is a byte it discards — and Claude, which does catch `^z`, had a handler
  that on macOS looked like an exit and got the pane killed. `SIGSTOP` is
  uncatchable, so it just freezes.) The one exception is a **shell** pane —
  `zsh`, `bash`, `sh` — whose `^z` is forwarded so its own job control keeps
  backgrounding the foreground job
- **^a + / ^a -** grow or shrink the pane
- **^a z** zoom the **active** region (toggle fullscreen). Pane focused →
  the pane fills the screen (list collapses to 0 rows, `[ZOOM]` on the
  divider) and a single spyc status line stays at the top (status, or
  flash / chord-arming / prompt when active). List focused → the list fills
  the screen with the pane **tab bar kept at the bottom** (`[ZOOM]` in the
  top status bar); from there `^a <n>` fullscreens that tab, and creating a
  new pane reveals the split. Focus stays where it is, `^a j` / `^a k` are
  inert while zoomed (only `^a z` exits), and the prior split is restored on
  un-zoom.
- **Agent-activity dots** — each **agent** pane tab shows a live activity dot
  in the divider, from these layered sources:
  - **Output timing** (no hooks, no screen-scraping): a **spicy heat-pulse `●`**
    — an orange → spark colour *breath*, eased in 16 small
    steps over 1.6 s (10 Hz), a stepped warm ramp on a 256-colour terminal — while
    output is flowing, fading to a quiet `·` once the agent goes silent. The
    pulse animates only while something is working, so a fully-idle pane set
    still renders at 0 fps.
  - **Semantic self-report** via the `report_status` MCP tool: a cooperative
    agent (Claude, codex, …) tells spyc when it's `working` (holds the pulse
    through a silent thinking pause — no false "idle"), `blocked` (a steady
    **hot-red square `■`** — the "which agent needs me" signal), `done` (a
    calm teal square `■`), or `waiting` (a steady blue hollow circle `○`: the
    turn is over, but background work the agent started will wake it). Shape
    carries meaning: a **circle `●`** is live / animated (working), a **square
    `■`** is a *settled* state that is your turn (blocked or done), and a
    **hollow `○`** is not working this moment but not your turn either — so you
    can tell "needs me / finished" from "busy" and "waiting on CI" at a glance.
    Claude Code's turn-end hook lists the background work still running, so its
    `waiting` needs no report; other agents report it, with a note saying what
    they wait on, which `:why-status` shows. A `waiting` an agent reports
    before its turn has ended shows once the turn ends; until then the dot
    stays working. Entering `waiting` pings nothing.
    For Codex and Claude, live reports survive output and footer redraws until
    a newer report or TTL expiry; quiet tool waits do not erase `working`.
    Agy yields non-blocked reports to fresh output for uncovered approvals.
    Codex's complete command, file-edit, MCP-tool and network approval forms
    temporarily override a non-blocked report while retaining it for recovery. **`blocked` is latched**: it stays a steady red
    square — no TTL, no output or animation revives it — until you actually
    answer the pane by pressing **Enter** in it (or the agent files a newer
    report). Identified Codex questions instead wait for their matching tool
    completion; Enter alone does not clear them. Non-agent tabs (a plain shell)
    get no dot.
  - **Auto-reporting (claude + codex; agy partial)** — so it works without the
    agent choosing to call the tool, spyc installs lifecycle hooks (prompt-submit
    → working, needs-permission/approval → blocked, turn-end → done) that run
    `spyc --report-status`. They go into each agent's own user configuration,
    once, for every project: **claude**'s `settings.json` (reloaded live),
    **codex**'s `$CODEX_HOME/hooks.json` (read at startup, so a codex already
    running picks them up on its next launch). Codex's metadata-bearing `PermissionRequest` is observational: it fires
    before automatic or human review, so it cannot establish a human wait.
    Known command, file-edit, MCP-tool and network forms require their default
    choices and complete footer at the viewport bottom; diagnostics identify
    this `scrape-fallback`. Missing required text and other forms produce no guess. Codex's `Interrupt` hook
    reports `idle` after cancellation. Complete default async-question editors
    and collapsed question summaries also show `blocked` while the tab needs an
    answer, even if Codex keeps working. Clearing the native view retires that
    fallback and resumes the latest live report; clipped/customized views are
    not guessed. Its `request_user_input` hooks report
    `blocked` on start and recover `working` only for the same pending
    pane/session/turn/call; other waits stay blocked. Project trust and `/hooks`
    review are separate from spyc's consent; spyc never auto-approves hooks. **agy** (Antigravity)
    gets a `spyc-status` set in `~/.gemini/config/hooks.json` and is **partial** — it
    covers `working`, `done`, and `blocked` for agy's own `ask_question` tool
    (a `PreToolUse` hook sees that one exactly when it's called). The *other*
    way agy waits on you — its tool-permission prompt — fires no event in any
    agy version, so that half of the red square is read off the pane instead.
    `done` needs **agy ≥ 1.1.10**, which is where `Stop` hooks became reachable
    at all; on an older agy that half falls back to output timing. Full
    mechanics: [`docs/AGENT_ORCHESTRATION.md`](../AGENT_ORCHESTRATION.md).
    **It asks first**, once per agent: the first `claude`/`codex`/`agy` launch
    pops a `[Y/n]` ("show this agent's live status and give it spyc's tools?"),
    and the answer covers every project — it never asks again for that agent.
    `y` adds the hooks and the agent's `spyc` MCP entry to its own configuration;
    nothing is written into your projects, and nothing is removed at exit. The
    write keeps your own settings and key order. Only `y` or `n` answers it;
    **any other key defers** — the popup closes, nothing is saved, and the next
    launch asks again. It has to work that way because the popup appears once
    the pane already has focus, so the first words of a message to the agent
    would otherwise be swallowed until one of them contained a `y`, which would
    then answer it. **`:hooks`** lists each agent's answer, and
    **`:hooks on|off|ask <agent>`** changes it later (the active tab's agent when
    none is named): `on` installs, `off` takes spyc's part back out, `ask` forgets
    the answer. If a claude live reload doesn't take, **`:hooks on!`** also
    restarts the active claude pane and resumes the conversation so the hooks
    load from launch. spyc writes nothing into a project; copies a version
    before 2.2 wrote there can be deleted by hand (`INSTALL.md`).
  - **Scrape fallback** (for a state an agent's hooks can't report) — spyc
    reads the **visible screen** for verified approval prompts. Agy exposes no
    approval event. Codex's permission event precedes both automatic and human
    review, so only a complete default command, file-edit, MCP-tool or network
    form establishes its user wait. That dialogue temporarily overrides a non-blocked report;
    semantic question/agent blocks retain precedence. Codex scans within 250 ms
    of the first pending repaint, so continuous redraws cannot hide a wait;
    other agents wait for quiet output. Codex requires several prompt phrases
    and the exact modal footer at the viewport bottom, accepting native word
    wrapping and excluding old dialogues above the normal composer.
    Prompt-settling input clears the detected wait.

  **`:why-status`** opens the active tab's diagnosis in the pager, like
  `:why-git`: its state and source (self-reported / scrape-fallback /
  output-timing) and seconds since last output first, then the tab's
  `:activity dump` report, with each hook setup fact on its own line. A
  tab that isn't an agent says it has no dot. **`:activity dump`** retains each pane's last received report even
  after expiry, distinguishes hook-file presence from startup/restart facts,
  and never treats a self-report or bound MCP connection as proof of hook
  execution or trust.
  Each pane also retains up to eight reported hook-event summaries (event,
  tool and available turn/call ids), without arguments, prompts or responses.
  These are claimed metadata, not verified execution; an older reporter may
  omit them. `--status-trace` logs the same sanitized metadata rather than raw
  hook input. Metadata does not infer that a permission/question was answered.
  **`:why-git`** opens a saveable pager dumping each column's git-marker refresh
  state — repo root / resolved gitdir, the cached poll key vs the live on-disk
  one (`index`'s mtime plus the latest of `HEAD`, its branch ref, and the shared
  `config`, so you can see whether the next 1 Hz poll SHORT-CIRCUITS or
  re-walks), that config's own mtime, and the displayed marker count. The tool
  for "why is this column showing stale git markers" — capture it the moment
  markers look wrong.
- **Agent notifications** (`[notify]` config) — the "which agent needs me" ping,
  fired the instant a pane transitions (0 delay, not a timer):
  - **Desktop notification** naming the tab (e.g. *"codex needs you — tab 2 is
    blocked"*) on `Blocked` and `Done`. **On by default.** `desktop_via` picks
    how it's delivered — default **`"auto"`**: an **OSC-9 terminal escape over
    SSH** (so it pops on your *client* terminal, where your eyes are) and the OS
    notifier (`notify-rust`) locally. Force it with `"system"` (OS notifier — the
    machine spyc runs on), `"osc9"` (terminal escape — needs iTerm2/kitty/WezTerm/
    …), or `"both"`. Set `desktop = false` to silence, or `desktop_done = false`
    to be pinged only when an agent is *blocked* (not on every finished turn).
  - **Terminal bell** (`bell = true`, off by default) — rings alongside the
    notification.
  - **Visual bell** (`visual = false` to opt out; **on by default**) — a brief
    spice-heat **border pulse** (spyc's pepper→ember→orange→spark gradient)
    sweeping around the whole frame; the branded, non-reflowing attention flash.
  - **Blocked-only by default for the intrusive channels.** `Blocked` ("needs
    me") fires every enabled channel; the routine `Done` (once per *finished
    turn*) fires only channels that opt in. The quiet desktop ping does
    (`desktop_done`, on) but the interrupting bell and on-screen flash stay
    `Blocked`-only so they don't ring/strobe every turn — flip `bell_done` /
    `visual_done` to fire those on `Done` too.
  - **`suppress_focused_tab`** (**off by default**) stays quiet when the
    transitioning tab is the one you're already watching. Off by default because
    spyc's keyboard focus doesn't mean your eyes are on the terminal — with the
    agent pane focused you're usually working in another app while it runs, so
    the "needs me" / "done" ping is exactly when you want it. Set it true to mute
    the on-screen tab.
  - **`:notify test`** fires every channel on demand (bell + visual + both desktop
    mechanisms), bypassing the config gating — to verify your setup without waiting
    for a real agent transition.
- **^s |** (alias **^a |**) vertical (left/right) split of the file area. Opens
  (50/50) with a **preview of the file under the cursor** (markdown rendered) in
  the right column. Press it again **on a different file** to swap the preview to
  that file, keeping the layout; **on the same file** it closes. A directory
  isn't previewable (it warns).
- **^s f** (alias **^a f**) flips the open split's **height**: *full-height* (divider runs the
  whole frame height; the pane is confined under the left column) ⇄ *top-only*
  (splits just the list; the pane stays full-width below both columns). Width and
  focused column carry across. The shape a split *opens* in is
  **`[layout] vsplit_mode`** — `"full_height"` by default, `"top_only"` for the
  old half-height open. The `^a |` / `^a f` aliases exist because the **pane**
  intercepts only `^a`-family keys, so `^s` alone would leave the split
  unreachable while you're typing to an agent. `^a a` / `^a h` focus the left pane
  (a), `^a b` / `^a l` the right (b); `^a + / ^a -` resize the split width when
  a column is focused. Opinionated: exactly two file panes, labelled a/b
  (numbers stay for PTY tabs). The preview **live-reloads**: edit the file
  (in spyc's editor, an agent pane, or any external editor) and the rendered
  markdown updates on save — the re-render runs off-thread, preserving your
  scroll position, and re-wraps when you resize the column.
- **^s n** open a **second full file-commander** in the right column
  (`b`) — a complete browser, not just a preview: its own cwd, listing,
  picks, filter, sort, git markers, and harpoon. It opens *top-only*
  whatever `vsplit_mode` says (the pane stays full-width below two peer
  browsers); `^s f` makes it full-height. **^s x** closes it.
  (`^d` does *not* close it — it quits, so `^d^d` with `b` open lets the
  session save + `-r` restore the split.)
  `^a a`/`^a h` focus the left commander (a), `^a b`/`^a l` the right
  (b). Everything follows the focused column: navigation, `V`/`D`
  overlays, file ops (`O`/`+`/`W`), `!`/`;`/`$` shell cwd, the MCP
  context, and the per-column tools — **grep `F`**, **find**, **MCP
  search**, and **harpoon** all scope to the focused column's *worktree
  root* (so `b` in a separate worktree searches its own tree, with its
  own pinned harpoon list). **g w** jumps the focused column to its
  worktree / repo root; `PROJECT_HOME` (jumped by `Space p`) stays the
  overall anchor. Git markers in each column refresh on filesystem
  events independently. (The second-commander chord is `^s`, not `^z`:
  `^z` to a shell in the bottom pane is SIGTSTP — it'd background a
  running job. `^s` is at worst terminal flow-control XOFF, which spyc's
  raw mode clears; over a flow-control-sensitive SSH/serial link it may
  still be intercepted.)
- **W l** opens a **worktree picker** — `j`/`k` (or arrows) move the
  highlighted row, `Enter` switches the *focused* column to it, `/`
  searches (the cursor lands on the match), and `1`-`9` quick-switch by
  number (focus `b` first to put a worktree there). Rows list the main
  worktree first (git's ordering), each showing the branch checked out
  in it; the main one is marked **`(main worktree)`** and the one the
  focused column is in `← current`. The marker is what tells you which
  row is your git home when a feature branch is checked out there
  directly — the branch column can't. With the MCP tools
  (`create_worktree` / `open_worktree` /
  `remove_worktree` / `clean_worktree`) an agent can spin up a worktree,
  open it in `b`, work, and tear it down — while `a` stays on the main
  branch.
- **^a v** enter scrollback view — browse up to 10K lines of pane
  history in the **in-app pager** (v1.5). Line numbers are on by
  default here (a gutter the live pane never shows, so it reads at a
  glance as scrolled-back rather than live; `l` toggles). All pager
  keys work: `/` search with `n` / `N`, `:N` jump, `V` visual line,
  `^v` visual block, `y` yanks the selection, `l` toggles line
  numbers, `w` whitespace markers, `T` swaps between terminal
  capture and an agent's transcript, and (in an agent transcript)
  `t` toggles tool-call lines. The pty keeps running off-screen — output
  you miss while reading lands in scrollback for the next view. `Esc`
  snaps back to live — and so does scrolling *down past the end*, so a
  flick out the bottom returns you to the agent rather than parking you
  at `[EOF]`.
  - The fundamental limit is that full-screen TUIs do *virtual
    scrolling* inside a fixed grid — old content lives in app
    memory, not the terminal — so even a parallel terminal parser
    can't recover it. `^a v` on a *non-agent* alt-screen app
    (vim, htop, lazygit) is therefore a dead end: it flashes a
    hint pointing at the app's own history viewer.
  - **Agent transcripts.** Agents that keep an on-disk JSONL
    transcript — Claude Code, codex
    (`~/.codex/sessions/.../rollout-*.jsonl`), and Antigravity
    (`agy`, under `~/.gemini/antigravity-cli/`) — get the actual
    conversation instead of a screen capture: user turns, agent
    replies, and tool calls rendered in the pager, titled
    `(transcript)` rather than `(history)`.

    Codex reads current structured message/command/MCP records and legacy
    events, including custom-tool calls. Repeated records with matching
    identities are shown once per session; instruction injections and reasoning
    records are excluded.

    Tool calls are
    labelled with their salient argument — `⚙ Bash(Find foo call
    sites)`, `⚙ Edit(src/lib.rs)` — and each result shows a dim
    one-line output preview with a `(+N lines)` count. The
    transcript is
    resolved and read **off-thread** (no UI stall on a big
    session), and **`r`** reloads it — the agent keeps appending
    while you read. If no transcript exists yet (brand-new
    session), spyc flashes a hint.
  - **The transcript wins whenever the terminal has nothing.** An
    agent with a transcript running on the alternate screen (e.g.
    Claude Code's full-screen mode) has no terminal capture at all,
    and neither does one whose pane hasn't scrolled anything off
    yet — same situation, so `^a v` engages the transcript in
    both, config gate bypassed.
  - **`T` swaps the source.** With both available, `T` flips the
    open scrollback between the terminal capture and the agent's
    transcript, and back; the choice sticks for that tab (so `r`
    reloads what you picked) and is dropped when the view closes.
    This is what inline Claude Code needs: both sources are real
    there, and only the capture holds what the shell printed
    before the agent started.
  - **Inline panes are config-gated.** When the agent runs inline
    (output scrolls into the main buffer) *and* a capture exists,
    the verbatim terminal capture works, so which one `^a v`
    opens first is a per-agent choice:
    `[pane] claude_transcript_scrollback` (off by default —
    inline claude captures fine from the terminal) and
    `[pane] agy_transcript_scrollback` (on by default). Codex
    transcripts are always on — there is no config gate.
  - **`t` toggles tool calls.** In a transcript scrollback, `t`
    shows / hides the agent's tool-use and tool-result lines (the
    `⚙`/`└` entries) so you can skim just the conversation prose,
    then bring the tool activity back. Shown by default; the choice
    persists across `^a v` re-opens for the session.
  - **`H` opens the scrollback's own help.** The transcript view has
    its own help (the keys above + a blurb on the transcript engine),
    shown in the bottom pane; pressing `H` again toggles to the full
    pager-keys help (separate but linked), and `Esc`/`q` returns to the
    scrollback. (`H` is a no-op in the *generic* overlay pager only when
    it's a bottom scrollback — there it's this transcript help.)
- **Ctrl+J** newline in pane (multi-line input for Claude CLI)
- **gf** jump to a file path referenced in pane output; **gF** also
  opens the pager at the referenced line. Scans the last 200 lines of
  output (including scrollback) so paths in large diffs are still found.
  A path wrapped onto the next row — by the terminal, or by the agent's
  own line wrapping — is put back together. In a claude pane the scan
  stops at its input box, so the status line under it (which names
  `CLAUDE.md`) can't outrank what claude printed. A relative path resolves
  against the pane's cwd, then its worktree root (a `cargo` run from `src/`
  prints paths relative to the root), then the focused column's directory
  and its worktree root, then PROJECT_HOME; the newest line that resolves
  under any of them wins. `J`'s default and `^a u`'s uppercase open use the
  same order.

Codex tabs capture their launch `CODEX_HOME` (default: `$HOME/.codex`) after
shell rc loading, including runtime `:setenv` overrides. Explicit per-pane
homes are reapplied after rc loading. That home stays attached to the pane
through restart, session save/restore and fork. Transcript lookup uses its exact
session id in that home; missing data keeps the pane live with a diagnostic.
Discovery scopes claims by home and waits for an explicit id when simultaneous
launches or unnamed resumes have ambiguous ownership.

### Multi-tab

Multiple tabs, each running an independent pty:

- **^a c** new tab (prompts for command and working directory)
- **^a K / ^a x** close the active tab — confirms first (`y`/`N`) when the
  tab's child is still running, so a stray keystroke can't kill a live agent
  session; an already-exited tab closes silently
- **^a 1..9** switch to tab N
- **^a P** pipe selected file contents to the pane. Content over 8 MiB asks
  for confirmation of its size (`y`/`Y` sends, other keys cancel). A confirmed
  large pipe needs an empty input queue and stays together as one bracketed
  paste; later input is refused until that batch finishes writing. Ordinary pastes
  over 8 MiB are refused with a size-limit error.
- **^a p / ^a [** prev tab
- **^a n / ^a ]** next tab
- **^a ^a** jump to the last-active tab (screen/tmux "last window")
- **^a r** rename the active tab
- **^a R** restart the active tab — respawns the same command in the same
  working directory, **in place**, so the tab keeps its number (and its `^a r`
  rename). Confirms first (`y`/`N`) when the child is still running, for the
  same reason `^a x` does; an already-exited tab restarts silently. The new
  child is a new process, so its `SPYC_PANE_ID`, agent conversation, and
  scrollback all start fresh
- **^a F** fork the active tab — its conversation carries on in a new tab as a
  branch, and the original stays where it was. claude branches with
  `--resume <id> --fork-session` and codex with `codex fork <id>`: a new
  session that starts from the old one's history, which the agent replays on
  screen and `^a v` reads back. Each needs a conversation on disk first, so a
  tab that hasn't had a prompt has nothing to fork yet. agy and zot can resume
  a conversation but not branch it, so `^a F` says so rather than opening one
  conversation in two tabs. Codex preserves launch options and quoting even
  after an old `resume`/`fork` selector; ambiguous commands are refused rather
  than launching with different settings. A tab running anything else forks
  into a copy of its command, opened at the tab's current directory. `docs/HARNESS.md` §4
  has the per-agent detail
- Activity indicator (**+**) on background tabs that have new output
- **Startup tabs**: `[pane] tabs = ["claude", "zsh"]` (or `[[pane.tab]]`
  tables with a per-tab `cwd`, `label`, and `agent` for a command that wraps
  one, like `npx @openai/codex`, which then runs as written but is that
  agent's tab) opens those tabs at launch, the
  config-driven version of pressing `^a c` once per tab. `spyc -r` restores
  its saved tabs instead. Yours in `~/.spycrc.toml` open unasked. A
  project-local `.spycrc.toml` list runs only after you approve it in a
  pop-up (`y` run, `n` never, `Esc` not now). The approval binds to the
  exact commands and cwds, so an edited list asks again. `:startup-tabs`
  shows the answer, and `:startup-tabs forget` clears it. Reference:
  `CONFIGURATION.md` → "Startup tabs"
- When the tabs overflow the divider, labels crop so every tab stays visible
  and clickable
- **Default command** for `^a c` resolves in this order:
  `$SPYC_PANE_CMD` env var → `[pane] default_command` in
  `.spycrc.toml` → built-in `"claude"` fallback. Switch your daily
  driver to codex (or anything else) by adding
  `[pane]\ndefault_command = "codex"` to your config.
- **Default cwd** for a new pane (`^a c` and bare-spawn / `F9 resume`)
  follows `[pane] new_tab_cwd`: `"worktree_root"` (default) anchors it
  to the focused column's worktree/repo root — `gw`'s target, and the
  column `^a k` returns to, so a pane opened while browsing a linked
  worktree lands there; `"project_home"` pins it to `PROJECT_HOME`
  regardless of the focused worktree; `"browse_dir"` opens it in the
  focused column's current dir.

## Quick Select — labelled overlay picker

Borrowed from [WezTerm's Quick Select][wezterm-qs]. Press **`^a u`**
to scan the visible pane for matches (URLs, file paths, git SHAs,
IPv4 addresses, plus any custom regexes from `.spycrc.toml`); each
match is overlaid with a 1- or 2-letter label.

[wezterm-qs]: https://wezterm.org/quickselect.html

- **Lowercase label** — yank the match to the clipboard, exit.
- **Uppercase label** — "open" intent:
  - **URL** → hand to the system handler (`open` on macOS,
    `xdg-open` on Linux)
  - **Path** → cursor-jump in spyc (chdir to parent + place cursor)
  - **Git SHA** → open the commit in the in-house gix-backed
    `show` view (same pager as `gd`; `|` toggles layout)
  - **Custom pattern** with a `url = "..."` template → fill `{}`
    with the match, then `open`/`xdg-open`
  - Other kinds (IPv4, custom without template) → fall back to
    yank with a flash hint
- **`q` / `Esc`** — exit without action

Scroll mode just works: scroll up to a Claude reply, hit `^a u`,
the URLs in *that* reply get labels (the picker scans exactly the
visible viewport at the user's scroll position).

Custom patterns in `.spycrc.toml`:

```toml
[[scan.patterns]]
name = "jira"
regex = '[A-Z]+-\\d+'
url = "https://tripstack.atlassian.net/browse/{}"   # optional
```

Without `url`, uppercase falls back to yank+hint. Bad regexes are
dropped at config load and noted in the debug log; a typo never
prevents spyc from starting.
