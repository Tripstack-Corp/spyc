# The agent pane

Part of spyc's [feature reference](../../FEATURES.md). The terminal below the file list, its tabs, and the ways text moves between it and spyc.

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
spyc's workflow: browse files above, talk to Claude below.

The pane is a terminal in its own right, not a pass-through to the host. A
program that redraws in one go is shown once the redraw is finished, and a
program that asks what the terminal supports gets answers about the pane, not
the host, so claude's and brew's redraws don't flicker even in a host terminal
that can't do this itself. The pane doesn't offer the kitty keyboard protocol,
image protocols or OSC 52 clipboard writes. How it answers:
[ARCHITECTURE.md → "Answering the child's queries"](../../ARCHITECTURE.md#answering-the-childs-queries).

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
- **^z** suspend/resume the pane's program — an agent (claude/codex/…) or any
  other command (`htop`, `vim`, a long build) — with 💤 on the divider while
  it's stopped. spyc stops it itself rather than passing the key on, because a
  full-screen program ignores a forwarded `^z`. The one exception is a
  **shell** tab (`zsh`, `bash`, `sh`): its `^z` goes to the shell, so job
  control still backgrounds the foreground job
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
- **Agent-activity dots** — each **agent** tab shows a live dot in the
  divider:
  - a pulsing **`●`** (an orange-to-spark breath) while it's working;
  - a steady hot-red **`■`** when it's **blocked** on you, and a calm teal
    **`■`** when it's **done**: a square is a settled state, and your turn. A
    done that landed while you weren't looking at its tab stays teal until
    you are, even after its report times out;
  - a blue hollow **`○`** when it's **waiting** on background work it started
    (a CI run, a build) that will wake it. `:why-status` shows what it said it
    waits on, and entering `waiting` pings nothing;
  - a quiet **`·`** when it's idle. A plain shell tab gets no dot.

  The pulse animates only while something is working, so an idle spyc draws
  nothing. **`blocked` stays red** until you answer the pane by pressing
  **Enter** in it, or the agent reports again; a Codex question stays red until
  that question is answered.

  Where the state comes from, best first: the agent's own reports, through
  spyc's `report_status` tool and the status hooks spyc installs (claude and
  codex fully; agy partly, and its `done` needs agy ≥ 1.1.10); for a wait an
  agent can't report, such as agy's tool-permission prompt or a Codex approval
  dialogue, a verified prompt read off the screen; and failing both, output
  timing. The mechanics, per agent:
  [`docs/AGENT_ORCHESTRATION.md`](../AGENT_ORCHESTRATION.md).

  **It asks first**, once per agent: the first `claude`/`codex`/`agy` launch
  pops a `[Y/n]` ("show this agent's live status and give it spyc's tools?"),
  and the answer covers every project. `y` adds the hooks and the agent's
  `spyc` MCP entry to its own configuration, keeping your settings and key
  order; nothing is written into your projects, and nothing is removed at
  exit. Only `y` or `n` answers it; **any other key defers** and the next
  launch asks again, because the popup appears with the pane already focused
  and would otherwise swallow the start of what you typed to the agent. Codex
  reads its hooks at startup, so a codex already running picks them up on its
  next launch, and its own project trust and `/hooks` review still apply:
  spyc never approves hooks for you. **`:hooks`** lists each agent's answer,
  and **`:hooks on|off|ask <agent>`** changes it (the active tab's agent when
  none is named): `on` installs, `off` takes spyc's part back out, `ask`
  forgets the answer. If a claude live reload doesn't take, **`:hooks on!`**
  also restarts the active claude pane and resumes the conversation. Copies a
  version before 2.2 wrote into a project can be deleted by hand
  (`INSTALL.md`).

  **`:why-status`** opens the active tab's diagnosis in the pager: its state,
  where that came from (self-reported, scrape-fallback or output-timing), the
  seconds since its last output, and its hook setup. **`:activity dump`** has
  the same for every pane, with the last report each one received and its
  recent hook events. It keeps an event's name, tool and ids, never its
  arguments, prompts or responses, and treats them as what the agent claimed,
  not proof that a hook ran; `--status-trace` logs the same.
- **Agent notifications** (`[notify]` config) — the "which agent needs me" ping,
  fired the instant a pane transitions (0 delay, not a timer):
  - **Desktop notification** naming the tab (e.g. *"codex needs you — tab 2 is
    blocked"*) on `Blocked` and `Done`, and its project when several are open
    (*"codex in web needs you — web: tab 2 is blocked"*). **On by default.** `desktop_via` picks
    how it's delivered — default **`"auto"`**: an **OSC-9 terminal escape over
    SSH** (so it pops on your *client* terminal, where your eyes are) and the OS
    notifier locally. Force it with `"system"` (OS notifier — the
    machine spyc runs on), `"osc9"` (terminal escape — needs iTerm2/kitty/WezTerm/
    …), or `"both"`. Set `desktop = false` to silence, or `desktop_done = false`
    to be pinged only when an agent is *blocked* (not on every finished turn).
  - **Terminal bell** (`bell = true`, off by default) — rings alongside the
    notification.
  - **Visual bell** (`visual = false` to opt out; **on by default**) — a brief
    spice-heat **border pulse** (spyc's pepper→ember→orange→spark gradient)
    sweeping around the whole frame; the branded, non-reflowing attention flash.
    When it rings for another project, a flash says where (*"web: tab 2 is
    blocked"*).
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
- **^a v** enter scrollback view — browse up to 10K lines of pane
  history in the **in-app pager**. Line numbers are on by
  default here (a gutter the live pane never shows, so it reads at a
  glance as scrolled-back rather than live; `l` toggles, and
  `[pager] scrollback_line_numbers = false` opens without them). All pager
  keys work: `/` search with `n` / `N`, `:N` jump, `V` visual line,
  `^v` visual block, `y` yanks the selection, `l` toggles line
  numbers, `w` whitespace markers, `T` swaps between terminal
  capture and an agent's transcript, and (in an agent transcript)
  `t` toggles tool-call lines. The pty keeps running off-screen — output
  you miss while reading lands in scrollback for the next view. `Esc`
  snaps back to live — and so does scrolling *down past the end*, so a
  flick out the bottom returns you to the agent rather than parking you
  at `[EOF]`.
  - A full-screen program that isn't an agent (vim, htop, lazygit)
    keeps its history in its own memory, not the terminal's, so
    `^a v` can't show it: it flashes a hint pointing at the
    program's own history view.
  - **Agent transcripts.** Agents that keep an on-disk JSONL
    transcript — Claude Code, codex
    (`~/.codex/sessions/.../rollout-*.jsonl`), and Antigravity
    (`agy`, under `~/.gemini/antigravity-cli/`) — get the actual
    conversation instead of a screen capture: user turns, agent
    replies, and tool calls rendered in the pager, titled
    `(transcript)` rather than `(history)`.

    A codex transcript shows its messages, commands and tool calls, each
    once; its injected instructions and reasoning are left out.

    Tool calls are
    labelled with their salient argument — `⚙ Bash(Find foo call
    sites)`, `⚙ Edit(src/lib.rs)` — and each result shows a dim
    one-line output preview with a `(+N lines)` count. The
    transcript loads in the background, so a big session doesn't
    stall spyc, and **`r`** reloads it — the agent keeps appending
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
- **^a t** open the active `agy` tab's thinking history in the
  **in-app pager**, in the bottom pane: each turn's prompt (`❯ …`), then
  the reasoning agy recorded for it, oldest first, without its replies or
  tool calls. A long session shows its newest turns under a line saying
  older ones were left out. `r` reloads while the agent keeps running;
  `Esc` (or scrolling down past the end) returns to the live pane.
- **Ctrl+J** newline in pane (multi-line input for Claude CLI)
- **gf** jump to a file path referenced in pane output; **gF** also
  opens the pager at the referenced line. Scans the last 200 lines of
  output (including scrollback) so paths in large diffs are still found.
  A path wrapped onto the next row — by the terminal, or by the agent's
  own line wrapping — is put back together. In a claude, codex, or agy pane
  the scan stops at its input box, and in a codex or agy pane at a live
  approval dialog too, so the draft and status line under it can't outrank
  what the agent printed. A relative path resolves
  against the pane's cwd, then its worktree root (a `cargo` run from `src/`
  prints paths relative to the root), then the focused column's directory
  and its worktree root, then PROJECT_HOME; the newest line that resolves
  under any of them wins. `J`'s default and `^a u`'s uppercase open use the
  same order.

A codex tab keeps the `CODEX_HOME` it launched with (default `$HOME/.codex`,
after your shell rc and any `:setenv`) through restart, save and restore, and
fork, and finds its transcript there; how spyc tells two codex launches apart is
in [`docs/HARNESS.md`](../HARNESS.md).

### Multi-tab

Multiple tabs, each running an independent pty:

- **^a c** new tab (prompts for command and working directory). `%` in the
  command expands to the selection as it does for `!`, each path named from
  the tab's working directory, so `tail -f %` follows the highlighted file;
  `%%` is a literal `%`.
- **^a K / ^a x** close the active tab — confirms first (`y`/`N`) when the
  tab's child is still running, so a stray keystroke can't kill a live agent
  session; an already-exited tab closes silently
- **^a 1..9** switch to tab N
- **^a P** pipe selected file contents to the pane. Content over 8 MiB asks
  for confirmation of its size (`y`/`Y` sends, other keys cancel). A confirmed
  one is refused while other input is still queued for the pane; otherwise it
  goes as one paste, and the pane takes no other input until it's written. An
  ordinary paste over 8 MiB is refused.
- **^a p / ^a [** prev tab
- **^a n / ^a ]** next tab
- **^a ^a** jump to the last-active tab (screen/tmux "last window")
- **^a r** rename the active tab
- **^a R** restart the active tab — respawns the same command in the same
  working directory, **in place**, so the tab keeps its number (and its `^a r`
  rename). Confirms first (`y`/`N`) when the child is still running, for the
  same reason `^a x` does; an already-exited tab restarts silently. The new
  child is a new process, so its agent conversation and scrollback start
  fresh
- **^a F** fork the active tab — its conversation carries on in a new tab as a
  branch, and the original stays where it was. claude branches with
  `--resume <id> --fork-session` and codex with `codex fork <id>`: a new
  session that starts from the old one's history, which the agent replays on
  screen and `^a v` reads back. Each needs a conversation on disk first, so a
  tab that hasn't had a prompt has nothing to fork yet. agy and zot can resume
  a conversation but not branch it, so `^a F` says so rather than opening one
  conversation in two tabs. A codex command spyc can't rewrite exactly is
  refused rather than forked with different settings. A tab running anything else forks
  into a copy of its command, opened at the tab's current directory. `docs/HARNESS.md` §4
  has the per-agent detail
- Activity indicator (**+**) on background tabs that have new output
- **Startup tabs**: `[pane] tabs = ["claude", "zsh"]` (or `[[pane.tab]]`
  tables with a per-tab `cwd`, `label`, and `agent` for a command that wraps
  one, like `npx @openai/codex`, which then runs as written but is that
  agent's tab) opens those tabs at launch, the
  config-driven version of pressing `^a c` once per tab. A tab whose `cwd`
  isn't a directory opens in the default one, and the message at launch
  names it. `spyc -r` restores
  its saved tabs instead. Yours in `~/.spycrc.toml`, or in a project's own
  file under `~/.config/spyc/projects/`, open unasked. A
  project-local `.spycrc.toml` list runs only after you approve it in a
  pop-up (`y` run, `n` never, `Esc` not now). The approval binds to the
  exact commands and cwds, so an edited list asks again. `:startup-tabs`
  shows the answer, and `:startup-tabs forget` clears it. Reference:
  [Startup tabs](../../CONFIGURATION.md#startup-tabs--pane-tabs)
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
  - **Git SHA** → open the commit in the `show` view (the same
    pager as `gd`; `|` toggles the layout)
  - **Custom pattern** with a `url = "..."` template → fill `{}`
    with the match, then `open`/`xdg-open`
  - Other kinds (IPv4, custom without template) → fall back to
    yank with a flash hint
- **`q` / `Esc`** — exit without action

Scroll mode just works: scroll up to a Claude reply, hit `^a u`,
the URLs in *that* reply get labels (the picker scans exactly the
visible viewport at the user's scroll position). At the bottom of a
codex or agy pane it leaves out the agent's input box and footer, and an
approval dialog on screen, so a draft you're typing gets no labels.

Custom patterns in `.spycrc.toml`:

```toml
[[scan.patterns]]
name = "jira"
regex = '[A-Z]+-\\d+'
url = "https://example.atlassian.net/browse/{}"   # optional
```

Without `url`, uppercase falls back to yank+hint. Bad regexes are
dropped at config load and noted in the debug log; a typo never
prevents spyc from starting.
