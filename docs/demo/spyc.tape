# spyc VHS demo tape
# Records: navigate file tree → ask agent → agent reads MCP context → gf jump
#
# Prerequisites (run once):
#   brew install vhs
#
# Run from the spyc repo root:
#   vhs docs/demo/spyc.tape
#
# Output: docs/demo.gif  (embed this in README.md)
#
# Iterate on Sleep values if transitions feel rushed or draggy — that's normal
# for a first pass. The story beats matter more than timing on the first run.

Output docs/demo.gif

Set Shell      "bash"
Set FontSize   13
Set Width      200
Set Height     50
Set Framerate  24
Set PlaybackSpeed 1.0
Set Theme      "Catppuccin Macchiato"

# ── Setup (hidden from recording) ────────────────────────────────────────────
# Launch spyc with the scripted fake-claude and wait for full load.
# The recording starts after spyc is already rendered.

Hide
Type "export SPYC_PANE_CMD=$(pwd)/docs/demo/fake-claude.sh && spyc"
Enter
Sleep 5s
Show

# ── 1. Navigate the file tree ─────────────────────────────────────────────────
# spyc opens at the repo root. Directories sort first: docs/, fuzz/, src/
# Two j presses land on src/

Type "j"
Sleep 0.25s
Type "j"
Sleep 0.4s

# Enter src/
Type "d"
Sleep 0.6s

# app/ is the second entry (after agent/) — one j press
Type "j"
Sleep 0.4s

# Enter src/app/
Type "d"
Sleep 0.6s

# Use / to search the listing — jumps cursor to mcp.rs in one motion
Type "/"
Sleep 0.3s
Type "mcp"
Sleep 0.6s
Enter
Sleep 0.5s

# ── 2. Open the agent pane ────────────────────────────────────────────────────

Ctrl+\
Sleep 2.8s

# ── 3. Ask the agent — it will read context without being told what file we're on

Type "what's the entry point for MCP tool dispatch in spyc?"
Sleep 0.3s
Enter

# Wait for fake-claude to print its response (sleep in fake-claude.sh + print time)
Sleep 5s

# ── 4. gf: jump to the file the agent mentioned ───────────────────────────────

# Switch focus from the pane back to the file list
Ctrl+A
Sleep 0.15s
Type "k"
Sleep 0.9s

# gf reads the first file:line reference from the pane output and jumps there
Type "gf"
Sleep 1.5s

# Hold the final frame so the viewer can read the result
Sleep 3s
