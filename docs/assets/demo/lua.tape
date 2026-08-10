# ─────────────────────────────────────────────────────────────────────────────
# spyc tour — "script it": a Lua script bound to a key, and what it does.
#
#   Story:  read ~/.config/spyc/lua/todos.lua in the pager (it calls spyc's own
#           gitignore-aware content search, then picks what it found) → `:cd`
#           into the project → press the key it's bound to → every file with a
#           TODO is picked, and the flash says how many.
#
#   Generate (from the repo root):
#       make demos            # or: vhs docs/assets/demo/lua.tape
#
#   Notes:
#     • One session, no cut: `:cd` moves from the script's dir to the project,
#       so the viewer sees the same spyc do both halves.
#     • The binding (`map T lua todos`) lives in the fixture's $HOME rc —
#       `lua` is an executing verb, so a project-local rc cannot bind it.
# ─────────────────────────────────────────────────────────────────────────────

Output docs/assets/demo-lua.gif

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

# ── 1. The script itself, where spyc looks for it ────────────────────────────
#      Launching in the repo pins PROJECT_HOME to it, so the project chip stays
#      `tour` while we step outside — and `Space p` is the ride home.
Sleep 1.5s
Type ":cd ~/.config/spyc/lua"
Sleep 400ms
Enter
Sleep 1.5s
Enter
Sleep 5s
Type "q"
Sleep 800ms

# ── 2. Back to the project, into the source it will act on ───────────────────
Space
Sleep 400ms
Type "p"
Sleep 1.4s
Type "/"
Sleep 250ms
Type "src"
Sleep 350ms
Enter
Sleep 400ms
Enter
Sleep 1.6s

# ── 3. `T` runs it: the TODO files light up as picks ─────────────────────────
Type "T"
Sleep 3.5s

# ── 4. `=!` narrows the listing to exactly what the script picked ────────────
Type "=!"
Sleep 400ms
Enter
Sleep 3s
