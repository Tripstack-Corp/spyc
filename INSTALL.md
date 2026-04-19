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
git clone https://github.com/tripstack/spyc.git
cd spyc
make install        # builds release and installs to ~/bin
```

Or build manually:

```sh
cargo build --release
cp target/release/spyc /usr/local/bin/
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
4. **Ctrl+J** — in the pane with Claude, Ctrl+J should insert a
   newline for multi-line input.
