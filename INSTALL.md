# Installing spyc

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

## Rust toolchain

spyc is written in Rust. Install the toolchain via rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Minimum supported Rust version: **1.80**.

## Build and install

```sh
git clone https://bitbucket.org/tripstack/spyc.git
cd spyc
make release          # build optimized binary
sudo make install     # copy to /usr/local/bin
```

Or build manually:

```sh
cargo build --release
sudo install -m 755 target/release/spyc /usr/local/bin/
```

## Cross-compilation (optional)

To build for Linux or create a macOS universal binary, you'll need a
few extra tools:

```sh
brew install zig
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
rustup target add x86_64-apple-darwin aarch64-apple-darwin
```

Then use the Makefile targets:

```sh
make dist                    # all platforms → dist/
make release-macos-universal # macOS universal (arm64 + x86_64)
make release-linux-x86       # Linux x86_64 (static, musl)
make release-linux-arm       # Linux aarch64 (static, musl)
make deploy-fika             # scp to a remote host
```

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

No configuration is needed. spyc writes a `.mcp.json` in the working
directory on startup and Claude Code discovers it automatically.

The generated `.mcp.json` looks like this:

```json
{
  "mcpServers": {
    "spyc": {
      "command": "/usr/local/bin/spyc",
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

**You should not need to edit this file.** spyc manages it
automatically, including:

- **Instance takeover** — if a second spyc opens in the same
  directory, it updates `.mcp.json` to point at its own socket and
  notifies the old instance.
- **Cleanup** — the socket file is removed on normal exit.

`.mcp.json` is a runtime artifact — add it to `.gitignore`:

```
.mcp.json
```

spyc's own repo already has this entry.

### Enterprise managed environments

Organizations can deploy Claude Code with a `managed-settings.json`
that controls which MCP servers are allowed. spyc checks this policy
before writing `.mcp.json`:

- **`deniedMcpServers`** — if `"spyc"` appears in the denylist,
  the MCP server will not configure itself. A flash message warns
  in the TUI: *"MCP: blocked by enterprise policy"*.
- **`allowedMcpServers`** — if an allowlist is present, `"spyc"`
  must be included or configuration is blocked.
- If neither list mentions spyc, it is allowed by default.

Managed settings are read from:

| Platform   | Path                                                        |
|------------|-------------------------------------------------------------|
| macOS      | `/Library/Application Support/ClaudeCode/managed-settings.json` |
| Linux/WSL  | `/etc/claude-code/managed-settings.json`                    |

To allow spyc in a managed environment, add it to the allowlist:

```json
{
  "allowedMcpServers": [
    { "serverName": "spyc" }
  ]
}
```

When `SPYC_MCP_SOCK` is not set — as is the case when the MCP server
entry comes from a centrally deployed `managed-settings.json` rather
than spyc's own `.mcp.json` — the stdio proxy falls back to
**discovery mode**: it scans `~/.local/state/spyc/mcp-*.sock` for
any live instance and connects to the first one it finds.

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
