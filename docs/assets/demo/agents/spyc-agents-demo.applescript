-- ===========================================================================
--  spyc · agent orchestration demo — "Four agents. One glance."
--  --------------------------------------------------------------------------
--  The clip answers one question: WHICH AGENT NEEDS ME? It opens on four panes
--  already in four different states and lets the dots carry the point.
--
--    [1] heat-pulse ●  a Haiku agent mid-task
--    [2] steady red ■  a Haiku agent waiting on you        <- the point
--    [3] calm teal  ■  a Haiku agent that finished its turn
--    [4] 💤            rmatrix, ^z-suspended
--
--  The agents are REAL (`claude --model haiku`), so their prose is
--  non-deterministic. Every assertion therefore reads spyc's own state rather
--  than agent output -- see the two traps below.
--
--  Run:       osascript spyc-agents-demo.applescript
--  Rehearse:  osascript spyc-agents-demo.applescript dry    (no recording)
--  Arrange only, then stop (to eyeball the dots):
--             osascript spyc-agents-demo.applescript setup
--
--  TRAP 1 -- `blocked` and `done` are THE SAME GLYPH. Both render `■`
--  (U+25A0); only the colour differs (hot red vs teal) and `get text` carries
--  no colour. Counting `■` cannot tell "needs me" from "finished", so the
--  arrangement is verified through `:activity dump`, which names each pane's
--  state in words. Asserting on the glyph would repeat the mermaid-beat bug.
--
--  TRAP 2 -- only `working` is unstable. `blocked` is latched until you settle
--  it, `done` and `idle` persist, `💤` is a sticky toggle -- but `●` lasts only
--  while a turn is actually running. Tab 1 therefore gets a long read-only task
--  kicked off during setup, and the clip has to be shot while it is still going.
-- ===========================================================================

property stageDir    : "/private/tmp/spyc-demo"
property demoDir     : "/private/tmp/spyc-demo/aurora-docs"
property outDir      : "/private/tmp/spyc-demo/out"
property colCount    : 200
property rowCount    : 50
property recorder    : "screencapture"
property typeDelay   : 0.042
property tagName     : "SPYC-AGENTS"
--  Read-only work: Read is not gated by default, so this pane stays `working`
--  instead of stopping on a permission prompt like tab 2 deliberately does.
property workPrompt  : "Read every markdown file under this directory tree, one at a time, and then describe how the docs are organized."
--  Needs approval -> the hook reports `blocked` -> the dot latches red.
property blockPrompt : "Run the shell command: date"
--  Finishes in one turn -> `done`.
property donePrompt  : "Reply with just the word: ready"
--  After the block is answered, this drives spyc's own MCP tool so the file
--  list moves under the user.
property mcpPrompt    : "Use the navigate_to tool to open the guides directory."
property paneCmd     : "claude --model haiku"
--  The fixture is the markdown harness's tree, borrowed rather than copied.
--  One path, used by both the locate-check and the staging copy -- deriving it
--  twice is how the check passed while the copy looked somewhere else.
property fixtureRel  : "../markdown/aurora-docs"

global sess, win, isDry, isSetupOnly

on sh(c)
	return do shell script c
end sh

-- Where this script (and its aurora-docs/ fixture) lives. `path to me` is the
-- script itself under `osascript file.applescript`, but resolves to the
-- osascript binary under some hosts -- so verify the fixture is really there
-- and fall back to an override rather than staging an empty tree.
on harnessDir()
	try
		set d to my sh("dirname " & quoted form of (POSIX path of (path to me)))
	on error
		set d to ""
	end try
	if d is not "" and (my sh("test -d " & quoted form of (d & "/" & fixtureRel) & " && echo y || echo n")) is "y" then
		return d
	end if
	set o to my sh("printf %s \"${SPYC_DEMO_HARNESS_DIR:-}\"")
	if o is not "" then return o
	error "cannot locate the harness: run this from its folder in the repo, or set SPYC_DEMO_HARNESS_DIR to the folder holding " & fixtureRel
end harnessDir

-- Fresh disposable copy of the fixture + a clean output dir. This is also the
-- RESET between runs: no git, no leftover .swp, no edited RELEASE-NOTES.md.
on stageFixture()
	set src to my harnessDir() & "/" & fixtureRel
	my sh("rm -rf " & quoted form of demoDir & " && mkdir -p " & quoted form of demoDir & " " & quoted form of outDir & ¬
		" && cp -R " & quoted form of (src & "/.") & " " & quoted form of demoDir)
end stageFixture

on trace(m)
	log "  ‣ " & m
end trace

on stateFile()
	return outDir & "/.demo-window-id"
end stateFile

-- one key, no newline: a KEYPRESS, not a command
on emit(s)
	tell application "iTerm2" to tell sess to write text s newline no
end emit

on typeSlow(s)
	repeat with i from 1 to length of s
		my emit(character i of s)
		delay typeDelay
	end repeat
end typeSlow

on ctrlKey(c)
	my emit(ASCII character ((ASCII number c) - 96))   -- ^a=1 ^d=4 ^s=19
end ctrlKey

on enter()
	my emit(ASCII character 13)
end enter

on escKey()
	my emit(ASCII character 27)
end escKey

on chordA(c)
	my ctrlKey("a")
	delay 0.14
	my emit(c)
	delay 1.0
end chordA

on chordZ(c)
	my emit("z")
	delay 0.32
	my emit(c)
	delay 1.0
end chordZ

-- iTerm2 hands back the WHOLE SCROLLBACK (617 lines for a 50-row window), and
-- `contents` is no different. Matching against that is how an assertion passes
-- on stale output from earlier in the session, and how a `▸` count ends up
-- tallying folds from three beats ago. The visible screen is the LAST rowCount
-- lines, so slice it.
on screenText()
	tell application "iTerm2" to set t to (get text of sess)
	set ps to paragraphs of t
	set n to count of ps
	set i0 to n - rowCount + 1
	if i0 < 1 then set i0 to 1
	set acc to ""
	repeat with i from i0 to n
		set acc to acc & (item i of ps) & linefeed
	end repeat
	return acc
end screenText

-- right half only. The payoff assertion MUST be scoped: vim shows the same text
-- bottom-left, so a whole-screen match would prove nothing about the preview.
on rightText()
	set cut to (colCount / 2) as integer
	set acc to ""
	repeat with para in (paragraphs of (my screenText()))
		set l to (para as text)
		if (length of l) > cut then set acc to acc & (text (cut + 1) thru -1 of l) & linefeed
	end repeat
	return acc
end rightText

on dumpScreen(tag)
	try
		set f to outDir & "/dump-" & (my sh("date +%H%M%S")) & ".txt"
		my sh("cat > " & quoted form of f & " <<'SPYCEOF'" & linefeed & (my screenText()) & linefeed & "SPYCEOF")
		my trace("  ✗ " & tag & " — screen dumped to " & f)
	end try
end dumpScreen

-- poll the real screen instead of guessing with delay
on waitFor(pat, secs)
	repeat with i from 1 to (secs * 4)
		try
			if (my screenText()) contains pat then
				my trace("  ✓ " & pat)
				return true
			end if
		end try
		delay 0.25
	end repeat
	my dumpScreen("waitFor " & pat)
	error "timed out waiting for: " & pat
end waitFor

on waitForRight(pat, secs)
	repeat with i from 1 to (secs * 4)
		try
			if (my rightText()) contains pat then
				my trace("  ✓ right column: " & pat)
				return true
			end if
		end try
		delay 0.25
	end repeat
	my dumpScreen("waitForRight " & pat)
	error "timed out waiting for " & pat & " in the RIGHT column"
end waitForRight

on countOf(needle)
	set t to my screenText()
	set tid to AppleScript's text item delimiters
	set AppleScript's text item delimiters to needle
	set n to (count of text items of t) - 1
	set AppleScript's text item delimiters to tid
	return n
end countOf

on waitForCount(needle, want, secs)
	repeat with i from 1 to (secs * 4)
		if (my countOf(needle)) is want then
			my trace("  ✓ " & want & " × " & needle)
			return true
		end if
		delay 0.25
	end repeat
	my dumpScreen("waitForCount " & needle)
	error "wanted " & want & " × " & needle & ", saw " & (my countOf(needle))
end waitForCount

-- ------------------------------------------------- one window, found or made
on acquireWindow()
	set w to missing value

	-- 1. the window we used last time
	try
		set wid to (my sh("cat " & quoted form of (my stateFile()))) as integer
		tell application "iTerm2" to set w to (first window whose id is wid)
		my trace("reusing window " & wid)
	end try

	-- 2. any window still carrying our tag
	if w is missing value then
		tell application "iTerm2"
			repeat with ww in windows
				try
					if (name of current session of ww) contains tagName then
						set w to ww
						my trace("reusing tagged window")
						exit repeat
					end if
				end try
			end repeat
		end tell
	end if

	-- 3. make one
	if w is missing value then
		tell application "iTerm2" to set w to (create window with default profile)
		my trace("created a new window")
	end if

	set win to w
	-- A fresh TAB in the reused window. Trying to quit the previous spyc with
	-- ^d is unreliable (it needs ^d^d, and a half-quit instance then eats the
	-- launch command as keystrokes); a new tab is unconditional. New tab first,
	-- so the window is never left with zero tabs and cannot disappear.
	tell application "iTerm2"
		set nOld to count of tabs of w
		tell w to create tab with default profile
		delay 1
		repeat nOld times
			try
				tell tab 1 of w to close
				delay 0.5
			end try
		end repeat
		delay 0.5
		set sess to current session of w
		tell sess
			set name to tagName
			set columns to colCount
			set rows to rowCount
		end tell
	end tell
	my sh("echo " & (id of w) & " > " & quoted form of (my stateFile()))
	return w
end acquireWindow

-- leave whatever ran last time; get back to a shell prompt WITHOUT sending a
-- bare ^d at a shell (that would exit the shell and close the tab)
on ensureShell()
	repeat with i from 1 to 6
		set t to ""
		try
			set t to my screenText()
		end try
		if t contains "picks:" then          -- spyc's status bar => spyc is up
			my trace("quitting the previous spyc")
			my ctrlKey("d")
			delay 1.5
		else if t contains "E325" or t contains "swap file" then
			my emit("q")                     -- dismiss a vim recovery prompt
			delay 1
		else
			return true
		end if
	end repeat
	return true
end ensureShell

-- ----------------------------------------------------------------- recording
on screenPoints()
	-- `zoomed` maximizes to the visible frame, so reading the bounds back gives
	-- the screen size IN POINTS with no extra dependency. A scaled display is
	-- NOT 2x -- this machine is 3456px over 2056pt = 1.68 -- so this matters.
	tell application "iTerm2"
		set old to bounds of win
		set zoomed of win to true
		delay 0.6
		set b to bounds of win
		set zoomed of win to false
		delay 0.4
		set bounds of win to old
		delay 0.3
	end tell
	return {item 3 of b, item 4 of b}
end screenPoints

-- rect_ is "x,y,w,h" in POINTS -- screencapture captures it at native scale.
on startRec(path_, rect_)
	if isDry then
		my trace("[dry] recording skipped")
		return
	end if
	if recorder is "quicktime" then
		tell application "QuickTime Player"
			activate
			-- `new screen recording` declares NO <result> in QuickTime's sdef,
			-- while `new audio recording` and `new movie recording` both return
			-- a document -- so `set x to new screen recording` can never bind,
			-- and reports the useless "the variable x is not defined". Take the
			-- document off the app instead.
			new screen recording
			delay 1
			if (count of documents) is 0 then
				error "QuickTime made no recording document. It reports \"encountered an error while recording your screen -- try using the Screenshot app instead\", which is what the screencapture recorder already is. Use recorder \"screencapture\"."
			end if
			start document 1
		end tell
		delay 3
		tell application "iTerm2" to activate
		delay 1
	else
		my sh("nohup screencapture -v -R " & rect_ & " " & quoted form of path_ & " >/dev/null 2>&1 &")
		delay 2.5
		if (my sh("pgrep -x screencapture >/dev/null && echo y || echo n")) is not "y" then
			error "screencapture did not start -- grant Screen Recording to your terminal in System Settings > Privacy & Security"
		end if
	end if
end startRec

on stopRec(path_)
	if isDry then return
	if recorder is "quicktime" then
		tell application "QuickTime Player"
			stop document 1
			delay 2
			save document 1 in file (POSIX file path_)
			delay 3
			close document 1 saving no
			quit saving no
		end tell
	else
		my sh("pkill -INT -x screencapture || true")
		delay 3
	end if
end stopRec


-- ------------------------------------------------------- pane orchestration
on newTab(cmd)
	my chordA("c")
	delay 1.0
	-- `^a c` PREFILLS the command box with the default (`claude`). Typing on
	-- top of it yields `claudeclaude` and a pane that exits 127. `^u` clears
	-- the prompt buffer; the cwd prompt that follows is prefilled correctly,
	-- so that one just takes an Enter.
	my ctrlKey("u")
	delay 0.35
	my typeSlow(cmd)
	my enter()          -- the command
	delay 1.0
	my enter()          -- the cwd, prefilled
	delay 2.5
	my acceptHookConsent()
end newTab

-- The first agent pane per project raises a MODAL `[Y/n]` popup asking to write
-- status hooks into the project config. Only `y`/`n` dismisses it -- Esc and
-- every other key are swallowed and it stays up. Miss it and the whole
-- arrangement types itself into a prompt that never closes: no hooks get
-- installed, so no agent can self-report, and every dot falls back to output
-- timing. Answering it is what makes `working`/`blocked`/`done` mean anything.
-- Consent is remembered per project root, so this is a no-op after the first run.
on acceptHookConsent()
	repeat with i from 1 to 24
		try
			if (my screenText()) contains "agent status" then
				my emit("y")
				delay 1.4
				my trace("  ✓ accepted the status-hooks consent")
				return true
			end if
		end try
		delay 0.5
	end repeat
	return false          -- already consented for this root: nothing to answer
end acceptHookConsent

on focusTab(n)
	my chordA(n as text)
	delay 1.0
end focusTab

on zoomToggle()
	my chordA("z")
	delay 1.2
end zoomToggle

-- ^z is NOT an ^a chord: it goes to the pane directly.
on suspendToggle()
	my ctrlKey("z")
	delay 1.5
end suspendToggle

-- Type a prompt into the focused agent pane and submit it.
on promptAgent(p)
	my typeSlow(p)
	delay 0.4
	my enter()
end promptAgent

-- `:activity dump` names every pane's state in WORDS, which is the only way to
-- tell a red ■ from a teal one. Opens a pager; read it, then close it.
on dumpSays(wanted, secs)
	repeat with i from 1 to secs
		my chordA("k")       -- `:` reaches spyc only from LIST focus
		delay 0.5
		my emit(":")
		delay 0.4
		my typeSlow("activity dump")
		my enter()
		delay 1.6
		set t to my screenText()
		set ok to true
		repeat with w in wanted
			if t does not contain (w as text) then set ok to false
		end repeat
		my escKey()          -- close the pager
		delay 0.6
		if ok then
			my trace("  ✓ activity dump has: " & (wanted as text))
			return true
		end if
		delay 2
	end repeat
	my dumpScreen("activity-dump")
	error "activity dump never showed all of: " & (wanted as text)
end dumpSays

-- =========================================================================
on run argv
	set isDry to (argv contains "dry")
	set isSetupOnly to (argv contains "setup")
	set stamp to my sh("date +%Y%m%d-%H%M%S")
	set rawPath to outDir & "/qt-raw-" & stamp & ".mov"
	set outPath to outDir & "/spyc-agents-" & stamp & ".mp4"

	my trace("staging the demo tree")
	my stageFixture()
	my sh("pkill -x rmatrix 2>/dev/null; true")

	my acquireWindow()

	my trace("launching spyc at " & colCount & "x" & rowCount)
	tell application "iTerm2" to tell sess to ¬
		write text "cd " & quoted form of demoDir & " && clear && spyc"
	my waitFor("CONTRIBUTING.md", 25)
	delay 2

	--------------------------------------------------------------- ARRANGE
	-- Not filmed. Four panes are driven into four states, and the whole point
	-- of the clip is that they hold those states at once -- so verify before
	-- spending a take.
	my trace("arranging four panes")

	my trace("  [1] a long read-only task -> working")
	my newTab(paneCmd)
	my promptAgent(workPrompt)
	delay 3

	my trace("  [2] a command needing approval -> blocked")
	my newTab(paneCmd)
	my promptAgent(blockPrompt)
	delay 3

	my trace("  [3] a one-turn reply -> done")
	my newTab(paneCmd)
	my promptAgent(donePrompt)
	delay 3

	my trace("  [4] rmatrix -- a pane that is not an agent at all")
	my newTab("rmatrix")
	delay 2

	my trace("verifying the arrangement through :activity dump")
	my focusTab(1)
	my dumpSays({"working", "blocked", "done"}, 12)

	if isSetupOnly then
		my trace("setup only -- four panes are arranged, nothing recorded")
		return
	end if

	set pts to my screenPoints()
	tell application "iTerm2" to set wb to bounds of win
	set rectStr to "" & (item 1 of wb) & "," & (item 2 of wb) & "," & ¬
		((item 3 of wb) - (item 1 of wb)) & "," & ((item 4 of wb) - (item 2 of wb))
	my trace("recording (" & recorder & ") rect " & rectStr)
	my startRec(rawPath, rectStr)
	delay 1.5

	try
		------------------------------------------------------------- beat 1
		-- Cold open on the rain, zoomed. Suspending WHILE zoomed freezes it on
		-- camera, and the unzoom then reveals the frame with 💤 already set --
		-- so the glance that follows is complete from its first frame.
		my trace("beat 1 — cold open: rmatrix, zoomed")
		my focusTab(4)
		my zoomToggle()
		delay 4
		my suspendToggle()
		delay 1.2
		my zoomToggle()
		delay 1.5

		------------------------------------------------------------- beat 2
		my trace("beat 2 — the glance: four tabs, four states")
		my waitFor("💤", 10)
		-- a little movement so the frame is alive while the dots do the talking
		my chordA("k")
		delay 1.2
		my emit("j")
		delay 0.7
		my emit("j")
		delay 0.7
		my emit("k")
		delay 3.5

		------------------------------------------------------------- beat 3
		-- Deliberately NOT zoomed: the dot lives in the divider, so zooming the
		-- pane hides the very thing the beat is about. And no `:activity dump`
		-- here -- it is the right instrument for setup, but it throws a pager
		-- over the frame, and this is the shot.
		my trace("beat 3 — answer the one that needs me")
		my focusTab(2)
		delay 3.5
		my enter()                       -- approve the permission prompt
		delay 5

		------------------------------------------------------------- beat 4
		my trace("beat 4 — the agent moves the view")
		my promptAgent(mcpPrompt)
		delay 1.2
		-- This is the assertion that proves the whole chain: the prompt was
		-- approved, the agent resumed, took a new instruction, and called
		-- spyc's own tool. Agent prose is non-deterministic; the file list
		-- moving is not.
		my waitFor("quickstart.md", 40)  -- navigate_to landed: the list moved
		my trace("      the file list moved under the user, driven over MCP")
		delay 4

		------------------------------------------------------------- beat 5
		my trace("beat 5 — wake the rain, close")
		my focusTab(4)
		my suspendToggle()
		delay 4
	on error e
		my trace("BEAT FAILED: " & e)
	end try

	my trace("stopping recording")
	my stopRec(rawPath)

	if not isDry then
		set cmd to "set -e" & ¬
			"; RAW=" & quoted form of rawPath & ¬
			"; OUT=" & quoted form of outPath & ¬
			"; test -s \"$RAW\" || { echo 'no take was written'; exit 1; }" & ¬
			"; ffmpeg -v error -i \"$RAW\" -an -c:v libx264 -crf 18 -pix_fmt yuv420p -movflags +faststart \"$OUT\" -y" & ¬
			"; echo \"$(ffprobe -v error -select_streams v -show_entries stream=width,height -of csv=p=0 \"$OUT\") -> $OUT\""
		try
			my trace(my sh(cmd))
		on error e2
			my trace("post-processing failed: " & e2)
			my trace("raw take kept at " & rawPath)
		end try
	end if

	my trace("done — the panes are left running for another take")
end run
