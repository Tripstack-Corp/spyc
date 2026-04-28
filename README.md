<p align="center">
  <img src="docs/spyc-logo.png" alt="spyc logo" width="128">
</p>

<h1 align="center">spyc</h1>

<p align="center">
  A vi-keyboard-driven file commander that runs Claude Code in a split pane<br>
  and exposes itself to Claude as an MCP server.<br>
  When Claude asks "what files are you looking at?" -- it queries spyc directly.
</p>

<p align="center">
  macOS and Linux · v1.21.1 · actively developed
</p>

<p align="center">
  <img src="docs/screen_shot.png" alt="spyc split-pane screenshot" width="720">
</p>

---

## Why spyc?

spyc is a terminal file manager built around the AI agent being a
**peer in the workflow**, not a separate tab.

When you pick three files and ask Claude a question in the bottom pane,
Claude can call `get_spyc_context` and see your cwd, cursor file, picks,
inventory, active filter, and git branch -- without you copying or
pasting anything. When Claude mentions a file path in its response,
press `gf` to jump straight to it. The context flows both ways.

This is possible because spyc runs an **MCP server** on a PID-scoped
Unix domain socket. Claude discovers it automatically via `.mcp.json`
-- no flags needed. Multiple spyc instances coexist safely, and the
context stays current as you navigate. We don't know another TUI file
manager that exposes itself to an AI agent this way.

Everything else -- vi motions, marks, picks, inventory, pager, shell
integration -- is what you'd expect from a keyboard-driven file manager.
But the MCP bridge is what makes spyc different from Yazi, Broot,
Ranger, or anything else in the space. See
[How the MCP bridge works](#how-the-mcp-bridge-works) for the mechanism.

The name: **spy** (inspired by SideFX's in-house file manager) +
**c**laude = **spyc**.

## Quick start

### Prerequisites

- **Rust** 1.85+ -- `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Nerd Font** recommended for the powerline status bar; press `C`
  inside spyc to toggle a mono fallback if you don't have one.
  Install one with: `brew install --cask font-meslo-lg-nerd-font`
- **Claude Code** (optional) -- `npm install -g @anthropic-ai/claude-code`

### Install

```sh
git clone https://bitbucket.org/tripstack/spyc.git
cd spyc
make install          # builds release + copies to ~/.local/bin (no sudo)
```

### Launch

```sh
spyc            # opens in the current directory
spyc -r         # resume a previous session (restores Claude conversation)
```

spyc opens with your cwd in a multi-column listing. Move with `hjkl`,
enter a directory with `d`, view a file in the pager with `Enter`, open
in `$EDITOR` with `e`. Press `?` for the full help overlay.

To open the Claude pane, press **`^\`** (Ctrl+Backslash) or **F10**.
Press **`^a j`** / **`^a k`** to switch focus between file list and pane.

## How the MCP bridge works

When spyc starts, it automatically:

1. **Starts an MCP server** on a PID-scoped Unix domain socket
   (`~/.local/state/spyc/mcp-<PID>.sock`)
2. **Writes `.mcp.json`** so Claude Code discovers spyc via standard
   config -- no flags needed
3. **Keeps a context snapshot current** -- cwd, cursor file, picks,
   inventory, filter, git branch -- updated as you navigate

Multiple instances coexist safely. If a new spyc opens in a directory
already owned by a live spyc, it prompts on stderr (`PID N already owns
MCP here. Take over? [Y/n]`) before sending the disconnect and
rewriting `.mcp.json`. Decline and the old instance keeps ownership.
Enterprise `managed-mcp.json` and `managed-settings.json` policies are
respected — see [INSTALL.md](INSTALL.md#enterprise-managed-environments)
for details.

Claude can call `get_spyc_context` at any time to see exactly what
you're looking at. Use `gf`/`gF` to jump from Claude's output back to
the file list. The context is bidirectional and always current.

Sessions are auto-saved on quit. `spyc -r` opens a session picker that
restores all pane tabs and resumes Claude conversations via
`--resume <sessionId>`.

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Move (counts work: `5j`, `10k`) |
| `gg` / `G` | Top / bottom |
| `d` / `Enter` | Descend into dir or view file in pager |
| `e` / `v` | Descend into dir or open file in `$EDITOR` |
| `V` | Open `$EDITOR` in top pane (Claude pane stays visible) |
| `u` / `-` | Climb to parent |
| `/` | Search current listing (incremental, glob-aware) |
| `H` / `~` | Jump to home |
| `J` | Jump to any path |
| `F` | Project-wide fuzzy filename finder (gitignore-aware) |
| `:grep <pat>` | Project-wide content search (embedded ripgrep matcher) |

### Picks & inventory

**Picks** are per-directory multi-select. **Inventory** is a persistent
file cache that survives across sessions.

| Key | Action |
|-----|--------|
| `t` | Toggle pick |
| `T` | Pick by glob |
| `yy` | Yank to inventory (copies file to cache) |
| `yp` | Yank visible pane output to clipboard |
| `yP` | Yank last typed prompt to clipboard |
| `Y` | Remove cursor file from inventory |
| `p` | Put inventory files into cwd |
| `i` | Toggle inventory view |

### Split pane

The pane is a real pty -- it runs `claude` by default, but any command
works. Prefix is `^a` (screen-style); `^w` also works.

| Key | Action |
|-----|--------|
| `^\` / `F10` | Toggle pane |
| `F9` | Open pane with `claude --resume` |
| `^a j` / `^a k` | Switch focus |
| `^a c` | New tab |
| `^a n` / `^a ]` | Next tab |
| `^a p` / `^a [` | Prev tab |
| `^a K` / `^a x` | Close tab |
| `^a 1`..`9` | Switch to tab N |
| `^a s` | Send selection paths to pane |
| `^a P` | Pipe file contents to pane |
| `^a v` | Scroll mode (10K line scrollback) |
| `Ctrl+J` | Newline in pane (multi-line input) |
| `gf` | Jump to file path in pane output |
| `gF` | Jump to file + open at referenced line |

### Pager

Press `d` or `Enter` on a file to view it in the built-in pager with
syntax highlighting, search, line numbers, hex dump, and ANSI color
support. Press `?` inside the pager for its own help overlay.

### Shell

| Key | Action |
|-----|--------|
| `!` | Captured command -- streams into pager |
| `!!` | Repeat last command |
| `!?` | History editor (vi-editable, searchable) |
| `;` | Foreground command (top, vim, etc.) |
| `$` | Drop into `$SHELL` |
| `:` | Command line (`:cd`, `:sort`, `:limit`, `:grep`, `:fg`, `:task`, `:q`) |

`%` in any command expands to the current selection.

### Background tasks & buffer history

Long captured commands shouldn't lock you out of spyc.

| Key | Action |
|-----|--------|
| `^Z` | (in `!` pager) send the running task to the background |
| `:fg` / `:fg N` | resume the most-recent (or specific) backgrounded task |
| `gB` / `:task N` | open the *task viewer* -- a peek view without taking ownership |
| `[t` / `]t` | (in pager, chord) cycle the task viewer prev/next by id |
| `gp` | reopen the most-recently-closed pager buffer |
| `:bprev` / `:bnext` | walk pager buffer history back/forward |
| `[b` / `]b` | (in pager, chord) walk buffer history back/forward |

Backgrounded tasks render in the pane divider as `[N+]` (running, new
output), `[N●]` (running, quiescent), `[N✓]` (exit 0), `[N✗]`
(non-zero / killed / crashed), in a distinct color from pane tabs.
When a viewed task exits, closing the task viewer pushes its
final rendered view into the buffer-history stack so `[b` walks
back to it later.

### Marks & filters

| Key | Action |
|-----|--------|
| `m{a-z}` | Set a bookmark |
| `'{a-z}` | Jump to bookmark |
| `''` | Jump back (like `cd -`) |
| `` ` `` | Jump to start dir (set with `gS` or `:startdir`) |
| `a` | Toggle dotfile filter |
| `o` | Toggle build artifact filter |
| `=` | Temporary glob filter (`=*.rs`, `=!` for picks) |

### Project home & session

Each spyc run has a `PROJECT_HOME` (a sticky project root) and a
session name (a spice-themed label like `SAFFRON_CUMIN`). Both appear
on the top bar and persist across `spyc -r`.

| Key | Action |
|-----|--------|
| `gh` | Jump to `PROJECT_HOME` |
| `gP` | Set `PROJECT_HOME` to current directory |
| `gS` | Set start dir (target of `` ` ``) to current directory |
| `gU` | Flash `user@host` in the status line |
| `:project [.\|<path>\|clear]` | Manage `PROJECT_HOME` |
| `:startdir [.\|<path>]` | Manage start dir |
| `:name <NEW>` | Rename the active session |
| `:whoami` | Show `user@host` |

`PROJECT_HOME` is auto-set on startup if the launch directory contains
`.git`. New pane tabs default their cwd to `PROJECT_HOME` when set.

## Configuration

spyc reads `.spycrc.toml` from `~/.spycrc.toml` (user) and `./.spycrc.toml`
(project). Changes are picked up live -- no restart needed (`^R` to force).

To bootstrap a config with every option commented out at its default:

```sh
spyc --print-config > ~/.spycrc.toml
```

Selected options:

```toml
# Layout: where the status bar lives. "top" (default) or "bottom".
# Use "bottom" inside tmux so spyc's bar doesn't double up with tmux's.
[layout]
status_position = "bottom"

# Keymap: rebind keys to any action. Chord bindings supported.
keymap = [
    "map f unix file %",
    "map gh jump ~",
]

# Color overrides: customize the palette.
[colors]
cursor_bg = "#ff6600"
pick      = "#ffcb6b"
```

## Recommended setup

- **Terminal:** [iTerm2](https://iterm2.com/) (macOS), WezTerm, Kitty, Ghostty, or Alacritty
- **Font:** Any [Nerd Font](https://www.nerdfonts.com/) for the powerline status bar.
  Press `C` to toggle mono mode if you prefer not to install one.
- **Claude Code:** `npm install -g @anthropic-ai/claude-code`
- **Platforms:** macOS and Linux (x86_64, aarch64). Windows via WSL.

See [INSTALL.md](INSTALL.md) for detailed setup instructions.

## More docs

- [FEATURES.md](FEATURES.md) -- complete feature reference
- [INSTALL.md](INSTALL.md) -- terminal, font, build, and cross-compilation setup
- [ARCHITECTURE.md](ARCHITECTURE.md) -- concurrency model, MVU target shape, persistence, MCP transport
- [DESIGN.md](DESIGN.md) -- UI design language: components, surfaces, palette, extension checklist
- [CHANGELOG.md](CHANGELOG.md) -- release history
- [ROADMAP.md](ROADMAP.md) -- what's next
- [CONTRIBUTING.md](CONTRIBUTING.md) -- contribution guidelines and SemVer policy
- [BUGS.md](BUGS.md) -- known bugs and planned fixes

## License

BSD-3-Clause. Logo uses [Twemoji](https://github.com/jdecked/twemoji) pepper
artwork (CC-BY 4.0).
