# ─────────────────────────────────────────────────────────────────────────────
# spyc demo — the vertical split live-preview (^a |).
#
#   Story:  cursor on a markdown file → ^a | cycles a right-hand column: off →
#           top-only → full-height, rendering the file live → cycle it back.
#
#   Generate (from the repo root):
#       vhs docs/assets/demo/vsplit.tape        # writes docs/assets/demo-vsplit.mp4
#
#   MP4 output (crisp). Runs in a short /tmp repo with a rich README to preview,
#   so the status bar stays clean. DRAFT — record + tune the Sleep beats.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-vsplit.mp4

Set Shell        "bash"
Set FontFamily   "MesloLGLDZ Nerd Font Mono"
Set FontSize     16
Set Width        1200
Set Height       720
Set Padding      22
Set Margin       18
Set MarginFill   "#181825"
Set WindowBar    Colorful
Set BorderRadius 10
Set Framerate    24
Set Theme        "Catppuccin Macchiato"
Set CursorBlink  false
Set TypingSpeed  55ms

# ── Hidden setup: a short repo with a rich README to preview ─────────────────
Hide
Type `d=/tmp/spyc-demo && rm -rf "$d" && mkdir -p "$d/src" && cd "$d" && git init -q && printf 'fn main() {}\n' > src/main.rs && printf '[package]\nname = "demo"\n' > Cargo.toml && printf '# spyc\n\nThe file commander built for collaborating with your coding agents.\n\n## Features\n\n- vi navigation, marks, picks\n- MCP bridge to the agent\n- in-process git (gix)\n\n## Install\n\n    make install\n' > README.md`
Enter
Type "spyc"
Enter
Sleep 3.5s
Show

# ── 1. Put the cursor on the markdown file ───────────────────────────────────
Sleep 1s
Type "/"
Sleep 300ms
Type "README"
Sleep 400ms
Enter
Sleep 1.2s

# ── 2. ^a | — open the preview column (top-only), markdown rendered live ─────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 3.5s

# ── 3. ^a | — grow it to full-height ─────────────────────────────────────────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 3.5s

# ── 4. ^a | — cycle the preview back off ─────────────────────────────────────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 2s
