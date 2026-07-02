# ─────────────────────────────────────────────────────────────────────────────
# spyc demo — the bidirectional MCP bridge in one take.
#
#   Story:  open the agent  →  pick three docs in the commander  →  ask the
#           agent about "these" (never naming them)  →  it reads your picks over
#           MCP and answers with a path  →  gf jumps the listing to that file.
#
#   Generate (from the repo root):
#       brew install vhs
#       chmod +x docs/assets/demo/fake-claude.sh
#       vhs docs/assets/demo/spyc.tape          # writes docs/assets/demo.gif
#
#   Hard-won keybinding / behavior notes (verified live, NOT from the README):
#     • `d` is a chord-arming key (dd = delete). Descend is Enter-only.
#     • `^a c` creates the pane via TWO pre-filled prompts (command, then cwd) —
#       Enter each. Bare ^\ / ^a \ only TOGGLE an existing pane; Ctrl+\ (SIGQUIT)
#       and F10 don't survive the VHS/ttyd path.
#     • `^a j` focuses the pane, `^a k` focuses the list.
#     • gf resolves the bottom-most path in the pane's *visible* lines against
#       the pane cwd. The pane cwd follows the focused commander, so we keep the
#       list at the repo root: repo-relative paths (src/app/run.rs) then resolve
#       and gf jumps INTO the tree. The agent reply is kept short so its closing
#       path stays on screen (an off-screen path is invisible to gf).
#
#   Tuning: Sleep values are reading-beats — bump them if a step feels rushed.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo.gif

# ── Look & feel ──────────────────────────────────────────────────────────────
Set Shell        "bash"
Set FontFamily   "MesloLGLDZ Nerd Font Mono"   # falls back if absent; or press C in spyc for mono glyphs
Set FontSize     14
Set Width        1320
Set Height       820
Set Padding      22
Set Margin       18
Set MarginFill   "#181825"
Set WindowBar    Colorful
Set BorderRadius 10
Set Framerate    24
Set Theme        "Catppuccin Macchiato"
Set CursorBlink  false
Set TypingSpeed  55ms

# ── Setup (hidden): launch spyc with the scripted agent, wait for first paint ──
Hide
Type `export SPYC_PANE_CMD="$(pwd)/docs/assets/demo/fake-claude.sh"`
Enter
Type "spyc"
Enter
Sleep 4s
Show

# ── 1. Open the agent pane at the repo root (two pre-filled prompts) ──────────
Ctrl+A
Sleep 250ms
Type "c"
Sleep 800ms
Enter
Sleep 600ms
Enter
Sleep 3s

# ── 2. Focus the list and pick three architecture docs (amber * markers) ──────
#       Staying at the repo root keeps the pane cwd at root so gf resolves.
Ctrl+A
Sleep 250ms
Type "k"
Sleep 700ms

Type "/"
Sleep 300ms
Type "ARCH"
Sleep 400ms
Enter
Sleep 400ms
Type "t"
Sleep 700ms

Type "/"
Sleep 300ms
Type "AGENTS"
Sleep 400ms
Enter
Sleep 400ms
Type "t"
Sleep 700ms

Type "/"
Sleep 300ms
Type "CLAUDE"
Sleep 400ms
Enter
Sleep 400ms
Type "t"
Sleep 1200ms

# ── 3. Focus the pane and ask about "these three docs" — never naming them ────
Ctrl+A
Sleep 250ms
Type "j"
Sleep 700ms
Type "how is spyc structured, per these three docs?"
Sleep 500ms
Enter

# Wait out the scripted tool call + short response
Sleep 4s

# ── 4. Focus the list, then gf jumps to the path in the answer ────────────────
#       (src/app/run.rs — repo-relative, resolves against the root pane cwd,
#        and lands in src/app/, so the listing visibly changes)
Ctrl+A
Sleep 250ms
Type "k"
Sleep 1s
Type "gf"
Sleep 1.5s

# Hold the landing frame
Sleep 3.5s
