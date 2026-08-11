# ─────────────────────────────────────────────────────────────────────────────
# spyc tour — "look without leaving": the vertical split, end to end (`^s`).
#
#   Story:  an agent sits in the pane below. `^s |` opens a right-hand column
#           previewing the cursor file, FULL-HEIGHT — the reading surface the
#           default is tuned for → `^s f` flips it to top-only, handing the pane
#           its full width back, and `^s f` again returns → navigate elsewhere
#           and press `^s |` to SWAP the preview to that file → `^a b` to read
#           it (G / g scroll it), `^a a` back → `^a +` widens the column →
#           `^s |` closes it → `^s n` replaces the preview with a SECOND full
#           commander in `b`, which navigates on its own → `^s f` gives that one
#           the full height too → `^s x` closes it.
#
#   Generate (from the repo root):
#       make demos TAPE=vsplit    # or: vhs docs/assets/demo/vsplit.tape
#
#   Why the pane is open the whole time: full-height vs top-only IS the pane's
#   width. Full-height runs the divider the whole frame height and confines the
#   pane under column `a`; top-only splits just the list and leaves the pane
#   full-width below both columns. With no pane below, `^s f` reads as nothing
#   more than the status bar changing length.
#
#   What `^s |` does depends on where the cursor is (`vsplit.rs`): closed →
#   open on the cursor file; open with a DIFFERENT previewable file under the
#   cursor → swap to it; open on the SAME file → close. Moving the cursor alone
#   changes nothing, so a beat that only presses `j` records a preview that
#   looks stuck. ("Live reload" is about the previewed file changing on DISK —
#   that's `preview_ops`, not cursor movement.)
#
#   `^a |` / `^a f` alias `^s |` / `^s f` for exactly the state this tape opens
#   in — a focused pane intercepts only the `^a` family. The tape presses the
#   `^s` spellings because they're where the family lives; the aliases are in
#   `?` and KEYBINDINGS.md.
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

# ── Hidden setup: the fixture project, then an agent pane below (scenery — it
#    waits on its prompt), shrunk to ~45% so the file region has room, with the
#    keyboard handed back to the list.
#
#    The pane height matters to what this tape is about: top-only carves the
#    *list region*, so against the default pane share the right column comes out
#    a few rows tall and the beat reads as "the preview got crushed" instead of
#    "the pane got its full width back". `^a -` here resizes the pane (no split
#    is open yet — once one is, `^a -` narrows the split instead). ─────────────
Hide
Type `export SPYC_PANE_CMD="$PWD/docs/assets/demo/fake-claude.sh"`
Enter
Type "source docs/assets/demo/demo-env.sh"
Enter
Type "spyc"
Enter
Sleep 4s
Ctrl+A
Sleep 250ms
Type "c"
Sleep 800ms
Enter
Sleep 600ms
Enter
Sleep 2.5s
Ctrl+A
Sleep 250ms
Type "k"
Sleep 600ms
Ctrl+A
Sleep 200ms
Type "-"
Ctrl+A
Sleep 200ms
Type "-"
Ctrl+A
Sleep 200ms
Type "-"
Ctrl+A
Sleep 200ms
Type "-"
Sleep 800ms
Show

# ── 1. Cursor on the markdown file ──────────────────────────────────────────
Sleep 1.5s
Type "/"
Sleep 250ms
Type "README"
Sleep 350ms
Enter
Sleep 1.2s

# ── 2. ^s | — the preview column opens FULL-HEIGHT, rendering it. The divider
#       runs the whole frame; the pane pulls in under column a. ──────────────
Ctrl+S
Sleep 300ms
Type "|"
Sleep 3.4s

# ── 3. ^s f — flip the height: top-only splits just the list, handing the pane
#       its full width back. Done here, in the full listing, because that's
#       where the two shapes visibly differ. ──────────────────────────────────
Ctrl+S
Sleep 300ms
Type "f"
Sleep 3s

# ── 4. ^s f again — back to the full-height reading column ──────────────────
Ctrl+S
Sleep 300ms
Type "f"
Sleep 2.4s

# ── 5. Navigate into src/ — the preview stays put while you move ────────────
Type "/"
Sleep 250ms
Type "src"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 1s
Type "/"
Sleep 250ms
Type "parser"
Sleep 350ms
Enter
Sleep 1.2s

# ── 6. ^s | on a different file — the preview SWAPS to it, highlighted ───────
Ctrl+S
Sleep 300ms
Type "|"
Sleep 2.8s

# ── 7. ^a b — the reading column takes the keyboard: G to the end, g back to
#       the top. ^a a returns to the list. ─────────────────────────────────────
Ctrl+A
Sleep 300ms
Type "b"
Sleep 800ms
Type "G"
Sleep 1.6s
Type "g"
Sleep 1.2s
Ctrl+A
Sleep 300ms
Type "a"
Sleep 800ms

# ── 8. ^a + — grow the column you're reading in (the split's WIDTH, since a
#       file column is focused; the same key resizes the pane's height when the
#       pane is). ──────────────────────────────────────────────────────────────
Ctrl+A
Sleep 250ms
Type "+"
Sleep 400ms
Ctrl+A
Sleep 250ms
Type "+"
Sleep 2s

# ── 9. ^s | on the same file — close it ─────────────────────────────────────
Ctrl+S
Sleep 300ms
Type "|"
Sleep 1.6s

# ── 10. ^s n — not a preview but a SECOND commander in b: its own cwd, cursor
#       and listing. It opens focused, so it navigates on its own. ───────────
Ctrl+S
Sleep 300ms
Type "n"
Sleep 2.2s
Type "/"
Sleep 250ms
Type "docs"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 1.6s

# ── 11. ^s f — the same flip reaches a commander split, which opens top-only
#        whatever the config says: b takes the full height. ──────────────────
Ctrl+S
Sleep 300ms
Type "f"
Sleep 2.8s

# ── 12. ^s x — close b, back to one column ─────────────────────────────────
Ctrl+S
Sleep 300ms
Type "x"
Sleep 2s
