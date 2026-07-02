# spyc — Brand & Identity

> The file commander is the noun the agent operates on.

This document defines how spyc looks, sounds, and is named. It's for
contributors and for anyone writing copy or making assets. Audience for the
product itself: people who live in the terminal. Write for them.

## What spyc is

A keyboard-driven, MCP-native terminal file commander. Two panes — a
vi-flavoured file commander on top, your coding agent below (Claude Code by
default, any program in practice) — sharing a local MCP socket so the agent
sees exactly what you're looking at: cursor, picks, inventory, branch. No
copy-paste, no path description. macOS and Linux. Rust.

That's the product in three sentences. Everything below is how it should look,
sound, and be named.

## Origin

```
~1999   SGI IRIX workstation, a purple beast on top of the desk.
        Learning 3D. The software shipped a keyboard-driven file
        commander in the terminal — hjkl, tag, yank, go. Fastest
        I'd ever moved through a filesystem.
~2001   The SGI went away. Nothing matched it.
2024    Back in the terminal full time. Coding agent in the next
        pane — good, but boxed in a chat window, fed paths by hand,
        blind to what I was looking at.
2025    Rebuilt the file commander I missed, wired to the agent.
        Rust, modal, MCP socket. The agent reads the socket and
        sees what I see.
                                                         └─ spyc
```

I learned 3D on an SGI IRIX workstation — a purple beast that sat on top of the
desk, not under it. The software shipped a small file commander that ran in the
terminal: keyboard only. `hjkl` to move, a key to tag, a key to yank, a key to
go. It was the fastest I'd ever moved through a filesystem. Then the SGI went
away, and for twenty years nothing matched it.

I'm back in the terminal now, with a coding agent in the next pane. The agent is
good. It also lives in a chat window, and I spend half my time telling it what
it's already looking at — pasting paths, describing the tree, losing track of
what it can see. The control I had on that purple machine and the agent I rely
on now were in two different worlds.

spyc puts them in one. The file commander I missed, rebuilt in Rust, modal like
neovim, with a socket the agent reads so it sees what I see.

## The name

**spyc** — say it *spy-see*. Four characters that carry the whole story: **spy**
+ **c**.

**spy** is the lineage: the keyboard-driven file commander that shipped inside
the VFX-pipeline software on that SGI IRIX workstation, the one that set the
bar. **c** is the coding agent spyc is built to run beside (Claude Code by
default) — the reason the MCP socket exists at all. Lineage and agent, in four
keystrokes.

Said aloud, *spy-see* is a near-homonym for *spicy* — and that pun is why the
whole brand runs on chili and spice: the 🌶️ logo, the spice-pair session names,
the warm-heat palette. The name earns the identity rather than borrowing it.

Spice is also the right metaphor for the tool. It's specific — not "flavor" in
the abstract, but bright, sharp, and unmistakable, where a small amount does a
lot of work. That's spyc: precise, fast, no bulk.

## Lineage

spyc belongs to a long line of keyboard-driven terminal file managers — the
orthodox commanders, the vi-motion browsers, and the small, fast file tools that
once shipped inside VFX pipeline software, including the one that started all of
this for me. From that tradition it inherits the grammar: modal navigation,
tagging and picks, yank-to-buffer inventories, terminal execution from the
cursor.

It inherits ideas, not code. spyc is written from scratch in Rust, with a
modern TUI and a local MCP transport that none of its ancestors had. The
relationship is the one `ripgrep` has to `grep` or `bat` has to `cat`: a beloved
workflow, rebuilt for a new era. The keyboard grammar is a tribute. The engine
is new.

## Personality & voice

Confident, dry, economical. spyc talks to people who already know their way
around a terminal and respects that they're busy. It doesn't hype, it doesn't
hand-hold, and it doesn't explain what a file manager is.

The test for any sentence: would it survive being typed at a prompt? Write docs
the way you type commands — say the thing, then stop.

**Do**
- Lead with what it does, in motion: *pick three files, ask, the agent sees them.*
- Talk like the terminal — `hjkl`, yank, pick, jump, socket, branch. Verbs, keys, paths.
- Cut anything not earning its place. Brevity is the brand.
- Use dry humor when it's true. (The session names are a joke the tool is in on.)

**Don't**
- Reach for the marketing adjective cloud — *revolutionary, seamless, powerful, game-changing, blazingly fast.* If there's a benchmark, show the number instead.
- Decorate prose with emoji. (The chili 🌶️ is the logo and the status-bar mark — that's identity, not decoration. Everywhere else stays text.)
- Explain the terminal, version control, or LLMs to the reader. They know.
- Use exclamation marks in docs.

## Visual identity

**Logo.** A single chili pepper (`docs/assets/spyc-logo.png`). One mark,
recognizable at favicon size, never recolored off the palette below. The mark
carries the brand — there is no wordmark requirement beyond setting "spyc" in
the doc/site type.

**Palette.** The spice rack: red heat and warm accents against charcoal. Two of
these aren't just brand colors — they're what users see every session, so they're
canon, not decoration.

| Token | Hex | Role |
|---|---|---|
| Chili | `#D6391F` | Primary. Logo, brand red, links, emphasis. |
| Ember | `#FF6600` | The cursor. In-app `cursor_bg`. |
| Saffron | `#FFCB6B` | Picks / selection. In-app `pick`. |
| Charcoal | `#161310` | Terminal ground, dark surfaces. |
| Ash | `#8A7F74` | Muted text, captions, inactive elements. |
| Bone | `#EDE6D8` | Light surfaces, text on dark. |

Keep the brand red for true emphasis; if everything is hot, nothing is. Ember and
Saffron are the in-terminal accents (the orange cursor, the amber picks) — match
them in screenshots, READMEs, and the site so the brand and the running tool
agree.

**Typography.** spyc's real typeface is whatever monospace the user runs. The
only typographic claim the tool makes is the powerline status bar, which wants a
Nerd Font — with a mono fallback one keypress away (`C`). For docs and any web
surface: a clean grotesk for prose, a monospace for anything that's a command,
path, or key. Don't over-specify beyond that.

## Naming systems

**Session names.** Each run gets a two-word, all-caps, underscore-joined spice
label — `SAFFRON_CUMIN`, `ANCHO_FENNEL`, `SUMAC_CLOVE` — auto-generated, shown on
the top bar, and persisted across `spyc -r`. They make sessions memorable and
keep the brand present without a single marketing word.

Starter lexicon (extend freely): cayenne, saffron, cumin, ancho, sumac, clove,
fennel, paprika, nutmeg, cardamom, coriander, turmeric, fenugreek, mace, anise,
chipotle, harissa, dukkah, sichuan, urfa.

Rule for additions: real culinary spices, one or two syllables that read cleanly
in caps; skip anything that doubles as a plain English word and would read oddly
as a label.

**Namespace.** Internal surfaces share the `spyc` / `SPYC` namespace, for a
consistent footprint and grep-uniqueness: binary and command `spyc`; config
`~/.spycrc.toml` and `./.spycrc.toml`; runtime state `~/.local/state/spyc/`;
environment prefix `SPYC_*`; in-repo doc sigils like `SPYC-TRAP:` dereferencing
into anchors in `ARCHITECTURE.md`; the MCP context tool `get_spyc_context`.

## Messaging

**Positioning line (north star — don't change lightly):**
> The file commander is the noun the agent operates on.

**Tagline (recommended):**
> A file commander your agent can see.

Alternates, if the line above needs to flex:
- Heat for the agentic terminal.
- Keyboard-first file commanding, wired to your agent.
- The terminal file commander, rebuilt for agents.

**Elevator pitch.**
Put a coding agent in your terminal and you usually get a chat window — you
describe your working tree to it, paste paths back and forth, and lose track of
what it's looking at. spyc runs the agent in a pane beside a keyboard-driven
file commander and gives it live, structured access to exactly what you're
looking at over a local MCP socket. Pick three files and ask; the agent sees the
selection. When it names a path in its reply, press `gf` to jump there. Context
flows both ways.

**Crate / package description** (`Cargo.toml` `description`, ~one line):
> A keyboard-driven, MCP-native terminal file commander that gives your coding agent live eyes on your working tree.

Shorter, if a registry truncates:
> Keyboard-driven, MCP-native terminal file commander for working alongside coding agents.

## Lineage & courtesy

spyc is an independent project, written from scratch in Rust. Its instincts
come from a long line of keyboard-driven terminal file managers — orthodox
commanders, vi-motion browsers, and the small, fast file tools that once shipped
inside VFX pipeline software. The grammar is a tribute; the engine is new. All
third-party product names and trademarks remain the property of their owners;
spyc implies no affiliation, sponsorship, or endorsement.

## Maintainer

Maintained by TripStack (Etraveli Group) and released as open source.
License: BSD-3-Clause (the project's current license). Final public-release license pending legal review.
