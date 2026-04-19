<p align="center">
  <img src="docs/spyc-logo.png" alt="spyc logo" width="128">
</p>

<h1 align="center">spyc 🌶️</h1>

<p align="center">
  A vi-keyboard-driven file commander that pairs with Claude Code.<br>
  Browse files in the top half. Talk to Claude in the bottom half.<br>
  They share context via MCP — Claude can see what you see.
</p>

---

## ✨ Why spyc?

Most developers alt-tab between a file browser and an AI assistant.
spyc puts them in the **same terminal session** with shared context:

- 🗂️ **File list above** — vi motions, picks, marks, inventory, git status
- 🤖 **Claude Code below** — full pty pane with multi-tab, scrollback, session resume
- 🔗 **MCP bridge** — Claude can query your cwd, cursor, picks, inventory, filter, and git branch
- ⌨️ **100% keyboard** — if you think in `hjkl`, you're home

The name: **spy** (inspired by SideFX's in-house file manager) + **c**laude = **spyc** (spicy 🌶️).

## 🚀 Quick start

### Prerequisites

- **Rust** 1.85+ — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **A Nerd Font** — `brew install --cask font-meslo-lg-nerd-font` (for powerline status bar)
- **Claude Code** (optional) — `npm install -g @anthropic-ai/claude-code`

### Install

```sh
git clone https://bitbucket.org/tripstack/spyc.git
cd spyc
make install    # builds release → ~/bin/spyc
```

### Launch

```sh
spyc            # opens in the current directory
spyc -r         # resume a previous session
```

Press **`^\`** (Ctrl+Backslash) or **F10** to open the Claude pane.
Press **`^W j`** / **`^W k`** to switch focus between file list and pane.

## 🎯 Core concepts

### 📁 Navigation

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Move (counts work: `5j`, `10k`) |
| `gg` / `G` | Top / bottom |
| `d` / `Enter` | Descend into dir or view file |
| `u` / `-` | Climb to parent |
| `/` | Search (incremental, glob-aware) |
| `H` / `~` | Jump to home |
| `J` | Jump to any path |

### 📌 Picks & inventory

**Picks** are per-directory multi-select. **Inventory** is a persistent file cache.

| Key | Action |
|-----|--------|
| `t` | Toggle pick |
| `T` | Pick by glob |
| `y` | Yank to inventory (copies file to cache) |
| `Y` | Remove cursor file from inventory |
| `p` | Put inventory files into cwd |
| `i` | Toggle inventory view |

### 🖥️ Split pane

The pane is a real pty — it runs `claude` by default, but any command works.

| Key | Action |
|-----|--------|
| `^\` / `F10` | Toggle pane |
| `F9` | Open pane with `claude --resume` |
| `^W j` / `^W k` | Switch focus |
| `^W n` | New tab |
| `^W 1`..`9` | Switch to tab N |
| `^W s` | Send selection paths to pane |
| `^W p` | Pipe file contents to pane |
| `^W v` | Scroll mode (10K line scrollback) |
| `Ctrl+J` | Newline in pane (Claude multi-line input) |
| `gf` | Jump to file path in pane output |
| `gF` | Jump to file + open at referenced line |

### 🔍 Pager

Press `d` or `Enter` on a file to view it in the built-in pager with syntax
highlighting, search, line numbers, hex dump, and ANSI color support.

### ⚡ Shell

| Key | Action |
|-----|--------|
| `!` | Captured command → streams into pager |
| `!!` | Repeat last command |
| `!?` | History editor (vi-editable, searchable) |
| `;` | Foreground command (top, vim, etc.) |
| `$` | Drop into `$SHELL` |
| `:` | Command line (`:cd`, `:sort`, `:limit`, `:q`) |

`%` in any command expands to the current selection.

### 🏷️ Marks & filters

| Key | Action |
|-----|--------|
| `m{a-z}` | Set a bookmark |
| `'{a-z}` | Jump to bookmark |
| `''` | Jump back (like `cd -`) |
| `` ` `` | Jump to session home |
| `a` | Toggle dotfile filter |
| `o` | Toggle build artifact filter |
| `=` | Temporary glob filter (`=*.rs`, `=!` for picks) |

## 🤖 Claude Code integration

When spyc spawns Claude in the pane, it automatically:

1. **Starts an MCP server** on a background thread (HTTP, localhost)
2. **Injects `--mcp-config`** so Claude connects at spawn
3. **Writes context** every tick — cwd, cursor file, picks, inventory, filter, git branch

Claude can call `get_spyc_context` to see exactly what you're looking at.
Use `gf` to jump from Claude's output back to the file list. It's bidirectional.

### 💾 Session restore

spyc auto-saves on quit. `spyc -r` opens a session picker with:

- All pane tabs restored (command, cwd, label)
- Claude conversations resumed via `--resume <sessionId>`
- Human-readable timestamps ("2 hours ago", "3 days ago")

## 📋 Configuration

spyc reads `.spycrc.toml` from `~/.spycrc.toml` (user) and `./.spycrc.toml`
(project). Supports keymap DSL, color overrides, ignore masks, and live reload
(`^R`).

```toml
# Example: remap Space to toggle pick
[keymap]
bindings = [
    'map "<Space>" pick',
]

# Example: hide node_modules in mask 2
[masks]
mask2 = ["node_modules", "target", ".git", "__pycache__"]
```

## 🖥️ Recommended setup

- **Terminal:** [iTerm2](https://iterm2.com/) (macOS), WezTerm, Kitty, Ghostty, or Alacritty
- **Font:** MesloLGS Nerd Font — `brew install --cask font-meslo-lg-nerd-font`
- **Claude Code:** `npm install -g @anthropic-ai/claude-code`
- Press **C** in spyc to toggle mono mode if your font lacks powerline glyphs

See [INSTALL.md](INSTALL.md) for detailed setup instructions.

## 📖 More docs

- [FEATURES.md](FEATURES.md) — complete feature reference
- [INSTALL.md](INSTALL.md) — terminal, font, build, and cross-compilation setup
- [CHANGELOG.md](CHANGELOG.md) — release history
- [ROADMAP.md](ROADMAP.md) — what's next
- [CONTRIBUTING.md](CONTRIBUTING.md) — contribution guidelines and SemVer policy

## 📄 License

BSD-3-Clause. Logo uses [Twemoji](https://github.com/jdecked/twemoji) pepper
artwork (CC-BY 4.0).
