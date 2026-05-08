# history-arc-08-recoverability-and-deps — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-08-recoverability-and-deps
Created: 2026-05-08T07:45:00.493522+00:00

---
Entry: Claude Code (caleb) 2026-05-08T07:45:00.493522+00:00
Role: scribe
Type: Note
Title: Framing: arc 08 — failure-mode hardening across three subgroups, cadence option A

Spec: scribe

tags: #history #arc-08

Arc title: `recoverability-and-deps`. Date span: 2026-05-03 (PR #13) to 2026-05-06 (PR #31); five PRs, two calendar clusters. Member PRs:

- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — "graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)" (commit f25d635, 2026-05-02 22:29 -04 / merged 2026-05-03 02:41 UTC).
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — "fix: route :undo / :graveyard to App's handler (v1.41.1)" (commit 24c49a0, 2026-05-02 22:48 -04 / merged 2026-05-03 03:06 UTC).
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06) — "fix: cap directory listings at 50k entries to avoid hangs (v1.41.15)" (commit f98604c, 2026-05-06 13:16 -04).
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06) — "fix: catch vt100 parser panics so spyc survives bad escape sequences (v1.41.17)" (commit bbdc415, 2026-05-06 13:48 -04).
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — "chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)" (commit fc1789d, 2026-05-06 15:00 -04).

**Diagnosis: failure-mode-hardening shape, three independent failure modes, cadence option A.** Arc 08 reads as a closing-the-arc-shape — five PRs that share an audit-of-spyc-staying-alive register but do not share a single subsystem. The pattern catalogue at arc 06's framing offered four candidate fits; only one survives inspection of arc 08's diffs:

- **Pattern 5 (postmortem-twin) at the framing register** — rejected. The brief listed this as plausible because arc 08 closes the baseline phase, but no per-PR head's diff is structurally a postmortem. Each PR is a fix-or-feature, not a retrospective. The closing-the-arc-shape is calendrical, not voice-driven.
- **Arc 06 capability-and-correction (2+2)** — rejected on grain. Arc 06's two phase-α PRs widened the chord-prefix tree and arc 06's two phase-β PRs corrected dispatch as that surface grew. Arc 08's PR #14 corrects PR #13's routing, and PR #31 corrects PR #30's defensive framing, but the corrections are local pairs — they do not register as a phase-β over a phase-α.
- **Arcs 04 / 05 capability-accretion at finer grain** — rejected because the five PRs do not extend a single surface. Arc 05's eight PRs all touched the pager; arc 04's five PRs all touched git-awareness. Arc 08's PRs touch the file-system, the directory-listing path, and the pane parser respectively — three independent code surfaces.

The diagnosis the diffs actually support: **three failure-mode subgroups, each one defending a different way the prior spyc could lose state or die**:

- **Subgroup A — file-undo (PRs #13, #14)**: pre-arc, `R` was hard-delete; PR #13 ships graveyard + `:undo` + viewer; PR #14 routes the `:undo` command 25 minutes later when state's punt list misses it.
- **Subgroup B — directory-listing (PR #28)**: pre-arc, a 1M-entry tmp directory hung the event loop until the user killed the terminal; PR #28 caps `Listing::read` at 50k and flashes a hint when truncated.
- **Subgroup C — pane parser (PRs #30, #31)**: pre-arc, vt100 0.15's `screen.rs:934.unwrap()` on a particular nvim-exit-from-alt-screen byte stream took down the entire spyc process; PR #30 wraps `parser.process` in `catch_unwind` and respawns the parser on panic; PR #31 ships vt100 0.15 → 0.16 (plus ratatui 0.29 → 0.30, ansi-to-tui 7 → 8) as the proper upstream fix, keeping PR #30's safety net.

**Cadence choice: option A — five per-PR entries plus framing and closure (seven head entries).** Arc 03's option-A precedent (five PRs) inherits cleanly; phase-not-PR (option B) does not fit because the three subgroups don't decompose into investigation-then-harvest the way arc 02 did. Each subgroup's per-PR entry picks its own internal shape per the brief's per-entry-variety expectation:

- **PR #13 (graveyard)**: substantial. The system has multiple primitives — R-undo, per-entry tar.zst archive, `gy` viewer with p/P/dd/Z/Esc keymap, `:undo` one-shot, the 500 MB cascade-to-system-trash policy at startup. The entry will narrate the four primitives with code pointers.
- **PR #14 (routing fix)**: compact. Two lines added to `AppState::dispatch_command`'s punt list; the entry is the size of the diff.
- **PR #28 (directory cap)**: feature-shaped with a specific cap and rationale. The 50k constant, the `truncated: bool` field, the `read_capped` extraction for testability, three unit tests.
- **PR #30 (panic recovery)**: substantial. The recovery mechanism itself is structurally interesting — `catch_unwind` wrapping `parser.process`, parser-replacement at the same dimensions, the `panic = "abort"` → `panic = "unwind"` profile flip without which `catch_unwind` is a no-op in release.
- **PR #31 (dep upgrade)**: captures both halves of the dep bump (vt100 0.16 forces unicode-width ≥0.2.1 which forces the ratatui major + ansi-to-tui major) and answers the deferred §3 question against PR #31's diff.

**Two within-arc supersession pairs, with diagnostically-different acknowledgement shapes.** Arc 03 named the within-arc-twin shape on PR #26 → PR #29 (3.5 hours apart, silent supersession of a single branch). Arc 08 has two such pairs, both tighter in time:

- **PR #13 → PR #14 (25 minutes apart, 2026-05-03)**: feature-plus-immediate-hotfix. PR #14's CHANGELOG describes the routing bug accurately ("State's command dispatcher routes a fixed list of names to App's terminal-touching arms; `undo` and `graveyard` weren't on it") but does not cite PR #13 by number or title. The supersession is implicit; the bug-description-without-attribution shape mirrors arc 03's PR #26 → PR #29 pair, tighter in time. Same observation territory as the arc-03 within-arc twin.
- **PR #30 → PR #31 (49 minutes apart, 2026-05-06)**: defensive-then-corrected, with **explicit reframing** in PR #31's commit body. PR #30's commit message says "vt100 0.15 is unmaintained and has known unwrap() edge cases — this specific one fires while parsing the exit-from-alt-screen byte stream" and frames the upgrade as "the right-but-bigger fix" tracked in BUGS.md MAYBE. PR #31's commit body says verbatim "The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). **Smaller than I'd previously framed it**" (commit fc1789d, 2026-05-06; emphasis preserved verbatim from the source). PR #30's BUGS.md MAYBE addition (added in the same diff) further self-corrects the unmaintained-0.15 framing: "We're pinned at 0.15.2; upstream is at 0.16.2 (active, not unmaintained — earlier notes saying 'the unmaintained 0.15' were inaccurate, corrected here)." PR #31 then deletes that MAYBE entry along with the mode-2026 and OSC 8 MAYBE entries it points at.

The two pairs land within four days, both intra-arc, both with the supersession happening within the hour. Their acknowledgement shapes differ — silent (PR #14) vs. explicitly self-correcting (PR #31). Naming this factually is arc 08's job; what the recurrence-of-tight-twins means across arcs 03 and 08 is for the insight layer.

**The PR #30 → PR #31 safety-net-bought-budget reframing.** Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) projected: "the diff shape across the 49-minute gap points toward the panic-recovery as the safety net that buys budget for the major-version dep bump." The arc-02 author handed verification to arc 08. The diff verdict: **partly true, partly refuted**. PR #30 was framed by its own commit body as the only feasible 49-minute response because the upgrade was thought too big; PR #31's commit body explicitly reframes — the upgrade turned out to be six call-site adjustments in `pane/mod.rs` (`Parser::set_size` and `set_scrollback` moved to `Screen`, accessed via `parser.screen_mut()`) and one `&` borrow drop in `widget.rs` (because vt100 0.16's `Cell::contents` returns `&str` directly). The catch_unwind safety net **persists** under PR #31 ("costs nothing on the happy path, and any third-party parser can hit edge cases on some obscure escape sequence"), so PR #30's contribution is not retired — it is reframed from "until-the-upgrade" to "belt-and-suspenders." The safety-net-bought-budget reading is therefore: ⌐load-bearing-but-retained — PR #30's 38-line `process_bytes_safe` is not preconditional to PR #31's 12-line call-site update, but its `unwind` profile flip is preserved indefinitely.

**Suspect §3 (mode-2026) verification: option (a), answered.** Arc 02's investigation entry catalogued suspect §3 verbatim and deferred verification to arc 08. Arc 03's PR #29 entry restated the deferral. Arcs 04, 05, 06, 07 did not touch the question. Arc 08 owns the answer: PR #31's diff explicitly resolves it. PR #31's BUGS.md diff removes the MAYBE mode-2026 entry verbatim (the entry PR #12 had lifted from arc 02's gap-analysis suspect §3) and adds a `(fixed, v1.41.18)` block whose closing line reads verbatim: "Also retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly." PR #31's commit body restates the same resolution. The arc-02 → arc-08 cross-thread closes as **resolution**, not deferral. The PR #31 entry below carries the verbatim citation.

**The arc 02 → arc 08 cross-thread closure.** Three top suspects from arc 02's gap-analysis are now traceable to disposition:
- §1 (cursor-block, narrow → general) — answered in arc 03 by PR #29's three-condition guard.
- §2 (mouse, anywhere) — non-executed across the whole 22-day window; aligns with `onboarding-product-charter` non-goal at `ROADMAP.md:426-447`.
- §3 (synchronized-output mode 2026) — answered in arc 08 by PR #31's vt100 0.15 → 0.16 upgrade, per the maintainer's explicit claim in CHANGELOG / BUGS.md / commit body.

That is three for three on suspect-resolution-or-deferral across the arc network. Arc 08's PR #31 entry records this as factual handoff; whether the three-for-three-reading reads as a recurring-pattern (insight-layer turf) is not arc 08's to claim.

**PR #31 advisory-ignore reduction: zero.** The `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues five long-lived `cargo-deny` advisory ignores in `deny.toml:72-94`. PR #31's diff against `deny.toml` is empty — `git diff 105db8d^1..105db8d^2 -- deny.toml` returns no changes. All five ignores survive the upgrade unchanged: RUSTSEC-2026-0009 (time via syntect→plist; MSRV-blocked), RUSTSEC-2024-0320 (yaml-rust via syntect; build-time only), RUSTSEC-2025-0141 (bincode via syntect; complete-as-shipped), RUSTSEC-2024-0436 (paste via ratatui; build-time proc-macro), RUSTSEC-2017-0008 (serial via portable-pty; no alternative). The ratatui 0.29 → 0.30 bump in particular did not eliminate the `paste` ignore even though `paste` was specifically transit-via-ratatui per the catalogue's reason. Captured here factually; the per-PR PR #31 entry will not relitigate.

**Cross-thread back-link.** This thread continues from `history-overview` and the prior arc threads:

- `history-overview` framing (entry 0) = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation (entry 1) = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 08's three-paragraph rationale at "Arc 08 — `recoverability-and-deps`"; PR #14 boundary-call called out).
- `history-overview` PR #5 special-handling (entry 2) = 01KR0TYF5F11DA8P5HNPA20DBK (arc 02 hub; PR #31's mode-2026 cross-reference).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry (entry 1) = 01KR0YXXZRQR24CSNAK4Q7808T (gap-analysis "Top suspects" §3 verbatim; deferral to arc 08).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry (entry 2) = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md MAYBE mode-2026 lift; "defer until someone notices").
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (within-arc-twin-shape precedent on PR #26 → PR #29).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (capability-and-correction precedent ruled out as a fit for arc 08).
- `history-arc-07-codex-and-mcp-bridge` story-tail = 01KR2JM67RTQHQYN0223GTKH1V (single-tail-when-material-warrants precedent).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state of pane PTY ownership; the surface PR #30/#31 touches).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (advisory-ignore catalogue and panic-unwind dependency note for `process_bytes_safe`).

The per-PR entries that follow this framing narrate PRs #13, #14, #28, #30, #31 in arc order (which is also wall-clock order). The closure entry names what the eight arcs together make legible without claiming what the insight layer will say. Tails follow the new tail voice contract.

Provenance:
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — `git show --stat 6b2be36`: 16 files, +1483/-54.
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — `git show --stat c7419c1`: 6 files, +14/-2.
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06) — `git show --stat 306b43f`: 6 files, +102/-5.
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06) — `git show --stat e39f462`: 5 files, +96/-10.
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — `git show --stat 105db8d`: 6 files, +783/-123 (the bulk in `Cargo.lock`).
- `git log` PR #31 commit body (commit fc1789d, 2026-05-06): "Smaller than I'd previously framed it" verbatim; "The catch_unwind safety net from v1.41.17 stays" verbatim.
- `git diff 105db8d^1..105db8d^2 -- BUGS.md`: removes the MAYBE mode-2026 entry verbatim; adds `(fixed, v1.41.18)` FIXED block with the "retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks)" line.
- `git diff 105db8d^1..105db8d^2 -- deny.toml`: empty.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-08 rationale).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (suspect §3 verbatim and deferral).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md MAYBE mode-2026 lift).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (deny.toml advisory-ignore catalogue; `process_bytes_safe` panic-unwind note).

<!-- Entry-ID: 01KR38QZ1XQ6EP2A4QC94DRD80 -->
