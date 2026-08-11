<p align="center">
  <img src="docs/assets/spyc-logo.png" alt="spyc logo" width="128">
</p>

<h1 align="center">spyc</h1>

<p align="center">
  The file commander built for collaborating with your coding agents.
</p>

<p align="center">
  Keyboard-driven · MCP-native · Rust · macOS and Linux
</p>

<p align="center">
  <a href="https://github.com/Tripstack-Corp/spyc/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Tripstack-Corp/spyc/ci.yml?branch=main&label=CI&style=flat-square" alt="CI"></a>
  <a href="https://github.com/Tripstack-Corp/spyc/actions/workflows/audit.yml"><img src="https://img.shields.io/github/actions/workflow/status/Tripstack-Corp/spyc/audit.yml?label=audit&style=flat-square" alt="Audit"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-BSD--3--Clause-blue?style=flat-square" alt="License: BSD-3-Clause"></a>
  <img src="https://img.shields.io/badge/rust-1.96-orange?style=flat-square&logo=rust" alt="Rust 1.96">
</p>

<p align="center">
  <img src="docs/assets/demo.gif" alt="spyc demo: pick three docs, ask the agent about them over MCP, gf jumps to the answer" width="760">
</p>

---

## Why spyc?

Put an AI coding agent in your terminal and you usually get a chat
window. You still describe your working tree to it, paste paths back
and forth, and lose track of what it's looking at. spyc runs the agent
in a pane beside a keyboard-driven file commander, on macOS and Linux,
and gives it live, structured access to exactly what you're looking at
via a local MCP socket.

The agent can ask spyc *what is the cursor on, what is staged, what is
pinned, what is in this directory* — no copy-paste, no path description.
Pick three files and ask a question; the agent sees your selection. When
it mentions a path in its response, press `gf` to jump straight there.

The file manager is the shared workspace where you and your agents actually
work — not a file list bolted onto a chat window.

## What it is

A two-pane terminal program:

- The **top pane** is a keyboard-driven, vim-flavoured file commander with
  git-aware listings.
- The **bottom pane** is a child process — Claude Code by default (Codex,
  Antigravity, and zot are first-class too), but in practice any program.

The panes share focus through a screen-style `^a` chord prefix, and the
commander exposes a local MCP socket the agent connects to. Everything else
(vi motions, marks, picks, inventory, pager, shell integration) is what you'd
expect from a keyboard-driven file manager — the MCP bridge is what sets spyc
apart from Yazi, Broot, or Ranger.

**The name.** Say it *"spy-see"* — near enough to *spicy*, which is where the
chili comes from. It carries a lineage too: `spy` and the keyboard-driven file
commanders that came before it, rebuilt from scratch in Rust for the age of
coding agents.

<sub>spyc is an independent project, not affiliated with or endorsed by Side Effects Software Inc. or Anthropic.</sub>

## A tour

The bridge above is what makes spyc different. This is the rest of it — the
commander you spend the day in.

### Read anything, one key

<img src="docs/assets/demo-pager.gif" alt="Opening README.md as rendered markdown, then a Rust file with syntax highlighting, then a binary as a hex dump — all in the same pager" width="820">

Markdown renders — headings, tables, fenced code — source arrives
syntax-highlighted, and a binary falls back to a hex dump. Same key every time,
no editor, and the file list is still right there. Images open as actual
pictures on a terminal with a graphics protocol, and `^s |` puts any of it in a
live-reloading side column.

### Keep a file open beside the list

<img src="docs/assets/demo-vsplit.gif" alt="A preview column opening on a markdown file, swapping it to a Rust file, changing height, then closing" width="820">

`^s |` (or `^a |`) opens a preview column on the cursor file, full-height by
default — set `[layout] vsplit_mode = "top_only"` to keep the agent pane
full-width below instead, and `^s f` flips an open split either way. Press the
key on a *different* file and the preview swaps to that one; press it again on
the same file and it closes. The previewed file re-renders when it changes on
disk, so it doubles as a watch window while something else writes.

### Review it where you are

<img src="docs/assets/demo-review.gif" alt="The git gutter marking modified files, gd opening a syntax-highlighted side-by-side diff, | toggling unified, gb blaming the file" width="820">

The gutter marks what changed; `gd` opens a syntax-highlighted diff against
HEAD, `|` toggles side-by-side and unified, `gb` blames the file. All of it
in-process through gix — spyc never shells out to `git`.

### Script it in Lua

<img src="docs/assets/demo-lua.gif" alt="Reading a Lua script in the pager, then pressing the key it is bound to and watching every file with a TODO get picked" width="820">

`map T lua todos` binds a key to a script. This one runs spyc's own
gitignore-aware content search, then picks every file with a TODO left in it —
and `=!` narrows the listing to exactly those. Scripts run off the main thread
behind a kill switch, and `init.lua` can register `:` commands and event hooks
too.

### See which agent needs you

<img src="docs/assets/demo-agents.gif" alt="Two claude tabs running; one keeps working while the other blocks on a question, turning its tab dot red and pulsing the window border" width="820">

Two agents, one question: which one is waiting on *you*. Each tab carries a
live dot — pulsing while it works, settling to a hot-red square the moment it
blocks — and that transition rings a spice-heat border pulse and a desktop
notification, so you can be in another window and still get pulled back to the
right pane. It's driven by the agent reporting its own status over MCP, not by
scraping the screen.

## Quick start

### Prerequisites

- **A coding agent** — Claude Code is the default
  (`npm install -g @anthropic-ai/claude-code`); Codex and Antigravity also work. spyc runs as a plain file manager without one, but the agent
  bridge is the whole point.
- **Nerd Font** (recommended) for the powerline status bar; press `C` inside
  spyc for a mono fallback. Install: `brew install --cask font-meslo-lg-nerd-font`
- **Linux clipboard helper** — yank needs `wl-copy` (Wayland) or `xclip` /
  `xsel` (X11); macOS uses the built-in `pbcopy`. See
  [INSTALL.md](INSTALL.md#clipboard-helper-linux-only).
- **Rust** 1.88+ — only if you build from source (see [BUILD.md](BUILD.md)).

### Install

Pre-built, signed binaries — no Rust toolchain needed:

```sh
# macOS & Linux — Homebrew
brew install Tripstack-Corp/tap/spyc

# Debian / Ubuntu — apt (signed repo)
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://tripstack-corp.github.io/spyc/KEY.gpg | sudo tee /etc/apt/keyrings/spyc.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/spyc.asc] https://tripstack-corp.github.io/spyc ./" | sudo tee /etc/apt/sources.list.d/spyc.list >/dev/null
sudo apt update && sudo apt install spyc
```

Or grab a tarball from
[Releases](https://github.com/Tripstack-Corp/spyc/releases), or — with a Rust
toolchain — install from [crates.io](https://crates.io/crates/spyc) (also the
easy path on Arch and any distro without a native package):

```sh
cargo install spyc
```

To **build from source** instead, see [BUILD.md](BUILD.md). Full setup —
terminal, font, clipboard, MCP, and verification — is in [INSTALL.md](INSTALL.md).

### Running CURRENT (the development stream)

Everything above installs a **RELEASE** — a tagged, signed, frozen version. If
you'd rather run ahead of it, `main` is spyc's **CURRENT** stream: every merged
change lands there, gated by CI. It's what the author dog-foods daily, and it's
where a fix reaches you first.

It is also, by definition, unreleased: rolling, and it may break. There are no
prebuilt binaries — CURRENT is source-only, so you need a Rust toolchain.

```sh
# Use single-branch to avoid downloading the heavy gh-pages branch
git clone --single-branch --branch main https://github.com/Tripstack-Corp/spyc.git
cd spyc
make install        # release build → ~/.local/bin/spyc
```

Update it by pulling and rebuilding:

```sh
git pull && make install
```

A CURRENT build says so: the version carries a `-CURRENT` suffix naming the minor
it's heading for.

```sh
$ spyc --version
spyc <x.y.z>-CURRENT (<sha>)
```

The trailing SHA is the exact build: the version line is static for a whole
minor cycle, so it's the only thing that tells two CURRENT builds apart.

That suffix is how you tell a development build from the release of the same
number — `2.1.0-CURRENT` is *on the way to* 2.1.0, never 2.1.0 itself. The
suffix stays put for the whole cycle, so for a bug report you want the commit
too, which `--verbose` prints alongside the toolchain and terminal it saw:

```sh
$ spyc --verbose
🌶️ spyc 2.1.0-CURRENT
  git:     69a5ff1
  built:   …
  rustc:   …
```

On a rolling stream that SHA is what actually identifies your build — quote it.

To go back to a release, install via any channel above; a tagged build simply has
no suffix. Toolchain and cross-compilation detail is in [BUILD.md](BUILD.md); the
full stream model — CURRENT, STABLE, RELEASE, and how releases are cut — is in
[docs/RELEASE_ENGINEERING.md](docs/RELEASE_ENGINEERING.md).

### Launch

```sh
spyc            # opens in the current directory
spyc -r         # resume a previous session (tabs + each agent's conversation)
```

Move with `hjkl`, `Enter` to open a file/dir, `e` for `$EDITOR`, `?` for the
full help overlay.

### Your first 5 minutes

The whole point is the agent in the side pane seeing exactly what you see.
Try it in a git repo:

1. Run `spyc`.
2. Press `t` on two or three files to **pick** them.
3. Press `^\` (Ctrl+Backslash) to open the agent pane — it launches `claude`
   by default (install it first; see Prerequisites).
4. Ask: **"How do these files interact?"** The agent reads your picks over
   MCP — no pasting paths.
5. When it names a file in its answer, press `gf` to jump straight to it.

`^a j` / `^a k` switch focus between the list and the pane.

## The MCP bridge

This is what sets spyc apart. On startup it runs a local MCP server and writes
the agent's config automatically — no flags, no setup. The agent can then ask
spyc, at any point:

- **What you're looking at** — `get_spyc_context`: cwd, cursor file, picks,
  inventory, active filter, git branch.
- **Where things are** — `search_paths` / `search_content` (gitignore-aware),
  plus `search_picks` and `search_inventory` for state generic filesystem tools
  can't see.

Press `gf` / `gF` to jump from the agent's output back to a file (and line).
Multiple instances coexist safely, and enterprise `managed-mcp.json` policies
are respected — details in [INSTALL.md](INSTALL.md#mcp-configuration).

### Teaching the agent to use it

The MCP handshake tells an agent these tools exist, but it has to stay short.
For the depth — worktree lifecycle, which of the four search corpora to reach
for, the three `git_diff` scopes — install spyc's skill:

```sh
spyc --install-skill
#  → ~/.claude/skills/spyc/          (Claude Code)
#  → ~/.codex/skills/spyc/           (codex; honors $CODEX_HOME)
```

Claude Code and codex use the same skill format, so one install covers both, and
each picks it up automatically in every project.

spyc offers a `[Y/n]` update on startup when its own copy has moved past what
you installed; decline and it won't ask again until the skill actually changes.
If you've edited the installed copy, it says so rather than overwriting your work
silently. Manage it in-app with `:skill` (`status`, `update`, `remove`, `ask`).

## Running multiple agents

Run agents across several tabs and two problems show up; spyc handles both.

- **Which one needs me?** Each tab carries a live **activity dot** — a hot
  pulse while the agent works, a settled square when it's **blocked** (waiting
  on you) or **done**. The agent reports its own state over MCP, so it's right
  even while it redraws its UI. A transition into blocked/done fires a desktop
  notification naming the tab plus a brief border flash, so the nudge reaches
  you in another window. Tunable under `[notify]`.
- **Two agents, same files?** An advisory **scope registry** lets agents
  declare what they're editing or merging (`register_scope` / `list_scopes` /
  `wait_for_scope_clear`) so parallel agents stay out of each other's way — no
  daemon, in-memory, persisted across `-r`.

Sessions auto-save (and re-save seconds after any change, so a crash loses
almost nothing); `spyc -r` restores every tab and resumes each agent's
conversation. Full design:
[`docs/AGENT_ORCHESTRATION.md`](docs/AGENT_ORCHESTRATION.md).

## Keybindings

The essentials below. Press `?` in spyc for the full overlay, or see
[docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) for the complete map.

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Move (counts work: `5j`) |
| `Enter` / `e` | Open in pager / open in `$EDITOR` |
| `t` | Pick / unpick a file (multi-select) |
| `^\` or `F10` | Toggle the agent pane |
| `^a j` / `^a k` | Switch focus between list and pane |
| `^a s` | Send picked paths to the pane |
| `gf` / `gF` | Jump from pane output to a file (+ line) |
| `F` / `:grep` | Fuzzy filename finder / project content search |
| `?` | Full help overlay |
| `q` | Quit |

## Configuration

spyc reads `.spycrc.toml` from `~/.spycrc.toml` (user) and `./.spycrc.toml`
(project); changes apply live (`^R` to force). Bootstrap a fully-commented
config with every default:

```sh
spyc --print-config > ~/.spycrc.toml
```

You can rebind keys, set colors and layout, tune agent notifications, and
script with Lua. Full reference: [`CONFIGURATION.md`](CONFIGURATION.md).

> **Shell users — `^a` and `^w` are reserved.** spyc intercepts them as chord
> prefixes, so a shell (or tmux) running inside the pane won't see readline's
> `beginning-of-line` / `unix-word-rubout`. If you run an interactive shell as
> the pane child, rebind the prefixes in `.spycrc.toml`; inside tmux, keep
> spyc's `^a` distinct from tmux's prefix (or `set -s escape-time 0` for snappy
> input).

> **Project configs are sandboxed.** A `./.spycrc.toml` can set colors, layout,
> ignore masks, and rebind keys to built-in actions — but its *executing*
> bindings (`unix` shell commands, `lua` scripts, `jump`) are ignored; only your
> `~/.spycrc.toml` may bind those. So opening spyc in a cloned repo can't run
> commands a malicious `.spycrc.toml` planted there.

## Recommended setup

- **Terminal:** [iTerm2](https://iterm2.com/) (macOS), WezTerm, Kitty, Ghostty, or Alacritty
- **Font:** Any [Nerd Font](https://www.nerdfonts.com/) for the powerline status bar.
  Press `C` to toggle mono mode if you prefer not to install one.
- **Claude Code:** `npm install -g @anthropic-ai/claude-code`
- **Platforms:** macOS and Linux (x86_64, aarch64). Windows via WSL.

See [INSTALL.md](INSTALL.md) for detailed setup instructions.

## More docs

- [FEATURES.md](FEATURES.md) -- complete feature reference
- [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) -- the full keymap (the `?` overlay in browsable form)
- [CONFIGURATION.md](CONFIGURATION.md) -- config reference: `.spycrc.toml`, notifications, keymap DSL, Lua
- [INSTALL.md](INSTALL.md) -- install (Homebrew, apt, binary), terminal, font, clipboard, and MCP setup
- [BUILD.md](BUILD.md) -- build from source: Rust toolchain, `make install`, cross-compilation
- [ARCHITECTURE.md](ARCHITECTURE.md) -- concurrency model, MVU target shape, persistence, MCP transport
- [docs/HARNESS.md](docs/HARNESS.md) -- running agents in spyc: recommended settings, per-agent screen modes, where scrollback lives, session-recovery quirks
- [docs/AGENT_ORCHESTRATION.md](docs/AGENT_ORCHESTRATION.md) -- how the activity dots, notifications, session-resume, and scope registry fit together
- [DESIGN.md](DESIGN.md) -- UI design language: components, surfaces, palette, extension checklist
- [CHANGELOG.md](CHANGELOG.md) -- release history
- [ROADMAP.md](ROADMAP.md) -- strategy, direction, and the decisions log
- [CONTRIBUTING.md](CONTRIBUTING.md) -- contribution guidelines and SemVer policy
- [Issues](https://github.com/Tripstack-Corp/spyc/issues) -- the live backlog: bugs, features, and ideas (labeled, on the [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1))

## License

BSD-3-Clause. Logo uses [Twemoji](https://github.com/jdecked/twemoji) pepper
artwork (CC-BY 4.0).
