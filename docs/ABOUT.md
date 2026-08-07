# spyc — about

Starting on SGI IRIX boxes in the 90s, I never really left the terminal except
as management duties required. My editor is vim and unlikely to change. What
has changed though is that agents now write the code, and the tooling around
that felt lacking to me: either a chat window with a file tree bolted on, or
me pasting paths into a prompt like a meat puppet.

Spyc is the IDE I wanted and now I could assemble it! The top pane is a file
commander in the Norton Commander tradition (and inspired by my VFX days
working with SideFX's Houdini): hjkl, marks, harpoon for a pinned working set,
a which-key popup when you stall on a chord. The bottom pane is a pty with tabs
where the agents live. The pager renders markdown and diff/show/blame are
built-in, so reviewing an agent's work never leaves the app. `e` drops you into
$EDITOR on the file under the cursor.

The part that makes it "agent-aware": spyc runs an MCP server (over a unix
socket), so the agent can ask what file my cursor is on, what I have picked,
and search the project, etc. "Look at this" stops being a path I paste and
becomes a tool call the agent makes. Agents also self-report status, so a pane
shows blocked or done and I stop needing to scan various terminals for
requests.

## Implementation

Rust to have a lightweight, portable core. I wanted the file commander to be
easily deployable to various production systems. To keep things simple there is
no async runtime. A single event loop, std threads for anything slow, results
come back over an mpsc channel with a generation counter so stale work gets
dropped.

Git is in-process via gitoxide — no libgit2, no openssl. Lua and libzstd are
the only compiled-C components; everything else is Rust.

Worktrees are supported to help agents work through parallel tasks without
stepping on each other. Removing a worktree archives your dirty files to a
graveyard dir instead of deleting them, and the branch only goes if it was
merged.

Most of this code was written by agents. I review everything (to a reasonable
extent — I am NOT a Rust native). Quality is enforced through several layers
of testing and controls. If you do see something smelly please do let me know!

## The fine print

BSD-3-Clause. No telemetry. No accounts. This is not my day job; it is the
tool I do my day job with. Releases are signed, and the signing chain is
documented in SECURITY.md.

Built for my own workflow and dogfooded daily, but happy to extend it for your
work too. Check the ROADMAP or file an Issue on github.

I hope spyc makes your coding a bit more enjoyable!

## The name

Rhymes with spicy. It's spy (a loving clone of spy from SideFX Software) plus
Claude: spy + c == spyc. Thus the fun spice session names and default colour
scheme.
