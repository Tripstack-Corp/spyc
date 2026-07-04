# ─────────────────────────────────────────────────────────────────────────────
# spyc demo — agent awareness: which of TWO agents needs you.
#
#   Story:  two `claude` tabs are running. Tab 1 is busy on a long task — its dot
#           pulses ● the whole time. You go back to the file list. Tab 2 hits a
#           decision and reports `blocked`: its tab dot settles to a red ■ and the
#           border rings the spice-heat visual bell — the "this one needs me" cue
#           you can read at a glance without watching either pane. You switch to
#           tab 2, press Enter to acknowledge, and it finishes → done ■.
#
#   Why two panes: with one agent there's nothing to disambiguate. The dot earns
#   its keep only when several agents run at once and one of them needs you.
#
#   Generate (from the repo root):
#       chmod +x docs/assets/demo/fake-claude-status.sh
#       vhs docs/assets/demo/agents.tape        # writes docs/assets/demo-agents.mp4
#
#   The stand-in is installed as a bare `claude` on PATH so both tabs read
#   "claude" (identical — the DOT is what tells them apart), in a short throwaway
#   dir so the status bar stays clean. A per-launch counter splits the stand-in:
#   instance 1 = the busy worker, instance 2 = the one that blocks (see the .sh).
#
#   DRAFT — record it and WATCH: tab 1 must still show a working ● when tab 2's
#   red ■ + border pulse land, and both must be clearly on-screen before the
#   acknowledging Enter. Tune the Sleep beats with fake-claude-status.sh together.
#   Needs `[notify] visual = true` (default).
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-agents.mp4

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

# ── Hidden setup: install the stand-in as a bare `claude`, reset the instance
#    counter, then launch spyc in a short throwaway project dir. ──────────────
Hide
Type `mkdir -p /tmp/spycbin && cp "$PWD/docs/assets/demo/fake-claude-status.sh" /tmp/spycbin/claude && chmod +x /tmp/spycbin/claude`
Enter
Type `rm -f /tmp/spyc-demo-agent-n`
Enter
Type `export PATH="/tmp/spycbin:$PATH" SPYC_PANE_CMD=claude`
Enter
Type `d=/tmp/spyc-demo && rm -rf "$d" && mkdir -p "$d/src" && cd "$d" && printf 'fn main() {}\n' > src/main.rs && printf 'mod app;\nmod git;\n' > src/lib.rs && printf '# demo\n' > README.md && printf '[package]\nname = "demo"\n' > Cargo.toml && git init -q && git add -A && git commit -qm init`
Enter
Type "spyc"
Enter
Sleep 4s
Show

# ── 1. Open agent tab 1 and set it working on a long task (dot pulses ●) ─────
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
Type "j"
Sleep 600ms
Type "audit the crate for unused imports"
Sleep 300ms
Enter
Sleep 2s

# ── 2. Open agent tab 2 and hand it a task — it will block for your input ────
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
Type "j"
Sleep 600ms
Type "rename Config to Settings across the crate"
Sleep 300ms
Enter
Sleep 500ms

# ── 3. Go back to your own work in the file list. Both tabs show a working ●. ─
Ctrl+A
Sleep 250ms
Type "k"
Sleep 400ms

# ── 4. Tab 2 hits a decision → its dot goes red ■ and the border rings the
#       visual bell, while you're in the list. THIS is "which agent needs me". ─
Sleep 4s

# ── 5. Switch to the pane (tab 2 is active), acknowledge with Enter → the latch
#       clears, tab 2 resumes and finishes; its dot settles to a done ■. ──────
Ctrl+A
Sleep 250ms
Type "j"
Sleep 800ms
Enter
Sleep 3.5s
