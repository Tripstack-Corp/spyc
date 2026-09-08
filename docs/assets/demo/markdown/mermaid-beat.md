# The mermaid beat — must be a REAL screen capture

No scripted recorder can capture it (both vhs and tui-test rasterize text cells
only; spyc emits the bitmap and the recorder drops it). Do this take live.

1. Stage the fixture (the AppleScript harness does this for you; by hand it is
   a copy), open a REAL Ghostty window, size it ~200x50, and enter the tree:
       rm -rf /private/tmp/spyc-demo/aurora-docs
       mkdir -p /private/tmp/spyc-demo/aurora-docs
       cp -R aurora-docs/. /private/tmp/spyc-demo/aurora-docs
       cd /private/tmp/spyc-demo/aurora-docs && spyc

2. Start capture in another terminal (window-only, retina, 30fps).
   List devices first, then pick the screen index:
       ffmpeg -f avfoundation -list_devices true -i "" 2>&1 | grep -i screen
       ffmpeg -f avfoundation -framerate 30 -i "<screen-index>:none" \
              -c:v libx264 -preset ultrafast -crf 14 -pix_fmt yuv420p mermaid-raw.mov
   (Needs Screen Recording permission for your terminal in System Settings →
   Privacy & Security. QuickTime "New Screen Recording" works just as well.)

3. Keystrokes, slowly:
       /HAND  <Enter>  <Esc>      select HANDBOOK.md   (or RELEASE-NOTES.md)
       <Enter>                    open the in-app pager
       /mermaid <Enter>           jump to the fence
       i                          render the diagram inline, full screen
       c                          light/dark toggle  <- nice beat
       q                          dismiss

4. Trim and mute:
       ffmpeg -i mermaid-raw.mov -ss <start> -to <end> -an \
              -c:v libx264 -crf 18 -pix_fmt yuv420p mermaid-beat.mp4

Splice it after beat 4 of spyc-markdown-demo.mp4. Match the font size so the cut
is invisible: the scripted clips render at MesloLGL Nerd Font Mono, 15px.
