# ─────────────────────────────────────────────────────────────────────────────
# spyc tour — "look without leaving": the live preview column (^a |).
#
#   Story:  `^a |` opens a right-hand column previewing the cursor file →
#           navigate elsewhere and press it again to SWAP the preview to that
#           file → press it on the same file to grow it full-height → once more
#           cycles it off.
#
#   Generate (from the repo root):
#       make demos            # or: vhs docs/assets/demo/vsplit.tape
#
#   What `^a |` does depends on where the cursor is (`vsplit.rs`): closed →
#   open on the cursor file; open with a DIFFERENT previewable file under the
#   cursor → swap to it; open on the SAME file → cycle the shape. Moving the
#   cursor alone changes nothing, so a beat that only presses `j` records a
#   preview that looks stuck. ("Live reload" is about the previewed file
#   changing on DISK — that's `preview_ops`, not cursor movement.)
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-vsplit.gif

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

Hide
Type "source docs/assets/demo/demo-env.sh"
Enter
Type "spyc"
Enter
Sleep 4s
Show

# ── 1. Cursor on the markdown file ──────────────────────────────────────────
Sleep 1.5s
Type "/"
Sleep 250ms
Type "README"
Sleep 350ms
Enter
Sleep 1.2s

# ── 2. ^a | — the preview column opens, rendering it ────────────────────────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 3.2s

# ── 3. Navigate into src/ — the preview stays put while you move ────────────
Type "/"
Sleep 250ms
Type "src"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 1.2s
Type "/"
Sleep 250ms
Type "parser"
Sleep 350ms
Enter
Sleep 1.4s

# ── 4. ^a | on a different file — the preview SWAPS to it, highlighted ──────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 3.2s

# ── 5. ^a | on the same file — grow it full-height ──────────────────────────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 3.2s

# ── 6. ^a | once more — cycle it off ────────────────────────────────────────
Ctrl+A
Sleep 300ms
Type "|"
Sleep 2s
