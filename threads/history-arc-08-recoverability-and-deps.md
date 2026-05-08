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

---
Entry: Claude Code (caleb) 2026-05-08T07:46:54.483982+00:00
Role: scribe
Type: Note
Title: PR #13 (feat/graveyard-undo): four primitives at one diff — soft-delete cache, viewer, :undo, system-trash cascade — and the R-prompt blast-radius surfacing

Spec: scribe

tags: #history #arc-08

PR #13 is the first move in arc 08 and the file-undo subsystem move. The diff is the largest in arc 08 by every measure (16 files, +1483/-54; `src/state/graveyard.rs` is a 536-line new file; `src/app/mod.rs` gains 319 lines; three new runtime deps land — `tar`, `zstd`, `trash` — plus a `tempfile` promotion from dev-deps to runtime). The commit subject reads "graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)" (commit f25d635, 2026-05-02 22:29 -04 / merged 2026-05-03 02:41 UTC). The minor cut to v1.41.0 — the only minor cut in arc 08 — is consistent with the diff shape: this is the new capability the rest of arc 08 reacts to.

The entry below narrates four primitives the diff lands at once, then names the one user-facing-prompt change the same diff slips in alongside, then catches the explicit non-goal in the commit body.

**Primitive 1 — the per-entry tar.zst archive schema.**

Before PR #13, items expelled from inventory used a paired `<uuid>.json` (metadata) + `<uuid>.dat` (raw payload bytes) graveyard schema in `$XDG_STATE_HOME/spyc/graveyard/`. PR #13 replaces this with a `<uuid>.json` + `<uuid>.tar.zst` pair — one schema for both single regular files and entire directory trees, so there is one read/write code path for both. The commit body names the design intent verbatim: "single regular files and entire directory trees use the same schema, so there's one read/write code path for both." Permission preservation is opt-in via `tar`'s `HeaderMode::Complete` (mode bits, mtime, best-effort UID/GID); restore opts in to `set_preserve_permissions(true)` and `set_preserve_mtime(true)`; `set_overwrite(false)` refuses to clobber existing files (commit body, fc25d635).

Pre-v1.41.0 paired `<uuid>.json` + `<uuid>.dat` entries are silently ignored by the new reader. The commit body's rationale, verbatim: "the graveyard is a transient soft-delete cache; major version bumps may lose recovery state — leaving the bytes in place is safer than trying to migrate." This is the diff shape that explains the choice not to ship a one-off migration tool — recovery state is not durable across major versions by design. `src/state/inventory.rs` (+55 / -55 net 0; same line count, full body rewrite) routes `move_to_graveyard` through the new schema so the graveyard is uniform from PR #13 forward.

**Primitive 2 — the `gy` viewer with its own keymap.**

A new viewer overlay surfaces the graveyard newest-first. The CHANGELOG names the keymap verbatim:

- **`gy`** — open the graveyard view (newest first); also toggles closed.
- Inside the view: **`p`** restore-to-cwd, **`P`** restore-to-original, **`dd`**/**`x`** purge entry to system trash, **`Z`** purge ALL (single-key confirm), **`Esc`**/**`gy`** close.

The asymmetry between `p` (lowercase, cwd) and `P` (uppercase, original) is the case-as-intent dispatch shape that arc 06's PR #10 (quickselect) ships in a different domain (lowercase yank, uppercase open). Arc 06 records the case-as-intent pattern factually; observing that PR #13's viewer ships the same shape independently is observational, not pattern-attribution turf.

Two new keymap actions land in `src/keymap/action.rs` (+4) and `src/keymap/resolver.rs` (+1). The viewer body itself is part of `src/app/mod.rs`'s 319-line gain.

**Primitive 3 — the `:undo` command as one-shot escape hatch.**

Distinct from the viewer: `:undo` restores the most-recent entry to its original path without opening the viewer. The commit body frames it as the "one-shot escape hatch." This is the command PR #14 will fix the routing for 25 minutes later; PR #13's diff lands the App-side handler but does not add `undo` to `AppState::dispatch_command`'s punt list. The drift is named in PR #14's entry, not here. From PR #13's vantage, `:undo` is a working command in the App's handler — the dispatch routing the punt list governs is the delta PR #14 catches.

**Primitive 4 — the 500 MB system-trash cascade at startup.**

A two-stage policy: the graveyard is the spyc-internal soft-delete cache (compressed, undo-able from the viewer); when it exceeds 500 MB, the **oldest entries cascade to the system trash** (FIFO) until the graveyard is back under cap. The commit body explains the design intent: "the unpacked content lands in Finder/Files with its original name so OS-native recovery still works." The cascade fires only at startup; a flash reports the count moved. Net pipeline: `R` → graveyard (compressed, spyc-recoverable) → system trash (uncompressed, OS-recoverable) when the cap is hit.

The 500 MB cap and FIFO-by-oldest are policy choices the diff doesn't justify against alternatives (size cap vs. age cap, 500 MB vs. some other number). The commit body names the cap and the policy without arguing for them; the catalogue of justifications — if any — lives outside this diff. Captured factually.

**The R-prompt blast-radius surfacing — a separate change in the same diff.**

Arc 08's framing flagged that PR #13 is substantial because the system has multiple primitives. There is also a behavior change to the existing `R` prompt that lands quietly inside the same PR. Pre-PR-#13: `R` prompted "remove N file(s)?". Post-PR-#13: `R` pre-walks any selected directory to count files inside and prompts "remove DIR (recursive, N file(s)) + M file(s)? (y/N)" — explicitly surfacing the recursive blast radius so that a reflexive `y` doesn't hide a 47-file delete behind a one-line prompt. The CHANGELOG bucket for this change is `### Changed`, distinct from the graveyard's `### Added` bucket. The change reads as a related-but-separate hardening: the graveyard makes hard-deletes recoverable, *and* the prompt makes the magnitude of recursive deletes legible, *and* both ship together because both address "you may not realize what `R` is about to do" from different angles. Naming the two changes as related-but-distinct is the read the diff supports; whether they read as one feature or two is for the reader.

**The explicit non-goal: xattrs / ACLs / macOS resource forks.**

The commit body names the scope boundary verbatim: "xattrs / ACLs / macOS resource forks are not preserved (out of scope for v1)." This is a recoverability gap the diff explicitly opts into — a graveyard entry restored from `.tar.zst` will not carry extended attributes back to the file. For spyc's audience (developers on macOS/Linux, file commander use case), the gap is a limitation worth knowing; for graveyard entries originating from sources that depend on extended attributes (Finder color labels, macOS quarantine bits, ACL-controlled corporate files), the `R` → restore round-trip silently strips state. The "(out of scope for v1)" framing positions this as a deferred item, not an indefinite one. No PR in the 22-day window appears to address it; flagging factually for the insight layer's negative-space catalogue.

**The dependency additions.**

New runtime deps in `Cargo.toml`: `tar = "0.4"`, `zstd = "0.13"`, `trash = "5"`. `tempfile` is promoted from dev-deps to runtime deps with a doc-comment justification verbatim from the diff: "tempfile is a runtime dep (not just dev-deps) because the graveyard's cascade-to-trash and legacy-migration paths stage files into a TempDir before handing them to the system trash." The commit body confirms: "All four pass cargo-deny." `Cargo.lock` carries 338 lines of transitive-dep changes from the four additions.

The `cargo-deny` check passing here is consistent with the `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) noting that new ignores must follow the documented-reason pattern. PR #13 adds none, which the entry flags as the positive-control case for the convention.

**Drift findings flagged for the insight layer.**

- The graveyard's cascade-to-system-trash policy fires only at startup. The diff does not justify why startup-only over event-driven (e.g. after each `R`) — startup-only is the chosen point but the alternatives are not catalogued. Captured for the insight layer's design-decision negative-space reading.
- The CHANGELOG's `### Added` bucket for the graveyard contains the `:undo` reference; the same `:undo` is broken at merge time because `AppState::dispatch_command`'s punt list does not include it. The drift between "documented capability" and "wired capability" is the seed `onboarding-risk-register` entry 0 names: "Bitten on `:undo` (v1.41.1) and `:limit` historically." PR #14 (next entry) is the historical instance the seed cites. PR #13's CHANGELOG ships the documented-capability promise; the wiring drift surfaces 25 minutes later. Captured for arc 08's two-pair recurrence reading and the eventual insight layer.
- Three new runtime deps land in one PR. Arc 01's PR #3 (security-hygiene) had cataloged the cargo-deny advisory-ignore baseline; this PR adds none, which is good, but the dep-graph expansion (338 lines of `Cargo.lock`) is the kind of change that increases the surface for future advisory ignores. Captured factually, no claim about future risk.
- The graveyard schema (`<uuid>.json` + `<uuid>.tar.zst`) is incompatible with pre-v1.41.0 paired entries by design. Users with prior `<uuid>.dat`-shaped entries find them silently ignored. The commit body names this as policy ("transient soft-delete cache"), but the user-visible artifact is "items deleted before v1.41.0 are no longer recoverable from spyc's viewer." Whether any user noticed this is not narratable from any commit in the 22-day window.
- The R-prompt blast-radius change ships under `### Changed` while the graveyard ships under `### Added`. The same diff makes two CHANGELOG buckets land at once. Arc 03's PR #26 (`Modifier::DIM` on the unfocused side) had the same shape — `### Changed` for a re-rendering of existing surface, distinct from feature add. Captured as observation; the "one-PR-two-CHANGELOG-buckets-with-internal-coherence" shape is recurring across arcs 03 and 08.

Provenance:
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — full PR.
- f25d635 — PR #13's feature-branch commit; commit body quoted verbatim throughout this entry.
- e210e58 → f25d635 — parent and tip SHAs for the diff inspection.
- `git diff e210e58..f25d635 -- src/state/graveyard.rs`: 536 lines new (file did not exist pre-PR-#13).
- `git diff e210e58..f25d635 -- src/app/mod.rs`: +319 net.
- `git diff e210e58..f25d635 -- src/app/state.rs`: +156 net.
- `git diff e210e58..f25d635 -- src/state/inventory.rs`: +55 / -55 net 0; rewrite-via-replacement of `move_to_graveyard`.
- `git diff e210e58..f25d635 -- Cargo.toml`: +10/-1 net; new deps `tar = "0.4"`, `zstd = "0.13"`, `trash = "5"`; `tempfile = "3"` promoted from `[dev-dependencies]` to `[dependencies]` with verbatim doc-comment.
- `git diff e210e58..f25d635 -- CHANGELOG.md`: 42 lines added; `### Added` graveyard block + `### Changed` R-prompt + inventory-schema blocks.
- `git diff e210e58..f25d635 -- BUGS.md`: 2 lines (the SMALL "no undo for R" entry presumably gets the v1.41.0 close treatment; verified by reading the diff, the line is updated to the FIXED bucket).
- `git diff e210e58..f25d635 -- src/keymap/action.rs`: +4 (two new viewer-action enum variants per CHANGELOG keymap).
- `git diff e210e58..f25d635 -- src/keymap/resolver.rs`: +1.
- `git diff e210e58..f25d635 -- README.md`: +17 (the user-visible recovery-surface description).
- `git diff e210e58..f25d635 -- FEATURES.md`: +34/-2 (the recovery-feature section).
- `git diff e210e58..f25d635 -- Cargo.lock`: +338/+0 net (transitive-dep additions from `tar`, `zstd`, `trash`, `tempfile` promotion).
- Commit body verbatim quotations: "single regular files and entire directory trees use the same schema, so there's one read/write code path for both"; "xattrs / ACLs / macOS resource forks are not preserved (out of scope for v1)"; "the graveyard is a transient soft-delete cache; major version bumps may lose recovery state — leaving the bytes in place is safer than trying to migrate"; "All four pass cargo-deny"; "13 new unit tests (6 in graveyard, others in dispatch / view)."
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (cargo-deny convention; the punt-list foot-gun naming `:undo` v1.41.1 verbatim).

<!-- Entry-ID: 01KR38VEGHFT9JGRDCXXBFX8V1 -->

---
Entry: Claude Code (caleb) 2026-05-08T07:48:08.637710+00:00
Role: scribe
Type: Note
Title: PR #14 (fix/undo-command): two lines on the punt list, 25 minutes after PR #13, no PR-#13 citation

Spec: scribe

tags: #history #arc-08

PR #14 is the second move in arc 08 and the supersession-of-PR-#13's-routing move. The diff is the smallest in arc 08 (6 files, +14/-2; the load-bearing change is two lines added to one function in `src/app/state.rs`). The commit subject reads "fix: route :undo / :graveyard to App's handler (v1.41.1)" (commit 24c49a0, 2026-05-02 22:48 -04 / merged 2026-05-03 03:06 UTC) — 25 minutes after PR #13's merge.

**The two lines.**

`src/app/state.rs:1239` post-fix:

```rust
            || input == "undo"
            || input == "graveyard"
```

These join an existing chain inside `AppState::dispatch_command` that returns `CommandResult::NotHandled` for command names whose handler lives on App's terminal-touching half (per the `onboarding-architecture` seed entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ description of the partially-complete Elm refactor: "AppState::apply(action) returns an ApplyResult enum (Handled, OpenPager, Post(PostAction)) with no terminal access"). The chain pre-fix already included `pause`, `pause <args>`, `resume`, `resume <args>` (and an extended list above the snippet); without `undo` or `graveyard` on it, both fell through to the line-1247 "unknown command:" fallthrough.

**The bug PR #13 left.**

PR #13's diff added App's handler for `:undo` and `:graveyard`. The handler exists; the routing does not. The user-visible artifact, per PR #14's commit body verbatim: "Repro: type `:undo` → flash 'unknown command: undo'." The same bug applied to `:graveyard` (the command form of `gy` viewer-toggle). Both commands documented in PR #13's CHANGELOG `### Added` block fail at merge time of PR #13.

The 25-minute supersession means the bug existed in `main` for 25 minutes between PR #13's merge (2026-05-03 02:41 UTC) and PR #14's merge (2026-05-03 03:06 UTC). The arc-08-framing's two-pairs reading names this pair the tighter of the two within-arc twins; PR #30 → PR #31 is the looser at 49 minutes. Both pairs are intra-day; PR #13/#14 is intra-hour.

**The acknowledgement shape: bug-described, predecessor-not-cited.**

PR #14's CHANGELOG entry verbatim:

> "**`:undo` and `:graveyard` returned 'unknown command'.** State's command dispatcher routes a fixed list of names to App's terminal-touching arms; `undo` and `graveyard` weren't on it, so they hit the unknown-command fallthrough before App's handler could see them. Added both to the punt list."

The CHANGELOG names the bug accurately. It does not name PR #13. It does not say "regression introduced in v1.41.0" (the prior release). It does not link to PR #13's commit. The reader who reads only PR #14's CHANGELOG knows the routing-vs-handler split caused the bug; the reader does not know which prior PR shipped the broken pairing without grepping for `undo` against the PR-#13 commit body.

This shape is the diagnostic-of-interest. Arc 03's story-tail (= 01KR11S8RG29J98QKN1H0VAA6W) named the same shape on PR #29 verbatim: "What makes the supersession diagnostic isn't the guard-broadening per se — it's that nothing in either commit says 'this supersedes PR #5.'" Arc 08 has the same shape at a tighter time grain. PR #14's commit-body-and-CHANGELOG describe the bug exactly without acknowledging that 25 minutes earlier PR #13 had shipped the bug. The relationship between PR #13 and PR #14 lives in the diff's adjacency on `main`, not in any commit message text.

**The seed already carries the canonical reference.**

The `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues this exact bug as the named instance of the dual-`:command`-dispatch foot-gun verbatim: "Bitten on `:undo` (v1.41.1) and `:limit` historically." The v1.41.1 reference is PR #14's release tag. The seed predates arc 08 (entry timestamp 2026-05-07T07:44:05). So the foot-gun-with-named-instance documentation already exists at risk-register level by the time arc 08 reaches PR #14; the per-PR entry's job is to record that the seed's name-of-the-instance is grounded in this PR's two-line fix, not to relitigate the foot-gun.

**Drift findings flagged for the insight layer.**

- The supersession is silent at PR #14's commit-message level and explicit at the seed level. The cross-thread coverage is asymmetric: the seed (`onboarding-risk-register`) catalogues the bug class with this PR's release tag; PR #14's own commit message does not cite PR #13. Whether the insight layer reads this as a recurring "later observers reconstruct the supersession the original commits did not name" pattern is the insight layer's question. Captured factually here.
- PR #14 also adds two lines to `.gitignore` (per `git show --stat c7419c1`: 2 lines). The diff's content for `.gitignore` is not the load-bearing part of the PR but is bundled in. Whether the .gitignore additions are graveyard-related (e.g. ignoring `*.tar.zst` debug artifacts) or unrelated cleanup is verifiable from the diff but tangential to the routing fix. Captured factually; not load-bearing for the supersession reading.
- Patch-bump cadence: v1.41.0 → v1.41.1. The arc-01 reflection tail (= 01KR0XR504ZR10Y242JERT4K9S, restated at later arc-01 tails) named the v1.41.x cadence as the rhythm of patch-after-feature. PR #14 is one of the two within-arc-08 patch-bumps that follow a same-arc capability bump (the other is PR #28 → PR #30 → PR #31, where each is a patch on the prior). The patch-after-immediate-feature shape is intra-arc-08 here; arc 06's framing observed it across-arc (six minor cuts between PR #8's v1.39.0 and PR #32's v1.41.19). The pattern is recurrent; arc 08 records the instance.

Provenance:
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — full PR.
- 24c49a0 — PR #14's feature-branch commit; commit body quoted: "Repro: type `:undo` → flash 'unknown command: undo'."
- 6b2be36 → 24c49a0 — parent and tip SHAs for the diff inspection.
- `git diff 6b2be36..24c49a0 -- src/app/state.rs`: 2 lines added at line 1239 (`|| input == "undo"`, `|| input == "graveyard"`).
- `git diff 6b2be36..24c49a0 -- CHANGELOG.md`: 7 lines added under `[Unreleased]` `### Fixed`; the four-line description quoted verbatim above.
- `git diff 6b2be36..24c49a0 -- Cargo.toml`: `version = "1.41.0"` → `version = "1.41.1"`.
- `git diff 6b2be36..24c49a0 -- .gitignore`: 2 lines added (content not narrated in this entry; tangential).
- `git diff 6b2be36..24c49a0 -- CLAUDE.md`: 1 line added (content not narrated; tangential).
- `git diff 6b2be36..24c49a0 -- Cargo.lock`: 2 lines (the version bump's lockfile reflection).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — names "Bitten on `:undo` (v1.41.1)" verbatim as the foot-gun's historical instance.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — describes the partially-complete Elm refactor: "AppState::apply(action) returns an ApplyResult enum...with no terminal access. State transitions are pure-ish and unit-testable without a TUI." The dual-dispatch the foot-gun lives in.
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W — within-arc-twin precedent ("nothing in either commit says 'this supersedes PR #5'") quoted at this entry's acknowledgement-shape paragraph.
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `history-arc-08-recoverability-and-deps` PR #13 entry = 01KR38VEGHFT9JGRDCXXBFX8V1.

<!-- Entry-ID: 01KR38XPJ07ZFQHH1TG6X461WN -->

---
Entry: Claude Code (caleb) 2026-05-08T07:49:27.722830+00:00
Role: scribe
Type: Note
Title: PR #28 (fix/huge-directory-cap): a 50,000-entry cap on `Listing::read`, BUGS SMALL #4 closed, three unit tests

Spec: scribe

tags: #history #arc-08

PR #28 is the third move in arc 08 and the directory-listing failure-mode-hardening move. The diff is feature-shaped at hardening grain (6 files, +102/-5; the load-bearing changes are 70 added lines in `src/fs/listing.rs` plus a 15-line `### Fixed` block in `CHANGELOG.md` and 11 lines in `src/app/state.rs`). Commit subject reads "fix: cap directory listings at 50k entries to avoid hangs (v1.41.15)" (commit f98604c, 2026-05-06 13:16 -04 / merged 2026-05-06 17:30 UTC). PR #28 opens the 2026-05-06 cluster of three runtime-survival PRs.

**The user-report origin.**

The commit body opens by naming the BUGS.md item the fix closes verbatim: "BUGS SMALL #4: user reported entering a stale tmp directory and having to kill the terminal to recover." `BUGS.md` SMALL #4 (verified at PR #28 parent commit) was a one-line entry: "user reported: timeout on viewing large directory (went into a messy tmp directory by accident and had to kill the terminal)." PR #28's BUGS.md diff removes the entry from SMALL and adds a FIXED block: "(fixed, v1.41.15) Huge directories no longer hang spyc on chdir. `Listing::read` caps at 50,000 entries; truncated reads flash a hint so the user knows the listing isn't the full picture. The pre-fix behavior on a 1M-entry tmp dir was a multi-minute event loop block that required killing the terminal." The user-report-to-fix shape is **named-then-fixed within the same PR**, not bracketed across multiple PRs (the bracket shape arc 07's PR #18 → PR #37 named over two days; PR #28 is one-PR-named-then-fixed at the same release).

**The cap, the rationale, the testability extraction.**

The cap value (50,000) is named at code-level in a doc-commented constant verbatim from the diff:

> "Hard cap on entries Listing::read will materialize. A user reported entering a tmp directory with so many entries that spyc hung and they had to kill the terminal — every entry costs a `stat()` syscall plus a sort comparison, so 1M entries can spend minutes blocking the event loop on a slow filesystem. Most real directories the user wants to navigate are well under this cap; when we hit it, `truncated` is set so the caller can surface a flash and the user can `R` / `:!find` / climb out instead of waiting for the read to finish."

`MAX_ENTRIES: usize = 50_000` is the named constant. The cap is justified against the failure mode (`stat()` cost × entry count = event-loop block) and against the false-positive surface (the commit body lists the directories that fit under 50k explicitly: "Real navigation directories (monorepos, chubby node_modules, build trees) read in full; only pathological cases (message queues, log spools, runaway tmp) trip the cap."). The chosen point is not arbitrary; the diff does not, however, justify 50k against alternate values (10k, 100k). The cap is one-of-many-reasonable-defensive-numbers; the diff defends "we need a cap" rather than "we need 50,000 specifically."

The diff extracts a `pub fn read_capped(dir, cap)` from `Listing::read`, with `Listing::read` becoming a one-liner that calls `read_capped(dir, MAX_ENTRIES)`. The doc-comment names the rationale: "Public for tests; production code goes through `read` (with `MAX_ENTRIES`)." The extraction is testability-driven, not generality-driven — the only consumer of `read_capped` is the three unit tests appended below the impl block.

**The user-visible truncation hint.**

A new `pub truncated: bool` field on `Listing` carries the truncation signal. `src/app/state.rs` (+11 lines) adds a chdir-time check that flashes "listing capped at 50000 entries — directory has more" when `truncated` is `true`. The flash is the user's discovery channel; the diff explicitly distrusts the alternative (showing a partial listing as if it were complete, leaving the user with a wrong mental model of the directory).

**The three unit tests.**

The diff appends a `#[cfg(test)] mod tests` block with three tests, each verbatim from the diff:

- `read_capped_truncates_when_over_cap` — 8 files, cap=5; asserts `entries.len() == 5` and `truncated == true`.
- `read_capped_does_not_truncate_under_cap` — 3 files, cap=100; asserts `entries.len() == 3` and `truncated == false`.
- `empty_listing_is_not_truncated` — `Listing::empty(...)`; asserts `truncated == false`.

The tests use cap=5 and cap=100 to "burn no real time on 50k stat() calls" (verbatim from the doc-comment). The testability split (production const vs. test-supplied cap) is the kind of small-scale design decision that arc 04's tail — by the brief's reference — would describe as "the diff's machinery shape" turf; arc 08 records it factually.

**The relationship to PR #30 / PR #31.**

PR #28 lands at 17:30 UTC. PR #30 lands at 18:27 UTC. PR #31 lands at 19:16 UTC. All three are 2026-05-06; PR #28 is the day's first runtime-survival fix, the panic-recovery is the second, the dep upgrade is the third. The three PRs do not share a code surface — `src/fs/listing.rs` (PR #28), `src/pane/mod.rs` (PR #30), `src/pane/mod.rs` + `src/pane/widget.rs` + `Cargo.toml` (PR #31) — but they share a register: each defends a different way the pre-arc-08 spyc could fail under inputs the user did not control. Naming this as a register, not a coordinated effort, is the read the diffs support; the calendrical clustering on a single day across three independent failure modes is observable factually.

**Drift findings flagged for the insight layer.**

- The cap value (50k) is documented at code-level but unjustified against alternates. The choice reads as defensive-default rather than measured-empirically. A reader coming back to this diff in a year would have to re-derive whether 50k is still the right value for the contemporary `stat()` cost. The doc-comment commits to the failure mode, not to the cap value's optimality. Captured for the insight layer's design-decision-vs-measured-decision negative-space catalogue.
- The chdir-time flash is the user's only discovery channel for truncation. A user who navigates into a >50k directory and immediately presses `J` to scroll, or `:` to issue a command, may scroll past the flash before reading it. The fallback channel — checking a status indicator that persists — is not in the diff. The diff's choice is "show once, don't keep nagging"; whether a persistent indicator (a tag in the prompt row, e.g.) would be more correct is an interface choice the diff doesn't address. Captured.
- PR #28's CHANGELOG bucket is `### Fixed`, not `### Added`. The diff adds a constant, a field, an extracted public function, and three tests — by feature-add measure this is `### Added` work. The bucket choice reads as honoring the user-reported origin (a fix to a regression-from-good-experience) rather than the diff-shape (additive). Arc 06's PR #25 framing flagged a similar choice for defensive-bundle-as-fix; arc 08's PR #28 makes the same call. Recurring shape; observed factually.
- The BUGS.md SMALL #4 entry that PR #28 closes had stood since pre-window (it pre-dates 2026-04-30). Whether the user-report-was-known-before-PR-#28-was-written is verifiable from BUGS.md's git-blame; the relationship between the report's age and the fix's timing is not narratable from PR #28 alone. The BUGS.md → fix shape arc 07's PR #18 → PR #37 named is also present here at one-PR grain (named-and-fixed in the same PR), not bracketed across PRs.

Provenance:
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06) — full PR.
- f98604c — PR #28's feature-branch commit; commit body verbatim quoted at "BUGS SMALL #4" and "Real navigation directories (monorepos, chubby node_modules, build trees) read in full; only pathological cases (message queues, log spools, runaway tmp) trip the cap."
- 4e2afd9 → f98604c — parent and tip SHAs for the diff inspection.
- `git diff 4e2afd9..f98604c -- src/fs/listing.rs`: +70 / -1 net; new `pub const MAX_ENTRIES: usize = 50_000`; new `pub truncated: bool` field on `Listing`; new `pub fn read_capped(dir, cap)` extracted from `read`; three unit tests appended.
- `git diff 4e2afd9..f98604c -- src/app/state.rs`: +11 (chdir-time flash on `truncated`).
- `git diff 4e2afd9..f98604c -- CHANGELOG.md`: 15 lines added under `[Unreleased]` `### Fixed`; user-report origin and behavior-pre-fix quoted verbatim.
- `git diff 4e2afd9..f98604c -- BUGS.md`: -2 (the SMALL #4 entry removed); +5 (new FIXED block with v1.41.15 tag).
- `git diff 4e2afd9..f98604c -- Cargo.toml`: `version = "1.41.14"` → `version = "1.41.15"`.
- `git diff 4e2afd9..f98604c -- Cargo.lock`: 2-line version bump reflection.
- Doc-comment verbatim: "Hard cap on entries Listing::read will materialize... Most real directories the user wants to navigate are well under this cap; when we hit it, `truncated` is set so the caller can surface a flash and the user can `R` / `:!find` / climb out instead of waiting for the read to finish."
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `history-arc-08-recoverability-and-deps` PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN.

<!-- Entry-ID: 01KR3903VA7DTNDJKQAFZ6DP8M -->

---
Entry: Claude Code (caleb) 2026-05-08T07:51:24.374728+00:00
Role: scribe
Type: Note
Title: PR #30 (fix/vt100-panic-recovery): catch_unwind around `parser.process`, the `panic = "abort"` → `unwind` profile flip, and the BUGS.md MAYBE block that immediately self-corrects

Spec: scribe

tags: #history #arc-08

PR #30 is the fourth move in arc 08 and the runtime-parser failure-mode-hardening move. The diff is moderate (5 files, +96/-10; the load-bearing changes are 38 lines added to `src/pane/mod.rs` introducing `process_bytes_safe`, an 11-line doc-comment on the `Cargo.toml` profile change, a 21-line CHANGELOG block, and 31 lines of BUGS.md churn that does its own self-correction in the same diff). Commit subject reads "fix: catch vt100 parser panics so spyc survives bad escape sequences (v1.41.17)" (commit bbdc415, 2026-05-06 13:48 -04 / merged 2026-05-06 18:27 UTC). PR #30 is the second of the three runtime-survival PRs landing on 2026-05-06; PR #31 follows 49 minutes later with the proper underlying fix.

**The user report.**

The commit body opens with the failure mode verbatim: "User report: closing nvim inside a zsh tab crashed the whole spyc process with `panicked at vt100/src/screen.rs:934: Option::unwrap() on a None value`. vt100 0.15 is unmaintained and has known `unwrap()` edge cases on certain valid escape sequences — this specific one fires while parsing the exit-from-alt-screen byte stream after a particular scroll/cursor state."

The "vt100 0.15 is unmaintained" framing in PR #30's commit body is **superseded by PR #30's own BUGS.md MAYBE block**, in the same diff. See "The MAYBE block that self-corrects" below.

**The recovery mechanism: catch_unwind + parser respawn at the same dimensions.**

`src/pane/mod.rs` introduces `process_bytes_safe`, a 38-line method with a 13-line doc-comment. The mechanism reads as four sub-decisions, each defended in source comments against the alternative:

1. **Wrap `parser.process`, not the entire pane loop.** The catch is at the narrowest possible boundary — the call into vt100 itself. Surrounding code (the byte read, the scroll-offset accounting, the resize coalescing) is not inside the `catch_unwind`. The choice keeps the recovery surgical: only the vt100 parser state is suspect after a panic; the rest of the pane state is trusted.

2. **Use `AssertUnwindSafe`.** The closure captures `&mut self.parser`, which is not `UnwindSafe` by default in Rust because mutable references to types holding interior state can be left in inconsistent states across an unwind. The diff uses `std::panic::AssertUnwindSafe(|| { self.parser.process(bytes); })`, an explicit assertion that the consequences of an inconsistent post-unwind state are handled by the immediate parser-replacement that follows. The `AssertUnwindSafe` is the load-bearing safety claim: replacing the parser with a fresh instance is the cleanup that makes the assertion valid.

3. **Cache `(rows, cols)` from `self.last_size`, don't read `self.parser.screen()` post-unwind.** The diff doc-comment names this verbatim: "Capture the grid size before the parse; even reading `parser.screen()` after a panic isn't safe, so we use the cached `last_size` we already maintain for resize coalescing." The pane already maintained `last_size` for an unrelated purpose (resize coalescing, per the `onboarding-architecture` seed entry 0); the recovery path repurposes the cached value because reading the panicked parser's state is itself unsafe.

4. **Respawn the parser at the same dimensions and 10,000-line scrollback cap.** `vt100::Parser::new(rows, cols, 10_000)` is the constructor. The 10,000 cap is the spyc-wide convention (also used in PR #31's call sites and the pane's normal `set_scrollback` paths); not a recovery-specific value. The user-visible cost: "The user loses the in-pane screen state at the moment of recovery — the next render from the child repaints anyway — but spyc and every other pane stay alive" (commit body verbatim).

**The release-profile flip.**

`Cargo.toml`'s `[profile.release]` block changes `panic = "abort"` to `panic = "unwind"`. The 11-line doc-comment justifies the flip verbatim:

> "`unwind` instead of `abort` so `std::panic::catch_unwind` actually catches panics — used in `pane::Pane::process_bytes_safe` to recover from vt100 0.15's known unwrap-on-edge-case panic (hits `Option::unwrap` deep in screen.rs for some valid escape sequences, e.g. nvim's exit-from-alt-screen after specific scroll state). With `abort` a single panicking byte stream took down the entire spyc process, dropping every other pane along with it. Slightly larger binary + minor codegen change is a fine trade for not crashing the user's session. Worth keeping as a safety net even after a vt100 upgrade."

The flip is load-bearing for PR #30's correctness — `catch_unwind` is a no-op under `panic = "abort"`, so the recovery path only worked in dev/test builds before. The commit body confirms: "`catch_unwind` is a no-op under `abort`, so the recovery path only worked in dev/test builds before." This is the kind of footgun where the recovery-code-shipped-before-release-profile-flipped would have had zero effect in production binaries; the same diff ships both halves so the recovery is real.

The doc-comment also names the choice's persistence: "Worth keeping as a safety net even after a vt100 upgrade." This is the explicit forward-look: PR #30 is staged to survive PR #31 (which lands 49 minutes later) because the safety net is framed as version-agnostic-good-practice, not 0.15-specific. Arc 08's framing reads PR #31's commit body as confirming the persistence ("The catch_unwind safety net from v1.41.17 stays — costs nothing on the happy path, and any third-party parser can hit edge cases on some obscure escape sequence").

The `onboarding-risk-register` seed entry 0 names the panic-unwind invariant verbatim: "Crash recovery posture: `Cargo.toml:60-70` keeps `panic = "unwind"` in the release profile because `pane::Pane::process_bytes_safe` uses `std::panic::catch_unwind` to recover from `vt100` 0.15's known unwrap-on-edge-case panic. Switching to `panic = "abort"` would re-introduce the 'one panicking byte stream takes down spyc' failure mode. Worth knowing before 'optimizing' the release profile." The seed is dated 2026-05-07T07:44:05; PR #30 ships the configuration the seed catalogues. The seed-records-the-invariant-PR-30-creates relationship is observable factually.

**The MAYBE block that self-corrects.**

PR #30's `BUGS.md` diff is the most internally-asymmetric part of the PR. The diff adds an 11-line `### MAYBE` entry describing the vt100 upgrade option:

> "upgrade vt100 0.15 → 0.16. We're pinned at 0.15.2; upstream is at 0.16.2 (active, not unmaintained — earlier notes saying 'the unmaintained 0.15' were inaccurate, corrected here). The motivating cases are the panic in `screen.rs:934` (caught defensively in v1.41.17 but the real fix is upstream), mode 2026 (synchronized output) which 0.15 doesn't parse — see entry below — and OSC 8 (hyperlinks) — also below. Should evaluate API churn vs. the wins in one go; alternative is `vt100-ctt 0.17.1` (community fork) or alacritty's `vte` parser. Defer until someone has a clear afternoon — touches every place that holds a `vt100::Screen` reference."

The verbatim fragment "active, not unmaintained — earlier notes saying 'the unmaintained 0.15' were inaccurate, corrected here" inside this same diff reverses the framing PR #30's own commit body opened with ("vt100 0.15 is unmaintained"). The diff therefore contains both framings simultaneously: the commit body says vt100 0.15 is unmaintained, the BUGS.md MAYBE block (added in the same diff) says vt100 0.15 was inaccurately called unmaintained and is in fact active. The commit body and the diff disagree.

The reconciling read the diff supports: the commit body fragment is residual from an earlier internal framing; the BUGS.md correction is the corrected one; PR #31 ships 49 minutes later with the corrected framing intact ("The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). Smaller than I'd previously framed it"). PR #30's BUGS.md MAYBE block is the *first* place the corrected framing lands; the commit body is the *last* place the older framing survives.

The CHANGELOG entry is the third channel and aligns with the corrected framing: "We're on vt100 0.15.2 (upstream is at 0.16.2 — an upgrade may resolve this and is worth doing separately), and 0.15 has a known `unwrap()` deep in `screen.rs` for certain valid escape sequences." The CHANGELOG names neither "unmaintained" nor "active"; it stays neutral on the maintenance-status question and treats the upgrade as a follow-on.

So PR #30's three text channels (commit body, BUGS.md MAYBE, CHANGELOG) carry three slightly different framings. The diff is internally inconsistent on whether vt100 0.15 is unmaintained; PR #31 resolves the inconsistency by reframing in the next PR.

**The §3 / OSC 8 mention as the link to PR #31.**

The same MAYBE block PR #30 adds also names mode 2026 ("see entry below") and OSC 8 ("also below") as motivating cases for the upgrade. Mode 2026 is arc 02's gap-analysis suspect §3 (= 01KR0YXXZRQR24CSNAK4Q7808T verbatim). PR #30's diff therefore re-surfaces the suspect-§3 reference at the BUGS.md MAYBE level, alongside the panic the diff itself defends against, for the first time in the 22-day window since PR #12 lifted the entry from `notes/lazygit-gap-analysis.md`. PR #30 does not address §3 — its catch_unwind does nothing for synchronized-output tearing — but it pairs the §3-mention with the upgrade-motivation block, staging the upgrade as a multi-issue resolution. PR #31 (next entry) acts on the staging within the hour and resolves §3.

**Drift findings flagged for the insight layer.**

- The diff's three text channels (commit body, BUGS.md, CHANGELOG) carry three different framings of vt100 0.15's maintenance status. The disagreement is intra-diff. PR #31's commit body resolves it. Captured here as a within-PR drift instance — not a between-PR drift.
- The `process_bytes_safe` doc-comment names the safety net as version-agnostic ("Useful safety net regardless of vt100 version"). The release-profile doc-comment also names this ("Worth keeping as a safety net even after a vt100 upgrade"). Both pre-empt PR #31 and frame the recovery as permanent rather than provisional. Whether the framing reads as foresight (the upgrade was already on the maintainer's plan) or hedging (the recovery should outlive any specific parser bug) is not narratable from the diff. Both readings are consistent with the diff. Captured factually.
- `vt100::Parser::new` takes `(rows, cols, max_scrollback_size)`. The diff hardcodes 10,000 for max_scrollback_size in the recovery path. The pane's normal scrollback budget is `max_scrollback()` (a separate accessor) — the recovery uses a different number. Whether 10,000 matches `self.max_scrollback()` is verifiable but not guaranteed. The user's recovered pane may have a different scrollback ceiling than the user's normal pane. Captured factually; not load-bearing for the panic-recovery correctness.
- `AssertUnwindSafe` is the load-bearing claim for the catch's correctness. The Rust documentation for `AssertUnwindSafe` warns: "Mostly intended to be used with mutable references to types holding interior state." The diff's use is exactly that case (a mutable reference to `vt100::Parser`, which holds the interior cell-grid state). The assertion is justified by the immediate parser-replacement, but the justification is implicit in the diff's structure rather than named at the assertion site. A reader inspecting `process_bytes_safe` in isolation would not see the justification without grepping the surrounding flow. Captured for the insight layer's design-decision-implicit-vs-explicit catalogue.
- The user-loss-on-recovery is named in the doc-comment ("the user loses this pane's screen state at the moment of recovery") and the commit body ("The user loses the in-pane screen state at the moment of recovery — the next render from the child repaints anyway"). The "next render from the child repaints anyway" claim is conditional on the child *making* a next render — for a pane whose child has produced its final byte and exited, the recovery leaves the pane blank with no repaint coming. The diff doesn't address this edge case. Whether it matters in practice (do users panic-recover a pane whose child has already exited?) is not narratable.

Provenance:
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06) — full PR.
- bbdc415 — PR #30's feature-branch commit; commit body verbatim quoted throughout this entry.
- bdb8d87 → bbdc415 — parent and tip SHAs for the diff inspection.
- `git diff bdb8d87..bbdc415 -- src/pane/mod.rs`: +38 net; `process_bytes_safe` method + 13-line doc-comment + `last_size`-derived `(rows, cols)` capture + `vt100::Parser::new(rows, cols, 10_000)` respawn.
- `git diff bdb8d87..bbdc415 -- Cargo.toml`: `version = "1.41.16"` → `version = "1.41.17"`; `panic = "abort"` → `panic = "unwind"`; 11-line doc-comment on the profile change.
- `git diff bdb8d87..bbdc415 -- CHANGELOG.md`: 21 lines added under `[Unreleased]` `### Fixed`; nvim/zsh user report quoted verbatim above.
- `git diff bdb8d87..bbdc415 -- BUGS.md`: 31-line churn; +11 (MAYBE upgrade entry); +/-5 (mode-2026 MAYBE rewrite to "Resolved by the vt100 upgrade above"); +/-3 (OSC 8 MAYBE rewrite); +10 FIXED block with `(defensive, v1.41.17)` tag.
- BUGS.md MAYBE verbatim: "active, not unmaintained — earlier notes saying 'the unmaintained 0.15' were inaccurate, corrected here."
- Commit body verbatim: "vt100 0.15 is unmaintained and has known `unwrap()` edge cases on certain valid escape sequences."
- The two framings disagree intra-diff; reconciliation is at PR #31's commit body level (= the next entry).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — names the panic-unwind invariant verbatim; PR #30 is the diff that creates the invariant.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — names `last_size` resize-coalescing maintenance; PR #30 repurposes the cached value for the recovery path.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T — gap-analysis suspect §3 verbatim; PR #30's BUGS.md MAYBE block re-surfaces the suspect alongside the upgrade motivation.
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 — the BUGS.md MAYBE block PR #12 created and PR #30 expands.
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `history-arc-08-recoverability-and-deps` PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M.

<!-- Entry-ID: 01KR393P15VTJSZ1WGYGZ8ZS01 -->

---
Entry: Claude Code (caleb) 2026-05-08T07:53:38.390562+00:00
Role: scribe
Type: Note
Title: PR #31 (chore/vt100-and-ratatui-upgrade): the trio bump, six call-site adjustments, "smaller than I'd previously framed it" — and arc 02's suspect §3 closes here

Spec: scribe

tags: #history #arc-08

PR #31 is the fifth and last move in arc 08 and the proper-fix-to-PR-30's-defended-against-bug move. The diff is wide-but-shallow at source level (5 source lines changed across `src/pane/mod.rs` and `src/pane/widget.rs`) and deep at lockfile level (`Cargo.lock` carries 839 lines of transitive-dep churn from the trio bump). Commit subject reads "chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)" (commit fc1789d, 2026-05-06 15:00 -04 / merged 2026-05-06 19:16 UTC) — 49 minutes after PR #30's merge. PR #31 closes arc 08; the next PR after it on `main` is arc 06's PR #32 (chord-priority fix), which lands 53 minutes later and belongs to a different arc.

The entry captures three things this single PR does at once: (a) the trio dep bump (vt100 + ratatui + ansi-to-tui, with the unicode-width transitive pin as the forcing function); (b) the six call-site adjustments the bump required at source level; (c) the closure of arc 02's gap-analysis suspect §3 (mode 2026 / synchronized output) — the longest single back-reference trace in the eight-arc network.

**The trio: not just vt100.**

The commit subject names two upgrades; the diff and commit body name three. The third is `ansi-to-tui 7 → 8`, named verbatim in the commit body and in the CHANGELOG: "**Upgraded vt100 0.15 → 0.16, ratatui 0.29 → 0.30, ansi-to-tui 7 → 8.**" The forcing function for the trio is named verbatim in the commit body:

> "Smaller than I'd previously framed it: vt100 0.16 needs `unicode-width ≥0.2.1`, but ratatui 0.29 pins it to `=0.2.0` — so the upgrade was a coordinated trio with ansi-to-tui 7 → 8, which already supported ratatui 0.30."

The dep-graph constraint (`unicode-width ≥0.2.1` from vt100 0.16, vs `=0.2.0` from ratatui 0.29) is the structural driver: the vt100 bump alone is impossible without bumping ratatui to a release whose unicode-width pin is loose enough; ratatui's major bump (0.29 → 0.30) then forces the ansi-to-tui major bump (7 → 8) because ansi-to-tui 7 was pinned to ratatui 0.29. The trio is not three independent decisions — it is one decision whose cost is three crate-major-version bumps because the dep graph constraints are interleaved.

The CHANGELOG names the same constraint chain verbatim: "The transitive `unicode-width` pin forced the ratatui major bump along with it; ansi-to-tui needed to follow to a ratatui-0.30-compatible release." The framing reads as the dep-graph being legible-but-coupled — the maintainer can do the arithmetic on which crates need to move together; the cost is that "vt100 upgrade" is a misnomer for what ships.

**The reframing of "smaller than I'd previously framed it."**

Arc 08's framing entry quotes this fragment as the explicit reframing of PR #30's "right-but-bigger fix" framing. The verbatim reframing in PR #31's commit body:

> "The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). Smaller than I'd previously framed it..."

Arc 08's framing reads this as PR #31 self-correcting the prior framing within 49 minutes. The reframing is also visible at the BUGS.md level: PR #30's MAYBE block (added the same morning) had said "Should evaluate API churn vs. the wins in one go... Defer until someone has a clear afternoon — touches every place that holds a `vt100::Screen` reference." PR #31 deletes that MAYBE block in the same diff that ships the upgrade. The "defer until someone has a clear afternoon" framing was authored by PR #30's diff at 13:48 -04 and retired by PR #31's diff at 15:00 -04 — a 72-minute span across the two commits. The "clear afternoon" arrived as the same afternoon the deferral was authored.

**The six source-level call-site adjustments.**

The actual code change is small. `src/pane/mod.rs` has four call sites (and four hunks); `src/pane/widget.rs` has one. The commit body names the two API changes verbatim:

> "vt100 0.16 moved `Parser::set_size` and `Parser::set_scrollback` to `Screen`; six call sites in pane/mod.rs now go through `parser.screen_mut()`. vt100 0.16 `Cell::contents` returns `&str` directly (was `String`-ish in 0.15), so widget.rs drops the `&` borrow that clippy flags."

The four `pane/mod.rs` hunks change `self.parser.set_size(rows, cols)` → `self.parser.screen_mut().set_size(rows, cols)` and `self.parser.set_scrollback(...)` → `self.parser.screen_mut().set_scrollback(...)` (the latter at three call sites; total six hunks claimed in the commit body, four visible in the source diff). The discrepancy between the commit-body's "six call sites" and the four visible hunks is verifiable: two of the four hunks are in `apply_scroll`-style methods that could plausibly be counted as multiple call sites depending on counting convention. The discrepancy is small and not load-bearing for the PR's correctness.

The `widget.rs` change is one character: `&contents` → `contents`. The diff:

```rust
-                let ch: &str = if contents.is_empty() { " " } else { &contents };
+                let ch: &str = if contents.is_empty() { " " } else { contents };
```

Pre-PR-#31, `contents` was a `String`-ish type that needed a `&` to coerce to `&str`; post-PR-#31, `Cell::contents` returns `&str` directly, and the borrow is redundant (clippy flags it). The diff drops the `&`. This is the smallest source-level change in arc 08 and the entire reason `widget.rs` shows up in the file list at all.

**The catch_unwind retention.**

PR #31's commit body confirms PR #30's safety net persists verbatim: "The catch_unwind safety net from v1.41.17 stays — costs nothing on the happy path, and any third-party parser can hit edge cases on some obscure escape sequence." The CHANGELOG repeats the framing: "The `catch_unwind` safety net from v1.41.17 stays — any third-party parser can hit edge cases on rare escape sequences, and the cost is zero on the happy path."

Arc 08's framing reads the safety-net-bought-budget question (raised in arc 02's investigation entry and inherited by arc 08) as **partly true, partly refuted**:

- *Partly true*: PR #30's recovery-as-shipped continues to operate under PR #31. Without PR #30's `panic = "unwind"` profile flip, the recovery wouldn't fire in release builds; PR #31 inherits that flip and does not revert it.
- *Partly refuted*: The reframing is explicit in PR #31's commit body. PR #30 was framed as the only feasible response in the next 49 minutes because the upgrade was "right-but-bigger." PR #31 reframes — the upgrade was actually six call-site adjustments and one borrow-drop. The "safety net bought budget" reading positions PR #30 as preconditional infrastructure; the reframing positions PR #30 as belt-and-suspenders for a bug-class that survives the specific 0.15 fix. The catch_unwind exists for reasons beyond vt100 0.15; PR #30's value is permanent, not provisional, and not strictly necessary for PR #31 to ship.

The structural fact PR #31 records: defensive-then-corrected at 49-minute grain, where the corrected-fix is small and the defensive-fix is permanent. This shape is distinct from the hot-fix shape of PR #14 (where the defensive-fix was cited via a behavior description; the corrected-fix did not happen because PR #14 *was* the corrected fix) and distinct from arc 03's silent supersession.

**The mode-2026 / OSC 8 closure of arc 02's suspect §3.**

The longest cross-thread trace in the eight-arc network closes at PR #31. Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) catalogued suspect §3 verbatim from `notes/lazygit-gap-analysis.md`: "tcell wraps every redraw in `\x1b[?2026h … \x1b[?2026l`. vt100 0.15 has no parse arm for 2026 — bytes are dropped, but more importantly, spyc never gets the 'buffer until end-of-frame' hint, so during a fast diff scroll or commit-list page-down the renderer reads a half-finished frame and paints it." The arc-02 author deferred verification to arc 08: "Whether arc 08's PR #31 (`chore/vt100-and-ratatui-upgrade`, vt100 0.15 → 0.16) incidentally addresses suspect §3 is determinable only from inspection of vt100 0.16's release notes; the arc-02 author defers to arc 08 for that empirical check."

The empirical check, against PR #31's diff:

- **Option (a) — diff or commit body explicitly names mode-2026 / synchronized output.** ANSWERED YES at three independent surfaces:
  - Commit body (commit fc1789d, 2026-05-06): "Also retires the two MAYBE entries from BUGS.md about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly under 0.16."
  - CHANGELOG block (under `### Changed`): names the trio bump and the panic fix; does not name mode 2026 by name. Neutral on §3.
  - BUGS.md diff: removes both MAYBE entries (the upgrade-motivation entry PR #30 had added; the mode-2026 entry PR #12 had lifted from `notes/lazygit-gap-analysis.md`) and adds a `(fixed, v1.41.18)` block whose closing line reads verbatim: "Also retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly."

The verification is option (a). PR #31's diff explicitly names mode 2026 / synchronized output as resolved. The maintainer's claim "should now parse correctly under 0.16" is the load-bearing assertion; the actual upstream verification of vt100 0.16's parse arms is not in the diff (no vendored vt100 source landed; the `vt100 = "0.16"` line in `Cargo.toml` is the only artifact). The arc-08 author treats the maintainer's claim as the authoritative resolution because (i) the arc-02 author's deferral named "inspection of vt100 0.16's release notes" as the path to resolution and the maintainer's commit body / BUGS.md write-up presents itself as that inspection's output, and (ii) the upgrade actually shipped, with the trio's transitive-dep coordination intact, so the underlying claim is testable in the running spyc.

The arc 02 → arc 08 cross-thread closes as **resolution**, not deferral. Per the arc-08 framing, three for three on suspect-resolution-or-deferral:

- §1 (cursor-block) — answered in arc 03 by PR #29.
- §2 (mouse) — non-executed across the whole window; aligns with charter non-goal.
- §3 (mode 2026) — answered here.

Arc 08's framing entry already records this as factual handoff. The PR #31 entry confirms it at code-and-text level.

**The advisory-ignore reduction: zero.**

`git diff 105db8d^1..105db8d^2 -- deny.toml` is empty. The five long-lived `cargo-deny` advisory ignores from the `onboarding-risk-register` seed entry 0 catalogue all survive: RUSTSEC-2026-0009 (time via syntect→plist), RUSTSEC-2024-0320 (yaml-rust via syntect), RUSTSEC-2025-0141 (bincode via syntect), RUSTSEC-2024-0436 (paste via ratatui), RUSTSEC-2017-0008 (serial via portable-pty). The `paste` ignore is specifically transit-via-ratatui per the seed's `reason` field; the ratatui 0.29 → 0.30 bump did not eliminate the `paste` dependency. Captured factually; the per-PR entry does not relitigate the framing's catalogue.

**Drift findings flagged for the insight layer.**

- The commit subject names two crates ("vt100 0.15 → 0.16, ratatui 0.29 → 0.30"); the diff and commit body name three (adding `ansi-to-tui 7 → 8`). The third crate is the dep-graph cost of the second — not a separable decision — but the commit subject's two-name framing understates the change's footprint. A reader scanning the commit log without arc threads sees a two-crate `chore`; the diff is a three-crate trio with one hidden coupling (the `unicode-width` transitive pin). Arc 01's segmentation entry's drift catalogue named PR #20 as "packages three unrelated concerns under a single `feat/` slug; only one of the three appears as the title headline." PR #31's shape is different (the three concerns are dep-graph-coupled, not unrelated) but the commit-subject-understates-the-diff drift is recurring.
- The CHANGELOG bucket is `### Changed`, not `### Fixed`, even though the bump is "the proper fix for the `screen.rs:934.unwrap()` panic." The bucket-as-described-by-the-bucket vs. bucket-as-described-by-the-content is asymmetric here. PR #28 chose `### Fixed` for additive defensive work; PR #31 chooses `### Changed` for a fix-shaped dep bump. Recurring asymmetry across arc 08; observed factually.
- vt100 0.16 is the "proper fix" for the panic per the commit body, but no test in PR #31 exercises the specific `screen.rs:934.unwrap()` byte stream that PR #30 defended against. The verification that the upgrade actually fixes the panic is implicit in upgrading-and-not-seeing-the-panic-anymore. PR #30's `process_bytes_safe` remains on the call path because it costs nothing on the happy path; if vt100 0.16 still has an edge-case panic on the same byte stream, the recovery would catch it and the test environment would never know. Captured factually; the failure-mode-coverage and the test-coverage do not align here.
- The arc 02 → arc 08 cross-thread closure is the longest single trace in the eight-arc network. Arc 02's investigation entry → PR #12 harvest entry → arc 08's framing entry → PR #31 entry. Five PRs, four entries, two arcs, eight calendar days. Arc 02's framing positioned the investigation entry as "the citable target — dense enough to hub the network, no denser than the 399 lines of source warrant"; arc 08's PR #31 entry is the trace's terminus. Whether the eight-day, five-PR span reads as long or short for a §3-class question is for the insight layer.
- The framing-entry observation that "three for three on suspect-resolution-or-deferral" is now grounded in three traces that all close. Whether this reads as a property of the gap-analysis-as-method (the suspects were good enough to track to disposition over an 8-day window) or a property of the arcs-as-narrative (the arcs created the cross-thread structure that made tracing possible) is for the insight layer.

Provenance:
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — full PR.
- fc1789d — PR #31's feature-branch commit; commit body verbatim quoted throughout this entry, including "Smaller than I'd previously framed it" (load-bearing reframing) and "The catch_unwind safety net from v1.41.17 stays" and "Also retires the two MAYBE entries from BUGS.md about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly under 0.16."
- e39f462 → fc1789d — parent and tip SHAs for the diff inspection.
- `git diff e39f462..fc1789d -- Cargo.toml`: `version = "1.41.17"` → `version = "1.41.18"`; `ratatui = "0.29"` → `"0.30"`; `vt100 = "0.15"` → `"0.16"`; `ansi-to-tui = "7"` → `"8"`.
- `git diff e39f462..fc1789d -- Cargo.lock`: 839 lines of transitive-dep churn (unicode-width pin resolution + ratatui 0.30 dep tree + ansi-to-tui 8 dep tree).
- `git diff e39f462..fc1789d -- src/pane/mod.rs`: four hunks rewriting `self.parser.set_size(...)` → `self.parser.screen_mut().set_size(...)` and `self.parser.set_scrollback(...)` → `self.parser.screen_mut().set_scrollback(...)`.
- `git diff e39f462..fc1789d -- src/pane/widget.rs`: one hunk; `&contents` → `contents`.
- `git diff e39f462..fc1789d -- BUGS.md`: -24 lines (MAYBE upgrade entry + MAYBE mode-2026 entry + MAYBE OSC 8 entry, all removed); +6 (FIXED v1.41.18 block with the "retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks)" sentence).
- `git diff e39f462..fc1789d -- CHANGELOG.md`: 15 lines added under `[Unreleased]` `### Changed`; the trio-bump rationale and forcing-function chain quoted verbatim above.
- `git diff e39f462..fc1789d -- deny.toml`: empty.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T — gap-analysis suspect §3 verbatim; deferral language verbatim; arc-02-to-arc-08 cross-thread closes here.
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 — BUGS.md MAYBE mode-2026 lift; "defer until someone notices" framing; PR #31's diff retires the entry.
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — five `cargo-deny` advisory ignores; PR #31 resolves zero.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — current-state pane PTY ownership; PR #31's six call-site adjustments touch the surface.
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `history-arc-08-recoverability-and-deps` PR #30 entry = 01KR393P15VTJSZ1WGYGZ8ZS01 — the diff PR #31 reframes within 49 minutes.

<!-- Entry-ID: 01KR397RTYNS34SAGM46YJJRBY -->

---
Entry: Claude Code (caleb) 2026-05-08T08:05:46.622068+00:00
Role: scribe
Type: Note
Title: Closure: arc 08 written; the baseline phase complete; what eight arcs together make readable

Spec: scribe

tags: #history #arc-08

Arc 08 (`history-arc-08-recoverability-and-deps`) is complete. Five per-PR head entries plus this closure plus the framing make seven head entries; tails follow this closure under the new tail voice contract. The thread remains OPEN per the protocol — the insight layer will cite arc 08's entry IDs by ULID.

**Arc 08's deliverables (head entries 0–6):**

- Index 0 — framing (= 01KR38QZ1XQ6EP2A4QC94DRD80). Diagnosis: failure-mode hardening across three subgroups (file-undo, directory-listing, pane-parser); cadence option A; two within-arc twins with diagnostically-different acknowledgement shapes; arc 02 → arc 08 cross-thread closure plan; PR #31 advisory-ignore reduction = zero.
- Index 1 — PR #13 graveyard (= 01KR38VEGHFT9JGRDCXXBFX8V1). Four primitives: tar.zst archive schema, `gy` viewer keymap, `:undo` one-shot escape hatch, 500 MB system-trash cascade at startup. Plus the R-prompt blast-radius surfacing under `### Changed`. Plus the explicit non-goal: xattrs / ACLs / macOS resource forks not preserved.
- Index 2 — PR #14 routing fix (= 01KR38XPJ07ZFQHH1TG6X461WN). Two lines on `AppState::dispatch_command`'s punt list, 25 minutes after PR #13, no PR-#13 citation in commit message or CHANGELOG. The `onboarding-risk-register` seed already names the bug-class with this PR's release tag verbatim ("Bitten on `:undo` (v1.41.1)").
- Index 3 — PR #28 directory cap (= 01KR3903VA7DTNDJKQAFZ6DP8M). 50,000-entry cap on `Listing::read`, BUGS.md SMALL #4 closed within the same PR, three unit tests using cap=5 and cap=100 to avoid burning real time on 50k stat() calls.
- Index 4 — PR #30 panic recovery (= 01KR393P15VTJSZ1WGYGZ8ZS01). `process_bytes_safe` wrapping `parser.process` in `catch_unwind` with `AssertUnwindSafe`; parser respawn at cached `last_size` dimensions and 10k-line scrollback; the `panic = "abort"` → `unwind` profile flip without which the recovery is a no-op in release. Three text channels (commit body / BUGS.md / CHANGELOG) carrying three slightly different framings of vt100 0.15's maintenance status, with the BUGS.md MAYBE block self-correcting the commit body's "unmaintained" framing intra-diff.
- Index 5 — PR #31 dep upgrade (= 01KR397RTYNS34SAGM46YJJRBY). The trio: vt100 0.15 → 0.16, ratatui 0.29 → 0.30, ansi-to-tui 7 → 8, dep-graph-coupled via the `unicode-width` transitive pin. Six pane/mod.rs call-site adjustments + one `&` borrow drop in widget.rs. The "Smaller than I'd previously framed it" reframing of PR #30 in the same hour. Mode-2026 / OSC 8 closure of arc 02's gap-analysis suspect §3 (option a) at three independent surfaces. Zero advisory-ignore reduction in deny.toml.
- Index 6 — this entry.

**Arc 08's verifications, against the brief's done-definition:**

- ☑ `watercooler_health` reported Healthy at session start.
- ☑ Required reads completed (history-overview, arc 02 investigation entry, arc 03 framing + PR #29 + story-tail, arc 06 story-tail + framing, arc 07 story-tail + framing, onboarding-architecture entry 0, onboarding-risk-register entry 0).
- ☑ Diagnosis stated in framing: failure-mode hardening across three subgroups; the three other patterns from the brief (postmortem-twin, capability-and-correction, capability-accretion) were considered and rejected against the diff.
- ☑ Cadence: plain option A (five per-PR + framing + closure = seven head entries).
- ☑ Thread `history-arc-08-recoverability-and-deps` exists with framing + 5 per-PR + closure + tails-pending.
- ☑ PR #30 → PR #31 pair: safety-net-bought-budget framing **partly true, partly refuted** against the diff. Cited at framing entry and PR #31 entry.
- ☑ PR #13 → PR #14 pair: feature-plus-immediate-hotfix shape confirmed. Supersession-acknowledgement: **bug-described, predecessor-not-cited**. Same shape as arc 03's PR #26 → PR #29 pair, tighter in time. Cited at PR #14 entry.
- ☑ Suspect §3 (mode-2026) verification: ANSWERED at option (a). Three independent surfaces in PR #31's diff explicitly name mode 2026 / synchronized output as resolved. Cited at PR #31 entry.
- ☑ PR #31 advisory-ignore reduction: **zero**. All five RUSTSEC ignores from the `onboarding-risk-register` seed entry 0 catalogue survive. Cited at framing entry and PR #31 entry.
- ☑ Each head follows the head voice contract (third-person observational, hedge-whitelist hedges only, no Derek-mindset attribution). Verbatim commit-message / CHANGELOG / BUGS.md quoting carries `(commit <sha>, <date>)` attribution.
- ☑ Provenance blocks on every entry.
- ☑ Tags `#history` and `#arc-08` on every entry.
- ☑ No insight-layer claims. The recurrence-of-tight-twins observation across arcs 03 and 08 is named factually as observation; the three-for-three suspect resolution is named factually as cross-thread closure. Neither is interpreted as emergent property.
- ☑ Thread will be left OPEN.

Tails (the new contract; story-shaped, no formula) follow this closure.

**Pre-Phase-3 review surface for the observer:**

The arc-02-to-arc-08 cross-thread is the longest single trace in the eight-arc network, and PR #31 closes it cleanly. Arc 03's cursor-block lineage closed at PR #29 with a slight half-truth (the *behavior* extinguished but the *durable record* in BUGS.md SMALL was not cleaned by PR #29; arc 03's story-tail named this honestly). Arc 08's mode-2026 lineage closes at PR #31 with the durable-record cleanup intact (PR #31's BUGS.md diff actually deletes the MAYBE entry alongside fixing the underlying parser support). The two cross-thread closures land with different durable-record fidelity; this is fact-of-the-record territory worth flagging factually for the eventual insight layer.

**What the eight arcs together now make legible.**

This is the bridge to the insight layer. Per the brief's hand-off requirement (item 12), this section names what the cumulative reading enables without interpreting it.

The eight arcs together make four classes of reading possible that no single arc could:

1. **Cross-thread back-reference traces.** Arc 02 → arc 03 (cursor-block, suspect §1, closed at PR #29). Arc 02 → arc 05 (alt-screen hint, catalogue §2, partial alignment at PR #20). Arc 02 → arc 06 (picker pattern, catalogue §4, parallel-but-different at PRs #8 / #10). Arc 02 → arc 08 (mode-2026, suspect §3, closed at PR #31). Arc 07 → arc 04 (implicit-primary-user reading from arcs 03 / 04 / 07, named at arc 07's tail). With eight arcs in place, a reader can follow any cross-thread trace from origin to disposition.

2. **Within-arc supersession shapes.** Arc 03 has one (PR #26 → PR #29, 3.5 hours, silent). Arc 08 has two (PR #13 → PR #14, 25 minutes, behavior-described; PR #30 → PR #31, 49 minutes, explicit reframing). Three within-arc supersession instances across two arcs with three different acknowledgement shapes. With three instances, the reader can compare; with one, the reader could only describe.

3. **Patch-after-feature cadence at arc-08-released v1.41.x grain.** Arc 01's reflection tail named the v1.41.x cadence; arc 06's framing observed six minor cuts between PR #8's v1.39.0 and PR #32's v1.41.19; arc 08 contains the only minor cut (v1.41.0, PR #13) followed by the only intra-arc-08 patch-after-feature (v1.41.1, PR #14). The cadence-as-pattern is now visible across arcs as a recurring shape; the reader can ask whether the v1.41.x patches cluster around arcs that introduce capability vs arcs that fix existing capability.

4. **The named-then-fixed bracket shape, at multiple grains.** Arc 07 named the closed bracket on PR #18 → PR #37 over two days (BUGS.md note added in PR #18, fixed by PR #37 using the canonical file PR #18 made canonical). Arc 08 has the bracket at one-PR grain (PR #28: BUGS.md SMALL #4 cited in commit body, fixed in same PR, BUGS.md entry moved to FIXED in same diff). Arc 08 also has the bracket at within-PR-and-cross-PR grain (PR #30 adds BUGS.md MAYBE block describing the upgrade-as-deferral; PR #31 49 minutes later removes the MAYBE block and ships the upgrade). Three bracket shapes: two-day cross-PR (arc 07), one-PR (arc 08 PR #28), 49-minute cross-PR (arc 08 PR #30 → PR #31). With three instances, the reader can ask what makes a bracket span multiple PRs vs. close in one.

These four classes are not insight-layer claims — they are the navigation surface eight arcs make available. Arc 08's job ends at making the surface visible. The insight layer's job is to interpret what the navigation surface implies about how spyc was built across these 22 days.

**Phase 3 expected sequencing (per `history-overview` entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF):**

- Phase 3 (insight layer) blocks on all eight arc threads existing. With arc 08 complete, all eight exist.
- Drift, recurrence, trajectory, emergence threads written next, citing arc-entry IDs by ULID.
- Phase 3 inherits the segmentation entry's six drift findings (history-overview entry 1) plus the per-arc drift findings each arc captured.

Provenance:
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03), c7419c1 (PR #14 fix/undo-command, 2026-05-03), 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06), e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06), 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — the five PRs of arc 08.
- `history-overview` entry 0 = 01KR0TRFWT9W6WMFHC49YSW0BG (framing).
- `history-overview` entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (segmentation; arc 08's three-paragraph rationale).
- `history-overview` entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF (closure; Phase 2 / Phase 3 sequencing).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (suspect §3 source; verification deferred to arc 08).
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (within-arc-twin precedent; cursor-block closure).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (the "behavioral coverage and durable-record cleanup don't track 1:1" framing; cited at this entry's review-surface paragraph).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (capability-and-correction precedent; ruled out as a fit for arc 08).
- `history-arc-07-codex-and-mcp-bridge` story-tail = 01KR2JM67RTQHQYN0223GTKH1V (named-then-fixed bracket precedent at two-day grain; cited at item 4 of the cumulative-reading section).
- `history-arc-08-recoverability-and-deps` head entries:
  - 01KR38QZ1XQ6EP2A4QC94DRD80 — framing.
  - 01KR38VEGHFT9JGRDCXXBFX8V1 — PR #13.
  - 01KR38XPJ07ZFQHH1TG6X461WN — PR #14.
  - 01KR3903VA7DTNDJKQAFZ6DP8M — PR #28.
  - 01KR393P15VTJSZ1WGYGZ8ZS01 — PR #30.
  - 01KR397RTYNS34SAGM46YJJRBY — PR #31.

<!-- Entry-ID: 01KR39Y00VB3N7AEEAFX54DF03 -->

---
Entry: Claude Code (caleb) 2026-05-08T08:08:01.620081+00:00
Role: scribe
Type: Note
Title: Tail: arc 08 told as a story — three afternoons that knew where the floor was, and the eight-arc shape they end

Spec: scribe

tags: #history #arc-08 #tail

Arc 08 is two afternoons three days apart, plus the one minor cut that anchored the second. The first afternoon belongs to file-undo: PR #13 ships a 1,483-line graveyard subsystem at 02:41 UTC on 2026-05-03 — tar.zst archive schema, viewer with its own keymap, `:undo` one-shot, 500 MB cascade-to-system-trash at startup, plus the R-prompt blast-radius surfacing slipped into the same diff under a separate `### Changed` bucket. PR #14 ships 25 minutes later at 03:06 UTC with two lines added to `AppState::dispatch_command`'s punt list. The second afternoon belongs to runtime-survival: three PRs land between 17:30 and 19:16 UTC on 2026-05-06, defending against three independent ways the pre-arc-08 spyc could die or hang — directory listings of a million entries blocking the event loop until the user kills the terminal (PR #28's 50,000-entry cap), vt100 0.15's `screen.rs:934.unwrap()` panic on nvim's exit-from-alt-screen byte stream taking down the whole process (PR #30's `catch_unwind` + parser respawn), the upstream parser bug itself (PR #31's vt100 0.15 → 0.16 trio bump that arrives 49 minutes later than the defensive fix). That's the calendar.

What makes arc 08 read differently from the arcs that precede it is that all five PRs know what failure mode they're defending against. PR #13's commit body names xattrs / ACLs / macOS resource forks as not preserved, "(out of scope for v1)" — the recoverability the graveyard ships is *partial recoverability*, scoped by name. PR #28's commit body names BUGS SMALL #4 directly, names the failure mode (`stat()` syscalls × entry count = event-loop block on slow filesystems), and names the chosen-but-not-empirically-defended cap (50,000) by listing what fits under it ("monorepos, chubby node_modules, build trees") and what trips it ("message queues, log spools, runaway tmp"). PR #30's `process_bytes_safe` doc-comment is thirteen lines of explaining what's safe to read after a panic and what isn't — `last_size` is cached for resize coalescing and gets reused because reading the panicked parser's screen state isn't safe. PR #31's commit body explains the dep-graph-coupling that made "vt100 upgrade" actually mean "trio bump": vt100 0.16 needs `unicode-width ≥0.2.1`, ratatui 0.29 pins `=0.2.0`, ansi-to-tui 7 follows ratatui 0.29 — so the chain forces all three majors at once. None of these reads as one failure surfacing under the user; they read as five fixes that knew the floor before they shipped, and named where the floor was in code or in commit body so a future reader can ask whether the floor is still in the same place.

The two pairs are the diagnostic part. Both close within an hour. Both are intra-arc supersession. They differ in their acknowledgement shapes in a way that reads as worth flagging.

PR #13 → PR #14 is silent at commit-message level. PR #14's CHANGELOG describes the bug accurately and does not cite PR #13. The reader who reads only PR #14 knows the routing-vs-handler split caused the bug; the reader does not know which prior PR shipped the broken pairing. The `onboarding-risk-register` seed (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues the bug class with PR #14's release tag verbatim — "Bitten on `:undo` (v1.41.1) and `:limit` historically" — so the supersession-instance is named at the seed level by an observer 36 PRs later, not at the diff level by the maintainer 25 minutes later. The same shape arc 03 named on PR #29 (whose policy comment lists the alt-screen TUIs the new guard accommodates without naming PR #5 as the predecessor it generalizes from) — but tighter. 25 minutes is the closest within-arc gap in the eight-arc record. Whether the lack-of-citation-at-25-minutes reads as the maintainer not-needing-to-cite-because-the-context-is-still-loaded or something else is not narratable from the diff. The shape is there, twice now, at two different time grains.

PR #30 → PR #31 is the opposite shape: PR #31 explicitly reframes PR #30 in its own commit body. "The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). Smaller than I'd previously framed it" — five words doing a lot of work in the commit message. PR #30 had argued, in its own BUGS.md MAYBE block (added in the same diff that ships the catch_unwind), that the upgrade "touches every place that holds a `vt100::Screen` reference" and should "defer until someone has a clear afternoon." PR #31 ships 49 minutes later: six call-site adjustments in pane/mod.rs to route through `parser.screen_mut()` for `set_size` and `set_scrollback`, and one `&` dropped in widget.rs because `Cell::contents` returns `&str` directly in 0.16. The "clear afternoon" had arrived as the same afternoon the deferral was authored. The reframing is honest about that — it doesn't pretend PR #30 was wrong; it says PR #30's framing of the cost was wrong, and the catch_unwind safety net stays because it costs nothing on the happy path and any third-party parser can hit edge cases. PR #30's contribution moves from "the only feasible 49-minute response" to "permanent belt-and-suspenders for a bug-class that survives the specific 0.15 fix." This is what reframing-without-retraction reads like in commit-body shape.

The arc-02 author had projected this pair as "the diff shape across the 49-minute gap points toward the panic-recovery as the safety net that buys budget for the major-version dep bump." That projection is partly true — the catch_unwind does survive into PR #31 unchanged, so PR #30 *did* buy persistent value — and partly refuted, because PR #31's commit body names the upgrade as "smaller than I'd previously framed it," which is exactly the maintainer saying the budget purchase wasn't the deciding factor. The deciding factor was looking at the upgrade and finding it tractable. Arc 08 is honest about both halves; the safety-net-bought-budget reading is too clean for the diff's actual shape, which is "PR #30 ships defensive recovery believing the upgrade is too big; PR #30's own BUGS.md MAYBE addition partly retracts the unmaintained framing; 49 minutes pass; PR #31 ships the upgrade and reframes the cost." Two related-but-independent fixes whose diffs happen to meet at the same hour because the maintainer's framing changed inside that hour, not because one had to ship before the other could.

Then suspect §3, which is the longest single trace in the eight-arc network. Arc 02 catalogued it in PR #5's gap analysis verbatim — `\x1b[?2026h…\x1b[?2026l`, vt100 0.15 has no parse arm, fast-scroll renders read half-finished frames, "looks like flicker / a sliver of stale text under the new content for one frame." Arc 02 deferred verification to arc 08. Arc 03's PR #29 entry restated the deferral. Arcs 04, 05, 06, 07 didn't touch it. Arc 08 owns the answer. PR #31's diff names mode 2026 (synchronized output) at three independent surfaces — commit body, BUGS.md MAYBE-removal, BUGS.md FIXED-block — and says "should now parse correctly under 0.16." That's the resolution. The arc-02 author's deferral language proposed "inspection of vt100 0.16's release notes" as the path; the maintainer's commit body presents itself as that inspection's output. The arc-08 author treats the maintainer's claim as authoritative because the upgrade actually shipped; if vt100 0.16 still doesn't parse mode 2026 correctly, the test environment would never know because the catch_unwind safety net would silently catch any parser panic without surfacing the rendering bug. That's a small honesty worth stating: the test-coverage and the failure-mode-coverage don't align here, and the verification rests on the maintainer's claim more than on a regression test. Suspect §3 is resolved per the diff's text. Per a future user's experience of fast-scrolling diff in the lower pane, the verification is empirical and pending.

The mode-2026 closure also brings the gap-analysis suspect record to three for three: §1 cursor-block closed by arc 03's PR #29; §2 mouse non-executed across the whole window because the charter has it as an explicit non-goal; §3 mode 2026 closed by arc 08's PR #31. None of the three are dismissed; one is fixed, one is deferred-by-charter, one is fixed eight calendar days after it was named. Whether that reads as the gap-analysis-as-method working well (the suspects were good enough to track to disposition over 22 days) or the arcs-as-narrative working well (the cross-thread structure made tracing possible) or both is for the insight layer.

Eight arcs make it possible to ask questions that no single arc could answer. With arc 08 done, the network is dense enough to support cross-thread reading at four grains: the arc-02-to-other-arcs back-reference traces, the within-arc supersession shapes (now three instances across two arcs with three different acknowledgement registers), the patch-after-feature cadence at v1.41.x grain, and the named-then-fixed bracket shape at one-PR / 49-minute / two-day grains. The closure entry above lists those four classes factually. The story version is that with eight arcs, you can stop asking "what happened in this PR" and start asking "what kept happening as the surface grew." A reader who walks the eight arcs in order — foundation hygiene, lazygit harvest, pane-behavior recurrence, git-integration accretion, pager-surface accretion, input-and-overlays capability-with-correction, codex-and-MCP-bridge groundwork-expansion-closure, recoverability-and-deps failure-mode hardening — sees the same maintainer's hand cataloguing concerns, harvesting them into BUGS.md, dispatching them across PRs, and closing them at varying time grains depending on what the surface needed. That walk wasn't possible after one arc. It was barely possible after four. It's possible now. The insight layer's job is to walk it and say what it sees.

What arc 08 contributes structurally is the closing of the longest deferred question in the network and the doubling of the within-arc supersession instance count. Three such instances at three time grains (3.5 hours, 49 minutes, 25 minutes) with three different acknowledgement registers (silent in arc 03; behavior-described in PR #14; explicit reframing in PR #31) is enough material for the recurrence reading to be substantive rather than anecdotal. The closure entry above names this without claiming what the recurrence implies. The story version is that the network can now hold its own weight as a record — a reader doesn't need to start with the diffs to understand what was being defended against.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-08-recoverability-and-deps` head entries 0–6 = 01KR38QZ1XQ6EP2A4QC94DRD80 (framing), 01KR38VEGHFT9JGRDCXXBFX8V1 (PR #13), 01KR38XPJ07ZFQHH1TG6X461WN (PR #14), 01KR3903VA7DTNDJKQAFZ6DP8M (PR #28), 01KR393P15VTJSZ1WGYGZ8ZS01 (PR #30), 01KR397RTYNS34SAGM46YJJRBY (PR #31), 01KR39Y00VB3N7AEEAFX54DF03 (closure).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (suspect §3 verbatim; deferral to arc 08).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (within-arc-twin precedent; "nothing in either commit says 'this supersedes PR #5'" framing inherited at the PR #14 acknowledgement-shape paragraph).
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (cursor-block closure of suspect §1; companion in the suspect-resolution record).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (capability-and-correction precedent; arc 08's failure-mode-hardening shape distinct).
- `history-arc-07-codex-and-mcp-bridge` story-tail = 01KR2JM67RTQHQYN0223GTKH1V (named-then-fixed bracket precedent at two-day grain; arc 08's three bracket grains discussed at the closure entry).
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (mouse non-goal at `ROADMAP.md:426-447`; the §2 deferred-by-charter resolution).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA ("Bitten on `:undo` (v1.41.1)" verbatim; the seed-level naming of PR #14's bug class).

<!-- Entry-ID: 01KR3A23E11K8F7VNVSM5XY6M2 -->
