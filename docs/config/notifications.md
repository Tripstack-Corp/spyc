# Notifications — `[notify]`

Part of [Configuring spyc](../../CONFIGURATION.md).

The "which agent needs me" signal. When an agent pane changes status, spyc fires
on the **transition itself** (0 delay, not a timer). Three channels:

- **Desktop** — an OS notification / OSC-9 escape naming the tab.
- **Bell** — the terminal bell (BEL).
- **Visual** — spyc's spice-heat gradient **border pulse** (the branded flash).

## The model: Blocked vs Done, per channel

- `Blocked` ("needs me") fires **every enabled channel**.
- The routine `Done` (a finished turn, once per turn) fires a channel **only if
  that channel opts in** via its `*_done` flag.

By default the **intrusive** channels (bell + flash) stay **Blocked-only**, while
the **quiet** desktop ping also fires on `Done`. That keeps a per-turn ring/strobe
from being annoying while you still get a "finished" ping.

```toml
[notify]
desktop = true            # notify on Blocked (and Done, if desktop_done) — on by default
desktop_via = "auto"      # "auto" | "system" | "osc9" | "both"  (see below)
desktop_done = true       # also ping desktop on Done; false = Blocked-only
bell = false              # ring the terminal bell (BEL)
bell_done = false         # ring on Done too (default: Blocked-only)
visual = true             # the spice-heat border-pulse flash — on by default
visual_done = false       # flash on Done too (default: Blocked-only)
suppress_focused_tab = false  # stay quiet about the tab you're already watching
```

**`desktop_via`** — how the desktop ping is delivered:

| value | behavior |
|-------|----------|
| `auto` (default) | **OSC-9 escape over SSH** (pops on your *client* terminal) + the OS notifier locally |
| `system` | OS notifier only (`notify-rust` — the machine spyc *runs* on) |
| `osc9` | terminal escape only (needs iTerm2 / kitty / WezTerm) |
| `both` | fire both |

`auto` is the "just works over SSH" default: over SSH the ping reaches your
laptop terminal, not the remote box.

**`suppress_focused_tab`** is **off** by default on purpose — spyc having focus
isn't the same as your eyes being on the terminal; you're usually in another app
while the agent works. Set it `true` if you want the tab you're actively watching
to stay silent.

## Recipes

```toml
# Quietest useful setup: one desktop ping only when an agent is *blocked*.
[notify]
desktop_done = false
visual = false

# Maximal: ring + flash on every transition, both delivery mechanisms.
[notify]
desktop_via = "both"
bell = true
bell_done = true
visual_done = true

# Terminal-only (no OS notifications), flash on Blocked:
[notify]
desktop = false
# visual stays true → Blocked-only flash

# Silence everything:
[notify]
desktop = false
visual = false
```

Run **`:notify test`** to fire every channel on demand — verify your setup
without waiting for a real agent transition.
