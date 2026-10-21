# Configuring spyc

Everything spyc reads at runtime, with examples. Two surfaces:

- **`.spycrc.toml`** — TOML: layout, notifications, colours, ignore masks, and the
  keymap DSL.
- **`~/.config/spyc/`** — Lua: `map KEY lua <name>` scripts and an optional
  `init.lua` for keybinds, `:` commands, and event hooks.

Six sections have their own page under `docs/config/`: the keymap DSL, Lua,
the mouse, notifications, the agent HUD and per-project files. Each keeps a
summary here.

The fastest start: `spyc --print-config` prints a fully-commented template with
every option at its default — pipe it to a file and uncomment what you want:

```sh
spyc --print-config > ~/.spycrc.toml
```

## Where config lives

| File | Scope |
|------|-------|
| `~/.spycrc.toml` | per-user defaults |
| `<project>/.spycrc.toml` | a repo's own settings — **win over** the user file |
| `~/.config/spyc/projects/<label>.toml` | your settings for one project, matched by its `home` — **win over** both ([per-project files](docs/config/projects.md)) |
| `~/.config/spyc/lua/<name>.lua` | scripts run by `map KEY lua <name>` |
| `~/.config/spyc/init.lua` | registers keybinds / `:` commands / event hooks |

Every `.spycrc.toml` and project file is **watched** — edits take effect
**without a restart** (`:lua reload` or `^R` re-runs `init.lua`). Each layers on
the one before per-field, so a bare `[notify]` in a project file doesn't clobber
your user defaults. `~/.config/spyc` is `$XDG_CONFIG_HOME/spyc` when that is set.

**Your own files** are `~/.spycrc.toml` and those under
`~/.config/spyc/projects/`; a repo's `.spycrc.toml` is someone else's text.

**Security:** the *executing* keymap verbs (`unix`, `command`, `lua`, `jump`,
`prompt`), and the four [actions](docs/config/keymap.md#action-names) that type at the pane, only take
effect from **your own files** — a project-local `.spycrc.toml` in an untrusted
clone can't bind a key to run code, and its
[`[prompts]`](#prompt-templates--prompts) are ignored, so a repo can't choose
what gets typed at your agent. Nor can it name the
[`[clipboard] command`](#clipboard), which runs on every yank, or give a
[scan pattern](#quick-select-patterns--scanpatterns) a `url` other than a web
page. A project file's
[startup tabs](#startup-tabs--pane-tabs) run their commands at launch with no
keypress at all, so they open only after you approve that exact list, and an
edited list asks again. Lua scripts load only from `~/.config/spyc/`.

## Per-project files

A file under `~/.config/spyc/projects/` holds your settings for one project: its
`home` key names the project, and the rest is `.spycrc.toml`'s own schema. It is
yours, so it is trusted as `~/.spycrc.toml` is, and it wins over both that and
the repo's own `.spycrc.toml`: tint each project, give it its own startup tabs,
bind keys only it needs. The file's name is the project's name. Nothing is ever
written into the project's directory. `:project config` opens it in your
editor, making it first when there's none. Matching, layering and examples:
[`docs/config/projects.md`](docs/config/projects.md).

---

## `.spycrc.toml` at a glance

```toml
[layout]
status_position = "top"        # or "bottom" (vim/tmux convention; prompt sits above)
chord_hint_delay_ms = 300      # ms holding a chord (g, ^a, H) before the which-key popup; 0 disables
color_depth = "auto"           # "auto" (truecolor if $COLORTERM says so, else 256), "truecolor", or "256"
vsplit_mode = "full_height"    # shape `^s |` opens a vertical split in; or "top_only" (pane stays full-width below)
status_flags = "auto"          # status-bar state: only what differs, short when tight; or "short", "full"

[pane]
default_command = "claude"     # pre-filled into the `^a c` new-tab prompt
tabs = ["claude", "zsh"]       # open these tabs at launch (a repo's list asks first; see below)
new_tab_cwd = "worktree_root"  # focused column's worktree root (gw's target); or "project_home" (PROJECT_HOME), "browse_dir" (the focused column's dir)
claude_transcript_scrollback = false  # `^a v` reads Claude's JSONL transcript instead of terminal scrollback
                                      # (only decides which comes up FIRST — `T` swaps in the view, and the
                                      #  transcript is used regardless when there's no terminal capture)
codex_mcp = true               # register spyc's MCP server in codex's user config; only $HOME's rc decides
codex_daemon = false           # false: codex panes run `--no-daemon`, so their hooks + MCP reach the spyc that launched them
preview_pasted_images = true   # keep a copy of images you paste into an agent pane, for `^a g`
agent_hud = "off"             # "off", "compact", or "full"; only your own files decide
agent_hud_resets = "clock"    # when a plan window resets: "clock" (3:40pm, Thu 9am) or "countdown" (2h05m)

[yank]
include_pager_title = true     # prepend a source header to pager yanks (y/Y)

[pager]
tab_width = 4                  # columns a tab expands to (min 1)

[markdown]
open_as_rendered = true        # open .md in rendered view (m toggles source)

[diff]
intraline = "char"             # intra-line highlight granularity: "char" | "word"

[delete]
confirm = true                 # y/N prompt before R / dd; false = yolo (still recoverable in the graveyard)
```

All optional; anything unset uses the built-in default. See `spyc --print-config`
for the annotated version of each.

### Agent HUD — `[pane] agent_hud`

The native agent strip below a Codex, Claude Code or Antigravity tab is **off by default**: `agent_hud = "compact"` (two rows) or `"full"` (four) in `~/.spycrc.toml`, or `:agent hud off|compact|full` for the session, and `agent_hud_resets` picks clock times or countdowns. What each row shows and where its facts come from: [`docs/config/agent-hud.md`](docs/config/agent-hud.md).

### Colour depth — `[layout] color_depth`

spyc's theme is 24-bit truecolor. Terminals that can't parse `\x1b[38;2;r;g;bm`
drop it wholesale — you get no colour and no highlight. The worst offender is
macOS's **bundled GNU screen 4.00.03** (frozen at pre-GPLv3, from 2006), which
also mangles the powerline/emoji glyphs.

| value | behavior |
|-------|----------|
| `auto` (default) | 256 inside GNU screen; else truecolor when `$COLORTERM` advertises `truecolor`/`24bit`, else 256 |
| `truecolor` | always emit 24-bit RGB |
| `256` | quantize every color to the nearest xterm-256 index |

When not truecolor, the finished frame is remapped once before it's written, so
**all** colours degrade — theme, syntax highlighting, diffs, and ANSI passthrough.
`--color auto|truecolor|256` overrides the config for a single run.

**GNU screen:** `auto` drops to 256 whenever it's running inside screen (`$STY`
set), *even if `$COLORTERM=truecolor`* — screen inherits that claim from the outer
terminal but doesn't render 24-bit SGR (macOS's bundled 4.00.03 can't at all; 5.x
needs `truecolor on`, off by default), so trusting it leaves you colourless. If
you've turned `truecolor on` in a modern screen, force it back with
`color_depth = "truecolor"`. tmux is unaffected — it renders RGB and keeps
truecolor.

In any non-truecolor mode spyc also swaps the 🌶️ header glyph for a spice-red
block — old screen mangles the 2-cell emoji, so the block keeps the header
looking intentional rather than broken.

### Vertical-split height — `[layout] vsplit_mode`

The shape `^s |` (alias `^a |`) opens a vertical split in.

| value | behavior |
|-------|----------|
| `full_height` (default) | the divider runs the whole frame height; the right column is a full-height reading surface and the pty pane is confined under the left column |
| `top_only` | only the file-list region splits; the pane stays full-width below both columns |

Aliases: `full` for the first, `half` / `half_height` / `top` for the second.
`^s f` flips an *open* split either way, so this only sets where it starts. A
second commander (`^s n`) ignores the setting and always opens `top_only` — two
peer browsers normally want one full-width pane beneath them — and `^s f` is how
you make that one full-height too.

### Status-bar state — `[layout] status_flags`

The status bar ends with the focused column's state, and lists only what
differs from rest:

| full | short | shown when |
|---|---|---|
| `picks:3` | `p:3` | anything is picked |
| `inv:2` | `i:2` | the inventory has items |
| `m1:off` / `m2:on` | the same | a mask differs from its configured default |
| `limit:*.rs` | `l:*.rs` | a `=` / `:limit` filter is active (`limit:picks` for `=!`) |
| `hidden:14` | `h:14` | the masks or the filter hide entries |
| `sort:mtime↑` | `s:t↑` | the sort isn't by name, ascending (`n` name, `s` size, `t` time, `e` ext) |

An archive mount's badge, background tasks (when there's no pane divider to
show them) and `[ZOOM]` appear as before.

| value | behaviour |
|-------|----------|
| `auto` (default) | the full words, or the short forms when the words would cut the path |
| `short` | always the short forms |
| `full` | every field on every frame, `[picks:0 inv:0 m1:on m2:on hidden:14 sort:name]` — the bar before this setting |

`:set flags=auto|short|full` changes it for the rest of the run; a config
reload doesn't undo it.

### Startup tabs — `[pane] tabs`

Opens the bottom pane with tabs already in place at launch, the config-driven
version of pressing `^a c` once per tab. The compact form lists commands:

```toml
[pane]
tabs = ["claude", "zsh"]
```

The table form adds a working directory and a display label per tab, and for
a command that wraps an agent, which agent it runs:

```toml
[[pane.tab]]
command = "claude"
cwd = "~/Work/my-project"   # ~, absolute, or relative to the launch directory
label = "coordinator"       # defaults from the command

[[pane.tab]]
command = "npx @openai/codex --no-daemon"
agent = "codex"             # claude | codex | agy | zot
```

- **`agent` is for wrappers.** spyc recognizes an agent tab by its command's
  program, so `npx @openai/codex`, `env FOO=1 claude`, `uv run agy` or a script
  of your own would be a plain tab. Naming the agent gives it that agent's
  activity dot, status hooks, MCP entry and `^a v` transcript. spyc never edits
  such a command, though: it runs exactly as written, at launch, on `^a R` and
  on restore. Claude still resumes its conversation on restore, because it does
  that by typing `/resume`; codex, agy and zot start fresh. `^a F` can't fork
  it, and codex needs `--no-daemon` in the command itself for hook status. When
  the command already names the agent (`codex -m o3`), `agent` changes nothing.
  An unknown name is a config error.

- **A project's own tabs need your approval first.** Yours, in your own
  files, open unasked, a project file's in place of `~/.spycrc.toml`'s. A project-local `.spycrc.toml` can declare a
  list too, a repo's own claude + dev-server set for instance. Those commands
  aren't yours, so at launch spyc lists every one in a pop-up:
  - **`y`** approves that list: it opens now and on later launches, in place of
    yours.
  - **`n`** declines it, and your own tabs open, now and on later launches.
  - **`Esc`** skips it this once and asks next time.
  - Any other key keeps the pop-up up, so a reflexive `Enter` can't approve a
    command.

  The answer binds to the **exact list**: every command, `cwd` and `agent`, in
  order. If a later `git pull` changes the list, spyc asks again. A changed
  `label` doesn't count, since labels don't change what runs. The prompt escapes
  control characters and caps very long commands at a visible count, so what
  you read is what would run. `:startup-tabs` shows the current answer, and
  `:startup-tabs forget` drops it so the next launch asks again.
- Use one form per file. Setting both is an error at load, and so is more than
  9 tabs (the `^a 1..9` reach) or an empty `command`. In a project file each of
  those is a warning instead, and the project's list is ignored, so a broken
  repo can't stop your own config loading.
- A `cwd` that doesn't exist falls back to `[pane] new_tab_cwd`.
- `spyc -r` wins: resuming a session restores its saved tabs and skips these.
  Unset, nothing opens until the first `^a c`.
- Startup doesn't take the keyboard. The first tab is active, focus stays on
  the file list, and one flash summarizes the spawn, including any cwd
  fallbacks or failed spawns.
- When labels don't fit the divider, each tab is cropped so every tab stays
  visible and clickable. A per-tab `label` is the way to keep tabs with a
  shared prefix apart.

---

## `[clipboard]`

```toml
[clipboard]
via = "auto"   # auto | system | osc52 | both
```

Where a yank lands. `auto` uses an **OSC-52 terminal escape when spyc is over SSH**
and the local helper (`pbcopy` / `wl-copy` / `xclip`) otherwise.

This matters more than it sounds: the local helpers set the clipboard of the machine
spyc *runs on*. Over SSH that's the server, so a yank silently succeeds somewhere you
can never paste from. OSC 52 travels back up the same connection the UI is drawn on,
so it reaches the terminal you're typing at.

Locally it stays helper-first deliberately — OSC 52 is write-only with no reply, so
spyc can't confirm the terminal honoured it, and some terminals disable it on purpose
(a remote host writing your clipboard is a real risk). Inside tmux it needs
`set -g set-clipboard on`; spyc DCS-wraps the escape so tmux forwards it to the outer
terminal. A selection too large for the escape (~75 KB of base64) is **refused with a
message** rather than risking a terminal truncating it silently — it does not fall
back to the helper, because under `auto` over SSH the helper isn't enabled and writing
the server's clipboard wouldn't help you anyway. Only `via = "both"` has a second
mechanism to reach.

```toml
[clipboard]
command = "wl-copy"
```

`command` overrides everything above: when set, spyc runs this exact command
verbatim — whitespace-split into argv, same "no shell features" contract as
`$EDITOR`/`$PAGER` resolution, so wrap it in a script if you need pipes or
redirection — and pipes the yanked text to its stdin. `via`/OSC-52 are skipped
entirely, not layered underneath. `$SPYC_CLIPBOARD` is the env-var form and wins
over the config key if both are set, matching how other spyc envs layer over
static config. Only your own files may set `command`: a repo's is ignored with
a warning, since it would run the repo's command on a keypress.

## Notifications — `[notify]`

When an agent tab changes status, spyc can raise a desktop notification, ring the terminal bell, or pulse the border. `Blocked` fires every enabled channel; a finished turn fires a channel only if its `*_done` flag opts in. The keys, the model and some recipes: [`docs/config/notifications.md`](docs/config/notifications.md).

---

## Diff — `[diff]`

`intraline` sets how finely a modified line pair is marked up on top of the
row wash, in `gd` / `gD` / a commit opened from `gl`.

| value | marks | reads as |
| --- | --- | --- |
| `"char"` *(default)* | just the characters that differ — `value` → `values` brightens the `s` | precise; tells you exactly what moved |
| `"word"` | the whole changed token, like `git --word-diff` — brightens all of `values` | calmer on dense code; hides which characters moved |

Both granularities mark **every** changed region on the line, so an unchanged
token sitting between two changed ones is never swept into the highlight.

Pick `"word"` if per-character marking on tightly-packed code reads as
confetti to you; `"char"` is the default because saying precisely which
characters changed is what an intra-line highlight is for.

A very long line (roughly 4000+ characters, or 500+ words) gets no intra-line
highlight at all — on a minified or generated line it would be unreadable
noise, and the row wash still shows the line changed.

---

## Archives — `[archive]`

`Enter` on a zip / tarball browses it as a directory (see FEATURES.md →
Archives). A mount is an *index*, so entering one costs no disk; these knobs
bound the one case that does — a compressed tar, which has to be extracted as it
streams because it can't be listed any other way.

```toml
[archive]
enable = true              # false → Enter pages the archive as bytes
extract_budget_mb = 512    # hard ceiling for a .tar.gz / .tar.zst mount
warn_over_mb = 128         # confirm before extracting more than this
max_entries = 200000       # cap on indexed members
max_depth = 2              # how deep archives may nest (0 = no nesting)
write_back = "ask"         # "ask" | "never" — offer to write pending changes
snapshot_max_mb = 64       # graveyard-snapshot the original below this size
```

`max_entries` bounds the index spyc builds — members walked plus the directories
it synthesizes for them — and a listing that hit it says it is truncated. It does
**not** bound what the container's own parser does first: `zip` materializes one
record per central-directory entry before spyc sees any of them, so a zip with a
genuinely huge central directory is fully resident whatever the cap says. (The
crate does guard its pre-allocation against a lying entry count, so a small file
can't force a large one.)

`max_depth` is about disk, not correctness: a container inside a container has to
be copied out whole before anything can read it, so every level costs its own
size in staging. Set it to 0 to refuse nested archives entirely.

A write-back never puts the original at risk: the new archive is written to a
temp file beside it, verified by reading it back, and only then renamed into
place. `snapshot_max_mb` is about *regret*, not corruption — below that size the
original is also copied into the graveyard so `:undo` can restore it.

Staging lives under `$XDG_STATE_HOME/spyc/archives/<pid>-<hash>/` and is removed
when the archive is unmounted or spyc exits; a tree left behind by a killed
process is reaped at the next launch.

## Mouse — `[mouse]`

Mouse reporting is on by default (`capture`), and the wheel scrolls whatever is under the pointer; `scroll_lines`, `pane_scroll_lines`, `pane_scroll_view` and `invert_scroll` tune it. What each button does, and selecting text: [`docs/config/mouse.md`](docs/config/mouse.md).

## Colours — `[colors]`

Hex (`"#aabbcc"`) or named (`"red"`). Anything unset falls back to the built-in
palette. Match these to spyc's brand palette (see `docs/BRAND.md`) if you want
screenshots and the running tool to agree.

```toml
[colors]
dir           = "#82aaff"
exec          = "#c3e88d"
symlink       = "#89ddff"
file          = "#cccccc"
cursor_bg     = "#ff9e64"   # the cursor row
pick          = "#ffcb6b"   # multi-select
take          = "#c792ea"   # inventory
status_user   = "#bb9af7"
status_path   = "#7dcfff"
popup_border  = "#bb9af7"   # which-key / harpoon pop-up outline
# Diff / show / blame renderer:
diff_add_fg   = "#9ece6a"
diff_del_fg   = "#f7768e"
diff_add_bg   = "#122619"   # row wash behind +/- lines
diff_del_bg   = "#2a161b"
```

(`spyc --print-config` lists the full token set.)

---

## Ignore masks — `[[ignore_masks]]`

Two toggleable groups: group 1 (`a` key) and group 2 (`o` key). Patterns are
globs matched against the filename. **Defining any mask here replaces the
built-ins wholesale** — so redefine both groups if you customize.

```toml
[[ignore_masks]]
group = 1
enabled = true
patterns = [".*"]                       # dotfiles

[[ignore_masks]]
group = 2
enabled = true
patterns = ["*.o", "target/", "node_modules/", "*.pyc"]
```

---

## Quick Select patterns — `[[scan.patterns]]`

`^a u` (Quick Select) labels URLs / paths / SHAs / IPv4 in the visible pane;
lowercase yanks a match, uppercase opens it. Add your own patterns — they're
**appended** to the built-ins:

```toml
[[scan.patterns]]
name  = "ticket"                # label for the match kind
regex = "TICKET-[0-9]+"         # matched against the visible pane text

[[scan.patterns]]
name  = "ticket-link"
regex = "TICKET-[0-9]+"
url   = "https://tracker.example.com/browse/{}"   # uppercase-open target; {} = the matched text
```

A pattern with an un-compilable regex is skipped with a warning, not a crash.

An uppercase label hands the filled `url` to your system opener, so a project
`.spycrc.toml`'s template is kept only when it opens a web page (`http://` or
`https://`). One naming a local file (`./{}.command`, `file://…`) would launch a
program from the repo, so it is dropped with a warning; the pattern still labels
and yanks. Your own files may use any template.

---

## Prompt templates — `[prompts]`

A prompt template is a message you send an agent over and over. One key types
it into the active pane tab, with the selection filled in:

```toml
keymap = [
  "map <F6> prompt review",
  "map <F7> prompt explain",
]

[prompts]
review  = "Review % for bugs. Don't change anything yet."
explain = "Explain what %d does, starting from its entry point."
tests   = """
Write tests for %.
Cover the error paths first."""
```

(`keymap` goes above `[prompts]`: in TOML, a key after a table header belongs to
that table.)

| Token | Becomes |
|---|---|
| `%` | the picks, else the cursor row — what `^a s` sends |
| `%i` | the inventory's picked items, else all of them |
| `%d` | the focused column's directory |
| `%%` | a literal `%`, so "50%" in a template is written `50%%` |

Paths are written the way `^a s` writes them: relative to the receiving pane's
own cwd when they're under it (`./` for the cwd itself), absolute otherwise, and
quoted when they hold a space. A directory ends in `/` (a symlink to one
doesn't), so `%d` always does, unless the template already follows the token
with a slash: `%d/notes.md` comes out with one slash, not two.

**The prompt is typed, not sent.** The pane gets the keyboard and **Enter**
sends it, so you can read what was filled in, or finish the sentence, first. An
agent that asked for bracketed paste (claude and codex both do) gets the
template as one paste, so a multi-line template arrives whole rather than
submitting at its first newline.

A token with nothing behind it (`%` in an empty directory, `%i` with an empty
inventory) is refused rather than typing a prompt about nothing, and so is a
path that isn't valid UTF-8, since it can't be typed as itself.

`:prompt <name>` types one without a key, and a bare `:prompt` lists them.

**`[prompts]` loads only from your own files.** A template is text typed at
your agent, so a repo's `.spycrc.toml` that defines one is ignored with a warning,
and `prompt` is one of the executing verbs.

---

## Keymap — the `keymap` DSL

`keymap = [ ... ]` holds one binding per string. `map <KEY> <action>` binds any built-in action by name (`map <F8> harpoon_jump =2` gives one a parameter); `unix`, `command`, `lua`, `prompt`, `patternpick` and `jump` bind the other forms; `unmap <KEY>` silences a key. Only your own files may bind a form that runs something. The forms, the reserved `^a` / `^w` prefixes, and every action name: [`docs/config/keymap.md`](docs/config/keymap.md).

---

## Lua scripting — `~/.config/spyc/`

For logic a DSL line can't express: `map KEY lua <name>` runs `~/.config/spyc/lua/<name>.lua`, and an optional `~/.config/spyc/init.lua` registers keybinds, `:` commands and event hooks. Scripts run off the main thread under a kill switch, and only from `$HOME`. The `spyc.*` API and examples: [`docs/config/lua.md`](docs/config/lua.md).

---

## See also

- **`spyc --print-config`** — the annotated template (source of truth for every
  TOML option).
- **`README.md`** — install + a quick tour.
- **`FEATURES.md`** — the full feature reference.
- **`docs/BRAND.md`** — the palette, for theming to match the brand.
