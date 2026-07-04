# ─────────────────────────────────────────────────────────────────────────────
# spyc demo — the in-process review loop (gd), in a short throwaway repo.
#
#   Story:  a file shows the git gutter (~ modified) → gd opens the in-house,
#           syntax-highlighted side-by-side diff vs HEAD → | toggles unified →
#           q closes. No git subprocess; all gix, in the pager.
#
#   Generate (from the repo root):
#       vhs docs/assets/demo/review.tape        # writes docs/assets/demo-review.mp4
#
#   MP4 output (crisp). Runs in a short /tmp repo so nothing touches your tree
#   and the status bar stays clean. DRAFT — record + tune the Sleep beats.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-review.mp4

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

# ── Hidden setup: a short throwaway repo with one committed file, then a local
#    edit, so `gd` renders a real diff and your working tree is untouched. ─────
Hide
Type `d=/tmp/spyc-demo && rm -rf "$d" && mkdir -p "$d" && cd "$d" && git init -q && git config user.email d@x && git config user.name d && printf 'fn main() {\n    println!("hi");\n}\n' > main.rs && git add -A && git commit -qm init && printf 'fn main() {\n    // greet the room, in colour\n    println!("hello, spyc");\n}\n' > main.rs`
Enter
Type "spyc"
Enter
Sleep 3.5s
Show

# ── 1. Land on the modified file — the git gutter shows it changed ───────────
Sleep 2s

# ── 2. gd — the in-house side-by-side diff vs HEAD (word-level highlighting) ──
Type "gd"
Sleep 3.5s

# ── 3. | — collapse to a unified view ────────────────────────────────────────
Type "|"
Sleep 3s

# ── 4. | — back to side-by-side ──────────────────────────────────────────────
Type "|"
Sleep 2.5s

# ── 5. q — close the review pager, back to the list ─────────────────────────
Type "q"
Sleep 2.5s
