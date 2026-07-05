# Installing spyc

Pre-built, signed binaries are the easy path — Homebrew, `apt`, or a
Release tarball, no Rust toolchain required. To compile spyc yourself
(to hack on it or run unreleased changes), see [BUILD.md](BUILD.md).

## Install

### Homebrew (macOS and Linux)

```sh
brew install Tripstack-Corp/tap/spyc
```

Installs the latest signed release for your platform (macOS universal,
Linux x86_64 / aarch64) and upgrades with `brew upgrade spyc`. Homebrew
works on Linux too (Linuxbrew), if you'd rather not use `apt`. To build
the tip of `main` from source through the tap instead:

```sh
brew install --HEAD Tripstack-Corp/tap/spyc
```

### Debian / Ubuntu (apt)

spyc publishes a signed apt repository. Add it once, then install and
upgrade through your package manager like any other package:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://tripstack-corp.github.io/spyc/KEY.gpg \
  | sudo tee /etc/apt/keyrings/spyc.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/spyc.asc] https://tripstack-corp.github.io/spyc ./" \
  | sudo tee /etc/apt/sources.list.d/spyc.list >/dev/null
sudo apt update
sudo apt install spyc
```

`sudo apt upgrade` picks up new releases from then on. Both `amd64` and
`arm64` are published.

### Pre-built binary (any platform)

Download a tarball from the
[Releases page](https://github.com/Tripstack-Corp/spyc/releases), verify
it, and put the binary on your `PATH`:

```sh
# pick the asset for your platform:
#   spyc-<tag>-macos-universal.tar.gz   arm64 + x86_64
#   spyc-<tag>-linux-x86_64.tar.gz      static, musl
#   spyc-<tag>-linux-aarch64.tar.gz     static, musl
tar xzf spyc-<tag>-<platform>.tar.gz
install -m 755 spyc ~/.local/bin/
```

Every release ships a `SHA256SUMS` file plus keyless signatures — verify
before installing:

```sh
sha256sum -c SHA256SUMS --ignore-missing        # checksum
gh attestation verify spyc-<tag>-<platform>.tar.gz --repo Tripstack-Corp/spyc
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github.com/Tripstack-Corp/spyc/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Make sure `~/.local/bin` is on your `PATH` (see below).

### Build from source

To compile spyc yourself — to hack on it or run unreleased changes —
see **[BUILD.md](BUILD.md)** for the Rust toolchain, `make install`, and
cross-compilation. Make sure `~/.local/bin` is on your `PATH`; on macOS
add this to `~/.zshrc` (or `~/.bash_profile`):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Terminal

spyc is designed for modern terminals with true-color (24-bit) and
Nerd Font glyph support. We recommend **iTerm2** on macOS:

```sh
brew install --cask iterm2
```

iTerm2 supports true-color, Nerd Font glyphs, the mouse-pointer-hide
escape (`XTSMPOINTER`), and is the most tested terminal for spyc.

Other terminals that work well: WezTerm, Kitty, Alacritty, Ghostty.
The default macOS Terminal.app supports true-color but lacks Nerd Font
glyphs without a patched font.

## Font

The powerline status bar uses glyphs from a Nerd Font. Install one via
Homebrew:

```sh
brew install --cask font-meslo-lg-nerd-font
```

Then set your terminal's font to **MesloLGS Nerd Font** (or
**MesloLGM Nerd Font Mono**). In Ghostty, add to your config
(`~/.config/ghostty/config`):

```
font-family = MesloLGS Nerd Font Mono
font-size = 13
```

Other good Nerd Font options:

```sh
brew install --cask font-fira-code-nerd-font
brew install --cask font-jetbrains-mono-nerd-font
brew install --cask font-hack-nerd-font
```

If you don't install a Nerd Font, spyc still works — the powerline
separators will render as missing-glyph boxes. Toggle to mono mode
with **C** for a plain-text fallback that uses no special glyphs.

## Clipboard helper (Linux only)

The yank-to-clipboard features (`yf`, `yp`, `yP`, `ya`, and pager-side
yanks) need a helper binary to push text onto the system clipboard.
macOS uses the built-in `pbcopy`; Linux needs one of:

- `wl-copy` (Wayland; `sudo apt install wl-clipboard`)
- `xclip` (X11; `sudo apt install xclip`)
- `xsel` (X11; `sudo apt install xsel`)

spyc auto-detects the session — `wl-copy` when `$WAYLAND_DISPLAY` is
set, otherwise `xclip` → `xsel`. With none installed, yanks flash
`yank failed: no clipboard helper available — install xclip, xsel,
or wl-copy`.

## Claude Code (pane default)

The lower pane defaults to running `claude` (Claude Code CLI). Install
it if you haven't:

```sh
npm install -g @anthropic-ai/claude-code
```

Set `SPYC_PANE_CMD` to change the default pane command:

```sh
export SPYC_PANE_CMD="bash"
```

## MCP configuration

spyc runs an MCP server on a PID-scoped Unix domain socket
(`~/.local/state/spyc/mcp-<PID>.sock`) so Claude Code can query and
control the file manager. How it connects depends on whether your
Claude Code installation is managed by an organization or not.

### Unmanaged (personal) environments

No configuration is needed. On startup spyc writes two config files
in the working directory so each agent discovers it automatically:

- **`.mcp.json`** for Claude Code (JSON, `mcpServers.spyc` shape).
- **`.codex/config.toml`** for the codex CLI (TOML,
  `[mcp_servers.spyc]` shape — spyc ensures the entry, preserving
  the rest of the file).

Both registrations re-exec `spyc --mcp` as a stdio proxy to the same
socket, so a single server backs both agents.

The generated `.mcp.json` looks like this:

```json
{
  "mcpServers": {
    "spyc": {
      "command": "/Users/you/.local/bin/spyc",
      "args": ["--mcp"],
      "env": {
        "SPYC_MCP_SOCK": "/Users/you/.local/state/spyc/mcp-12345.sock"
      }
    }
  }
}
```

- **`command`** — path to the spyc binary (auto-detected from
  the running executable).
- **`args`** — `--mcp` runs spyc in stdio MCP proxy mode. Claude
  Code spawns this process; it proxies JSON-RPC to the running
  instance's Unix socket.
- **`env.SPYC_MCP_SOCK`** — tells the proxy which socket to
  connect to (PID-scoped, so multiple instances don't collide).

**You should not need to edit these files.** spyc manages them
automatically, including:

- **Instance takeover** — if a second spyc opens in the same
  directory, it updates both files to point at its own socket and
  notifies the old instance.
- **Cleanup** — the socket file is removed on normal exit.

Both are runtime artifacts — add them to `.gitignore`:

```
.mcp.json
.codex/
```

spyc's own repo already has these entries.

### Enterprise managed environments

In enterprise environments, Claude Code supports two system-wide
configuration files that IT can deploy to all machines. Both live in
the same directory:

| Platform   | Directory                                          |
|------------|----------------------------------------------------|
| macOS      | `/Library/Application Support/ClaudeCode/`         |
| Linux/WSL  | `/etc/claude-code/`                                |

#### `managed-mcp.json` — deploy MCP servers centrally

This file pushes MCP server entries to every Claude Code installation
on the machine, so individual users don't need a per-project
`.mcp.json`. To include spyc alongside other org-wide MCP servers:

```json
{
  "mcpServers": {
    "spyc": {
      "command": "/usr/local/bin/spyc",
      "args": ["--mcp"]
    }
  }
}
```

(In a managed deployment, IT typically installs the binary system-wide
to `/usr/local/bin` via `sudo make install PREFIX=/usr/local`.
For per-user installs, the path is `$HOME/.local/bin/spyc`.)

Note that the centrally deployed entry does **not** include
`env.SPYC_MCP_SOCK` (IT doesn't know the PID of each user's running
instance). The stdio proxy handles this automatically via
**project-scoped discovery**: it walks the caller's working directory
upward looking for `.spyc-context-<pid>.json` markers (each written
by a running spyc rooted at that directory) and connects to a live
socket from the first ancestor that has one. A spyc running in a
*different* project tree is never picked up — cross-project
attachment is refused — and if no marker matches, the proxy falls
back to read-only direct mode.

When spyc is deployed via `managed-mcp.json`, the per-directory
`.mcp.json` that spyc writes on startup is still useful — it carries
the exact socket path for faster, deterministic connections. The two
configs coexist without conflict.

#### `managed-settings.json` — control which servers are allowed

This file controls permissions and policies. spyc checks it before
writing `.mcp.json`:

- **`deniedMcpServers`** — if `"spyc"` appears in the denylist,
  the MCP server will not configure itself. A flash message warns
  in the TUI: *"MCP: blocked by enterprise policy"*.
- **`allowedMcpServers`** — if an allowlist is present, `"spyc"`
  must be included or configuration is blocked.
- If neither list mentions spyc, it is allowed by default.

To allow spyc in a managed environment:

```json
{
  "allowedMcpServers": [
    { "serverName": "spyc" }
  ]
}
```

## Verifying the setup

Launch spyc and check:

1. **Powerline bar** — status line at the top should show colored
   segments with arrow separators. If you see boxes instead of arrows,
   your font doesn't have powerline glyphs.
2. **Colors** — file listing should be color-coded (blue dirs, green
   executables). If everything is white, your terminal may not support
   true-color.
3. **Pane** — press `^\` (Ctrl+Backslash) to open the lower pane. It
   should spawn `claude` (or whatever `SPYC_PANE_CMD` is set to).
4. **MCP** — in the Claude pane, Claude should have access to spyc's
   MCP tools (`get_spyc_context`, `navigate_to`, etc.). Ask Claude
   "what file am I looking at?" to verify the connection.
5. **Ctrl+J** — in the pane with Claude, Ctrl+J should insert a
   newline for multi-line input.
