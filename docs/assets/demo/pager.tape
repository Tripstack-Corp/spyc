# ─────────────────────────────────────────────────────────────────────────────
# spyc tour — "read anything, fast": one pager, three kinds of file.
#
#   Story:  README.md opens rendered (headings, table, fenced code) → src/
#           parser.rs opens syntax-highlighted → docs/logo.png opens as an
#           actual image in the terminal. Same key every time, no editor.
#
#   Generate (from the repo root):
#       make demos            # or: vhs docs/assets/demo/pager.tape
#
#   Notes:
#     • `/foo` + Enter moves the cursor to a match; a second Enter descends a
#       dir or opens a file. Two separate Enters — don't collapse them.
#     • `q` closes the pager AND the image overlay, so every beat exits the
#       same way.
#     • The fixture (docs/assets/demo/demo-env.sh) repoints HOME, so the theme,
#       session name and status bar are identical on any machine.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-pager.gif

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

# ── 1. The listing: a real tree, git gutter on the edited files ──────────────
Sleep 1.8s

# ── 2. README.md — rendered markdown, not raw text ───────────────────────────
Type "/"
Sleep 250ms
Type "README"
Sleep 350ms
Enter
Sleep 500ms
Enter
Sleep 3s
Ctrl+D
Sleep 2.2s
Type "q"
Sleep 900ms

# ── 3. src/parser.rs — syntax highlighted, same key ──────────────────────────
Type "/"
Sleep 250ms
Type "src"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 900ms
Type "/"
Sleep 250ms
Type "parser"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 3.2s
Type "q"
Sleep 700ms
# Climb back to the repo root. `u` (or `-`), NOT `h` — `h` toggles hidden files.
Type "u"
Sleep 900ms

# ── 4. data/index.bin — a binary, dumped as hex by the same pager ────────────
#      (An image file opens as an actual picture here; that beat is NOT in this
#      tape because ttyd/xterm.js has no graphics protocol, so VHS records it
#      blank. It needs a screenshot from a real terminal instead.)
Type "/"
Sleep 250ms
Type "data"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 900ms
Enter
Sleep 3.5s
Type "q"
Sleep 1.5s
