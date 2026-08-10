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
Output docs/assets/demo.mp4

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
Type `mkdir -p /tmp/spycbin && cp "$PWD/docs/assets/demo/fake-claude.sh" /tmp/spycbin/claude && chmod +x /tmp/spycbin/claude`
Enter
# Private HOME (project fixture skipped — this tape records in spyc's own repo).
# Without it the recorder's spyc theme, session history and the REAL agent's
# statusline leak into the frame; the committed GIF this replaces was recorded
# against a live claude and published a garbled turn plus account usage stats.
Type `DEMO_HOME_ONLY=1 source docs/assets/demo/demo-env.sh`
Enter
Type `export PATH="/tmp/spycbin:$PATH" SPYC_PANE_CMD=claude`
Enter
Type "spyc"
Enter
Sleep 4s
Show

# ── 1. Pick three architecture docs (amber * markers), BEFORE any pane exists ─
#       Order matters. Picking first means the list already has focus — the
#       startup default — so these keys cannot be stolen by a pane. Opening the
#       pane first and then focusing back with `^a k` is what produced the
#       garbled takes: one dropped chord and the whole rest of the tape typed
#       itself into the agent (`> TS`, `thow is spyc structured…`).
#       Staying at the repo root keeps the pane cwd at root so gf resolves.
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

# ── 2. Open the agent pane at the repo root (two pre-filled prompts). Spawning
#       it focuses the pane, so no `^a j` is needed to type into it. ───────────
#       `^a c` raises TWO prompts (command, then cwd) — Enter each.
Ctrl+A
Sleep 400ms
Type "c"
Sleep 1.4s
Enter
Sleep 1.2s
Enter

#       Then WAIT for the agent's banner rather than sleeping at it. The
#       stand-in opens with `clear`, which erases whatever the tty has already
#       echoed — so typing before it lands silently eats the head of the
#       question and leaves `> c structured, per these three docs?` on screen.
#       That's a fixed-length loss, not a slow machine: no Sleep is long enough
#       if the clear happens after typing starts.
Wait+Screen@15s /MCP connected/
Sleep 1.2s

#       Then focus the pane EXPLICITLY. Spawning does not reliably leave focus
#       there by the time the banner is up, and the failure is silent: the head
#       of the question is swallowed by the file list as commands — the `h` in
#       "how is spy" toggles hidden files (watch `hidden:` in the status bar) —
#       leaving `> c structured, per these three docs?` on the agent's prompt.
#       Then hold, hard, before typing. The pane is already focused here (`^a j`
#       flashes nothing, so there is no event to wait on), but keystrokes sent
#       too soon after the banner are DISCARDED — the interactive shell spyc
#       execs the agent through drops type-ahead while it starts. Instrumenting
#       the stand-in proved it: the child READ only "c structured, per these
#       three docs?", so the head was never delivered to anything.
Sleep 3s

# ── 3. Ask about "these three docs" — never naming them ──────────────────────
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
