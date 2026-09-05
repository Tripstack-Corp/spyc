-- ===========================================================================
--  spyc · markdown demo — driven live in ONE reused iTerm window, and filmed
--  --------------------------------------------------------------------------
--  Why a real recording: spyc paints mermaid diagrams with a terminal graphics
--  protocol, and every scripted terminal recorder (vhs, tui-test) rasterizes
--  text cells only -- it renders the bitmap and the recorder throws it away.
--  So we drive the real app and film the real screen.
--
--  Why iTerm2 rather than System Events:
--    · exact geometry     -- `set columns to 200` / `set rows to 50`
--    · no focus stealing  -- keys go to the SESSION, so you can keep working
--    · readback           -- `get text` lets the script ASSERT, not just sleep
--  spyc supports iTerm2's native image protocol explicitly (SPYC-TRAP
--  iterm-osc1337), so diagrams paint correctly here.
--
--  ONE WINDOW, REUSED. The window id is remembered in a state file and the
--  session is tagged, so repeat runs land in the same window instead of
--  littering your desktop. Each run leaves it at a shell prompt.
--
--  Run:       osascript spyc-quicktime-demo.applescript
--  Rehearse:  osascript spyc-quicktime-demo.applescript dry    (no recording)
--
--  Paths resolve from this script's own folder; the fixture is staged to
--  /private/tmp/spyc-demo and takes are written there too, never into the repo.
--
--  macOS prompts once for Screen Recording (QuickTime Player, or this terminal
--  if you set recorder to "screencapture").
-- ===========================================================================

--  The fixture tree ships beside this script. It is COPIED to a disposable
--  staging dir per run, because a beat edits RELEASE-NOTES.md and the repo copy
--  must stay pristine -- a `git checkout --` against the repo copy would reach
--  into spyc's own working tree.
property stageDir    : "/private/tmp/spyc-demo"
property demoDir     : "/private/tmp/spyc-demo/aurora-docs"
--  Takes land OUTSIDE the repo on purpose: a .mov belongs in a release asset,
--  never in git history.
property outDir      : "/private/tmp/spyc-demo/out"
property colCount    : 200
property rowCount    : 50
--  `screencapture -v -R` films JUST the window rect, so nothing else on the
--  desktop is ever in the file and no crop pass is needed. The "quicktime"
--  path is kept only for older systems: on macOS 26 `new screen recording`
--  returns no document at all, so `start` fails with "variable is not defined".
property recorder    : "screencapture"   -- "screencapture" (region) | "quicktime" (dead on macOS 26)
property typeDelay   : 0.055
property tagName     : "SPYC-DEMO"
property paneCmd     : "vim -n RELEASE-NOTES.md"   -- -n = NO SWAP FILE. A stale
--  .swp from an interrupted run makes vim open on its E325 recovery prompt
--  instead of the file, which silently breaks every later beat.

global sess, win, isDry

-- ------------------------------------------------------------------ plumbing
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
	if d is not "" and (my sh("test -d " & quoted form of (d & "/aurora-docs") & " && echo y || echo n")) is "y" then
		return d
	end if
	set o to my sh("printf %s \"${SPYC_DEMO_HARNESS_DIR:-}\"")
	if o is not "" then return o
	error "cannot locate the harness: run this from its folder in the repo, or set SPYC_DEMO_HARNESS_DIR to the folder holding aurora-docs/"
end harnessDir

-- Fresh disposable copy of the fixture + a clean output dir. This is also the
-- RESET between runs: no git, no leftover .swp, no edited RELEASE-NOTES.md.
on stageFixture()
	set src to my harnessDir() & "/aurora-docs"
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
				error "QuickTime made no recording document -- it has no Screen Recording grant (System Settings > Privacy & Security > Screen & System Audio Recording). Use the screencapture recorder."
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

-- =========================================================================
on run argv
	set isDry to (argv contains "dry")
	set stamp to my sh("date +%Y%m%d-%H%M%S")
	set rawPath to outDir & "/qt-raw-" & stamp & ".mov"
	set outPath to outDir & "/spyc-mermaid-" & stamp & ".mp4"

	my trace("resetting the demo tree")
	my stageFixture()
	-- reap vim panes an interrupted run left behind. The pattern is our own
	-- launch wrapper, so this cannot touch an unrelated editor.
	my sh("pkill -f " & quoted form of ("exec " & paneCmd) & " 2>/dev/null; true")

	my acquireWindow()

	my trace("launching spyc at " & colCount & "x" & rowCount)
	tell application "iTerm2" to tell sess to ¬
		write text "cd " & quoted form of demoDir & " && clear && SPYC_PANE_CMD=" & quoted form of paneCmd & " spyc"
	my waitFor("CONTRIBUTING.md", 25)
	delay 2

	set pts to my screenPoints()
	tell application "iTerm2" to set wb to bounds of win
	my trace("window " & (item 1 of wb) & "," & (item 2 of wb) & " → " & (item 3 of wb) & "," & (item 4 of wb) & "   screen " & (item 1 of pts) & "x" & (item 2 of pts))

	set rectStr to "" & (item 1 of wb) & "," & (item 2 of wb) & "," & ¬
		((item 3 of wb) - (item 1 of wb)) & "," & ((item 4 of wb) - (item 2 of wb))
	my trace("recording (" & recorder & ") rect " & rectStr)
	my startRec(rawPath, rectStr)
	delay 1.5

	try
		------------------------------------------------------------- beat 1
		my trace("beat 1 — browse the docs tree")
		repeat 3 times
			my emit("j")
			delay 0.42
		end repeat
		delay 1
		my emit("G")
		delay 1.5

		------------------------------------------------------------- beat 2
		my trace("beat 2 — full-height rendered markdown preview")
		my ctrlKey("s")
		delay 0.25
		my emit("|")
		delay 1.5
		-- spyc RESTORES the scroll position, so a re-run opens the preview where
		-- the last run left it -- at the bottom, if that run ended on the mermaid
		-- fence. Take the top BEFORE asserting, or the anchor is off-screen above
		-- and the beat fails on a preview that is working perfectly.
		my chordA("b")
		my emit("g")
		delay 0.12
		my emit("g")
		my waitFor("compression codec", 15)
		delay 3
		repeat 4 times
			my ctrlKey("d")
			delay 1.1
		end repeat
		delay 1
		my emit("g")
		delay 0.12
		my emit("g")
		delay 1.5

		------------------------------------------------------------- beat 3
		my trace("beat 3 — edit in vim, preview re-renders on save")
		my chordA("a")
		my chordA("c")
		delay 1.4
		my enter()          -- accept the prefilled command ($SPYC_PANE_CMD)
		delay 1.2
		my enter()          -- accept the prefilled cwd
		my waitFor("1485B", 20)
		delay 1.5
		my emit("G")
		delay 0.9
		my emit("o")
		delay 0.7
		my enter()
		my typeSlow("## Rollback")
		my enter()
		my enter()
		my typeSlow("> [!CAUTION]")
		my enter()
		my typeSlow("> Roll back with `aurora rollback --to 2.9`.")
		delay 0.5
		my escKey()
		delay 0.9
		my typeSlow(":w")
		my enter()
		my waitFor("written", 20)
		delay 1.5

		my chordA("k")      -- pane -> list. ^a a / ^a b only move between COLUMNS
		delay 0.8
		my chordA("b")      -- the preview column
		my emit("G")
		my waitForRight("CAUTION", 20)
		delay 4

		-- Quitting vim does NOT remove the tab: spyc keeps it and shows
		-- "pane exited — ^a-R to restart, ^a-x to close". So close the tab
		-- explicitly, and use the ^a | ALIAS for the split, because plain ^s
		-- never reaches spyc while a pane holds focus.
		my chordA("j")
		delay 0.8
		my typeSlow(":q")
		my enter()
		my waitFor("[exited", 15)   -- the DIVIDER, not the flash: "pane exited" fades
		my chordA("x")      -- close the exited tab (no confirm once the child is gone)
		delay 2
		my chordA("|")      -- close the vsplit
		delay 2

		------------------------------------------------------------- beat 4
		my trace("beat 4 — outline folding")
		my emit("/HAND")
		delay 0.9
		my enter()
		delay 0.7
		my escKey()
		delay 0.6
		my enter()
		delay 1.2
		my emit("g")
		delay 0.12
		my emit("g")
		my waitFor("audience: operators", 15)
		delay 1.5
		my chordZ("M")
		my waitForCount("▸", 10, 12)
		delay 4.5
		repeat 3 times
			my emit("]]")
			delay 1
		end repeat
		my chordZ("R")
		my waitForCount("▸", 0, 12)
		delay 2

		--------------------------------------------- beat 5 — THE MERMAID BEAT
		my trace("beat 5 — mermaid diagram rendered in the terminal")
		-- The only fence in the fixture is in RELEASE-NOTES.md, and beat 4 left
		-- the pager on HANDBOOK.md -- so leave it and open the right file, or
		-- `i` has nothing to render and the beat films a plain page of text.
		my escKey()
		delay 1.2
		my emit("/RELEASE")
		delay 0.9
		my enter()
		delay 0.7
		my escKey()
		delay 0.6
		my enter()
		delay 1.2
		my emit("g")
		delay 0.12
		my emit("g")
		my waitFor("streaming ingest layer", 15)
		delay 1.2
		my emit("/mermaid")
		delay 1
		my enter()
		delay 1.8
		my emit("i")
		-- `mermaid diagram` alone is NOT evidence: it is also the rendered
		-- placeholder block, and it is inside the refusal "no mermaid diagram
		-- in view". `c theme` is on the image overlay and only for a mermaid
		-- origin, so it cannot match anything but a diagram actually on screen.
		my waitFor("c theme", 25)
		my trace("      diagram is on screen — the frame no scripted recorder can capture")
		delay 5
		my emit("c")
		delay 4
		my escKey()
		delay 1.5
		my escKey()
		delay 2
	on error e
		my trace("BEAT FAILED: " & e)
	end try

	my trace("stopping recording")
	my stopRec(rawPath)

	-- always leave the window at a shell prompt, ready to be reused
	try
		my escKey()
		delay 0.5
		my ctrlKey("d")
		delay 1.5
	end try
	my stageFixture()

	if not isDry then
		if recorder is "screencapture" then
			-- already exactly the window rect: transcode only, never crop twice
			set cmd to "set -e" & ¬
				"; RAW=" & quoted form of rawPath & ¬
				"; OUT=" & quoted form of outPath & ¬
				"; test -s \"$RAW\" || { echo 'no take was written'; exit 1; }" & ¬
				"; ffmpeg -v error -i \"$RAW\" -an -c:v libx264 -crf 18 -pix_fmt yuv420p -movflags +faststart \"$OUT\" -y" & ¬
				"; echo \"$(ffprobe -v error -select_streams v -show_entries stream=width,height -of csv=p=0 \"$OUT\") -> $OUT\""
		else
			set cmd to "set -e" & ¬
				"; RAW=" & quoted form of rawPath & ¬
				"; OUT=" & quoted form of outPath & ¬
				"; VW=$(ffprobe -v error -select_streams v -show_entries stream=width -of csv=p=0 \"$RAW\")" & ¬
				"; SC=$(python3 -c \"print(round($VW/" & (item 1 of pts) & ",4))\")" & ¬
				"; X=$(python3 -c \"print(int(" & (item 1 of wb) & "*$SC)//2*2)\")" & ¬
				"; Y=$(python3 -c \"print(int(" & (item 2 of wb) & "*$SC)//2*2)\")" & ¬
				"; W=$(python3 -c \"print(int((" & (item 3 of wb) & "-" & (item 1 of wb) & ")*$SC)//2*2)\")" & ¬
				"; H=$(python3 -c \"print(int((" & (item 4 of wb) & "-" & (item 2 of wb) & ")*$SC)//2*2)\")" & ¬
				"; echo \"video=${VW}px scale=$SC crop=${W}x${H}+${X}+${Y}\"" & ¬
				"; ffmpeg -v error -i \"$RAW\" -vf \"crop=$W:$H:$X:$Y\" -an -c:v libx264 -crf 18 -pix_fmt yuv420p -movflags +faststart \"$OUT\" -y" & ¬
				"; echo \"$OUT\""
		end if
		try
			my trace(my sh(cmd))
		on error e2
			my trace("post-processing failed: " & e2)
			my trace("raw take kept at " & rawPath)
		end try
	end if

	my trace("done — window left open for the next run")
end run
