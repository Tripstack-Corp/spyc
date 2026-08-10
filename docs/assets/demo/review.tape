# ─────────────────────────────────────────────────────────────────────────────
# spyc tour — "review it here": the in-process diff, no git subprocess.
#
#   Story:  the listing carries a two-column git gutter → `gd` opens the
#           in-house, syntax-highlighted diff vs HEAD → `|` collapses it to
#           unified and back → `gb` blames the file → `q` returns to the list.
#           All gix, in the pager.
#
#   Generate (from the repo root):
#       make demos            # or: vhs docs/assets/demo/review.tape
#
#   The fixture (docs/assets/demo/demo-env.sh) commits the tree and then leaves
#   a real local edit — a whitespace fix in the parser plus the test that pins
#   it — so the diff on screen is one a reviewer would actually read.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-review.gif

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

# ── 1. The gutter: src/ and tests/ carry the modified marker ─────────────────
Sleep 1.5s
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

# ── 2. gd — side-by-side vs HEAD, syntax highlighted, word-level ─────────────
Type "gd"
Sleep 4s

# ── 3. | — collapse to unified, then back ───────────────────────────────────
Type "|"
Sleep 3s
Type "|"
Sleep 2.5s
Type "q"
Sleep 1s

# ── 4. gb — blame the same file, in the same pager ──────────────────────────
Type "gb"
Sleep 4s
Type "q"
Sleep 1.5s
