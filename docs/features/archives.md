# Archives

Part of spyc's [feature reference](../../FEATURES.md). Browsing into zip files and tarballs as if they were directories.

## Archives

`Enter` on a `.zip` / `.tar` / `.tar.gz` / `.tar.zst` (and the zip-derived
formats — `.jar`, `.whl`, `.epub`, `.apk`, …) walks *into* it, as if it were a
directory. `h` / `..` climbs back out to the archive file. The whole zip family
is browsed by reading its index, so entering even a multi-gigabyte archive
extracts **nothing** — a member's bytes are read only when you open one, and
they land in a session-scoped staging dir that is cleaned up on exit.

- **What the rows are** — the archive's own members, with their real sizes,
  mtimes and permission bits. Sorting, `=` filters, picks, search and the
  cursor all work as they do in a directory.
- **Reading a member** — `Enter` extracts just that member and opens it in the
  pager. `D` on the archive itself still shows the raw bytes, so the hex view
  is never more than one key away.
- **A compressed tar is different** — `.tar.gz` / `.tar.zst` can't be listed
  without decompressing the whole stream, so mounting one extracts as it reads.
  `[archive] extract_budget_mb` (default 512) is the ceiling; past it the mount
  is refused rather than filling the disk, and `:archive cancel` abandons one
  that is taking too long. An archive is read once however often it is asked
  for: `Enter` again, or from another column or project, while it is still
  being read waits for that read rather than starting another.
- **It says what's odd** — unsafe member paths (`../…`, or one claiming spyc's
  own `.spyc` staging directory), symlinks pointing out
  of the archive, duplicate or case-colliding names, encrypted members and a
  capped index are all reported when the mount opens, in full under
  `:archive info`.
- **Taking things out** — `y` yanks a member into the inventory (contents and
  all, so `p` outside the archive writes a real file), remembered by its path
  *inside* the archive — `pkg.zip/src/main.rs` — so the row reads as taken, a
  re-yank refreshes that entry, and the inventory never holds a cache path that
  outlives the session. It puts as its basename, like any other yank, `c` copies members out,
  `^a p` pipes one to the pane, and `f` / `L` report on them. Each extracts what
  it needs first, so nothing is unpacked speculatively.
- **Deleting a member** — `R` marks it for removal; the archive on disk is
  untouched until you write. Because a delete is an edit to the container's
  index, dropping a 500 MB member never extracts it.
- **Writing back** — `:archive write` replaces the archive: untouched members are
  carried across without being recompressed, the result is verified by reading it
  back, and only then renamed over the original. **A failed write can't damage
  the original** — nothing touches it until that rename. Archives up to
  `[archive] snapshot_max_mb` are also copied into the graveyard first, so
  `:undo` can bring the old one back. `:archive discard` throws the pending
  changes away instead; unmounting a changed archive offers to write it first,
  and quitting warns before dropping unwritten changes. While a write runs,
  another write, a delete, rename or put into that archive, a discard, an
  unmount and entering an archive inside it are refused until it finishes. An
  edit saved to an extracted copy meanwhile, by an editor or an agent, isn't
  in that write: it comes back as a pending change once the write lands. An
  archive with another one mounted inside it is written only once that one is
  unmounted.
- **Changing an archive** — `R` marks a member for removal, `p` puts an
  inventory item in, `c` copies files in, `M` renames a member (an index edit, so
  no bytes move however large it is), and `e` / `V` edit a member by extracting it
  and opening your editor on that copy. A file you bring in is a member from that
  moment — read it, edit it, rename it — and `R` on one takes it straight back out
  rather than recording a removal, since the archive never held it. Everything
  else stays pending until
  `:archive write`; the suffix carries a `+2 ~1 -3` badge meanwhile, and each
  changed member is marked in the listing's own status gutter — and the archive
  file itself is marked `~` in the directory it lives in, so a container holding
  unwritten changes is visible from outside it too. Inside, `~` is an edited
  member, `+` added, `>` renamed — the same glyphs git changes use, because it's
  the same question: what about this row isn't written yet. An edit made
  outside spyc — by your editor, or by an agent in the pane — counts as a change
  too, and shows in that badge: spyc compares each extracted file against what it
  wrote. From outside the archive, where the badge isn't visible, `:archive info`
  and `:archive list` name every mount carrying unwritten changes.
- **The archive is still a file** — where it lives, `pkg.zip` can be yanked,
  copied, renamed and deleted like anything else, mounted or not. Leaving one and
  pressing `Enter` again re-enters it without re-reading it, so pending changes
  survive the round trip. Deleting or moving the archive drops its mount with it —
  unless it carries unwritten changes, which would then have nowhere to go.
- **An archive inside an archive** — `Enter` on a member that is itself a
  container walks into it, browsed at its own member path (`outer.zip/bundle.zip`)
  while its bytes are read from the copy spyc extracted. Each level costs a full
  copy of that container in staging, since bytes have to be a real file before
  anything can read them, so `[archive] max_depth` (default 2) bounds it. Writing
  the inner one leaves the outer with a pending change of its own — it says so,
  and `:archive write` there puts it back.
- **Coming back to a place inside one** — a mark (`ma`), a harpoon slot (`Ha`)
  and the session you quit from can all point inside an archive. Jumping to one
  mounts the archive again on the way, so it doesn't matter that nothing has it
  open — `spyc -r` puts you back where you left off, several directories deep in
  a tarball. A second commander (`b`) that was inside one is the exception: the
  split comes back, but parked on the directory holding the container with the
  cursor on it, and says so.
- **Not yet supported inside an archive** — creating an empty directory or a new
  file, copying a whole directory *in* (only files, since the archive would keep
  the name and lose what's under it), moving across the archive boundary in one
  step, `:grep`, `F` and shell commands are refused with a message rather than
  half-working, and so are `gP` and `gS`: a project's home and start directory
  are real directories. The suffix shows `ro` when the archive can't be written
  back.
- **An agent can read a member too** — MCP `get_file_content` resolves a path
  inside a mount, reading the container in memory (or your not-yet-written edit,
  if there is one), so asking the agent in the pane about the file you're looking
  at works in here as well. `search_content` / `search_paths` say why they can't
  instead of reporting "no matches" from walking a single binary file. The
  worktree tools keep working too: they anchor on the project rather than on the
  container, so an agent can still set up and tear down worktrees while you read a
  tarball.
- **`:archive`** — `info` (this mount, or what's mounted when you're outside one),
  `list` (every mounted archive, dirty ones marked), `write` / `discard`,
  `unmount` (drop it and its staged bytes), `cancel`. `write` and `discard` work
  from outside a mount too: they take the archive under the cursor — the row
  showing `~` — else the only one with changes, and name them rather than guess
  when several do.
