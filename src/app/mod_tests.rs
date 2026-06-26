//! Unit tests relocated from app/mod.rs (800-LoC campaign): registry/quit
//! guards, refresh + post-chord timing, layout, history buckets, and the
//! pure text/format helpers. Nested-module imports use `super::super::` to
//! reach the `app` module from this child.

#[cfg(test)]
mod guard_tests {
    /// Anti-monolith guardrail. `app/mod.rs` was a ~12k-line monolith;
    /// REFACTOR_PLAN Phases 1–2 decomposed it into focused `src/app/`
    /// modules. This test fails if `mod.rs` creeps back toward that —
    /// new render/key/command/action/session logic belongs in the
    /// matching child module (or a new one), not appended here.
    ///
    /// If you hit this: extract a module, don't bump the ceiling. The
    /// ceiling sits well below the old monolith and comfortably above
    /// what legitimately stays in `mod.rs` (the `App`/`Runtime`/
    /// `ViewState`/`Message` type defs + small glue — `run` lives in
    /// run.rs, `App::new` in bootstrap.rs), so tripping it means
    /// something that should be its own module landed here instead.
    /// See AGENTS.md → "Keep `src/app/` modularized".
    ///
    /// Ratcheted 4000 → 1500 after the impl-extraction sweep left mod.rs
    /// at ~1076 lines (a guard that allows tripling isn't guarding).
    #[test]
    fn mod_rs_stays_decomposed() {
        const CEILING: usize = 1_500;
        let src = include_str!("mod.rs");
        let lines = src.lines().count();
        assert!(
            lines <= CEILING,
            "src/app/mod.rs is {lines} lines, over the {CEILING}-line \
             anti-monolith ceiling. Extract logic into a src/app/ child \
             module instead of growing mod.rs (see AGENTS.md). Don't just \
             raise CEILING."
        );
    }

    /// `state.left`/`right` address a SPECIFIC column (render draws both
    /// explicitly; the fs-watch dedup-keys per column). "Where the user is
    /// working" — a spawn cwd, a restore target, the dir an op acts on — must
    /// use `cur()`/`cur_mut()` so a focused second commander is honored. The
    /// June-2026 vsplit review found six spawn/restore sites stranded on
    /// `state.left.listing.dir` (`:;`, the bare pane spawn, pager-edit,
    /// graveyard restore, …). A new read outside the allowlist is that smell —
    /// route it through `cur()`, or (if it's genuinely the left column) add the
    /// file to ALLOW with a why. See AGENTS.md → per-column scoping.
    ///
    /// Allowlisted: the left-column fs-watch dedup keys (`run.rs`; the right
    /// column has its own `watched_listing_right`), the startup `initial_cwd`
    /// (`bootstrap.rs`, pre-split), and the status-bar header (`chrome.rs`,
    /// deliberately anchored to the primary column).
    #[test]
    fn state_left_listing_dir_uses_are_allowlisted() {
        const ALLOW: &[&str] = &["run.rs", "bootstrap.rs", "chrome.rs"];
        // Split so this guard's own source can't match the needle.
        let needle = format!("{}{}", "state.left", ".listing.dir");
        let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut offenders = Vec::new();
        scan_rs(&app, &mut |path, src| {
            // Production portion only — a `#[cfg(test)]` block may legitimately
            // poke `state.left` to set up a fixture.
            let production = src.split("#[cfg(test)]").next().unwrap_or("");
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if production.contains(&needle) && !ALLOW.contains(&name) {
                offenders.push(name.to_string());
            }
        });
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "`state.left.listing.dir` read outside the allowlist in: {offenders:?}. \
             Use `cur().listing.dir` (the focused column) for spawn/restore cwd so a \
             second commander is honored; if it's genuinely the left column, add the \
             file to ALLOW with a why. See AGENTS.md → per-column scoping."
        );
    }

    /// Every top-level `src/app/<feature>.rs` module must be named in the
    /// AGENTS.md module index — the June-2026 review found `worktree_clean.rs`,
    /// `activity.rs`, and `git_view_session.rs` silently absent, so the "map of
    /// the codebase" had holes. A new feature module → add its bullet in the
    /// same PR. Test-only files (`mod_tests.rs` / `test_harness.rs`) are exempt.
    #[test]
    fn every_app_module_is_in_the_agents_index() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let agents = std::fs::read_to_string(root.join("AGENTS.md")).expect("read AGENTS.md");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(root.join("src/app")).expect("read src/app") {
            let path = entry.expect("dir entry").path();
            // Top-level feature modules only; subdir modules (render/, state/, …)
            // are documented as groups.
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "mod.rs" | "mod_tests.rs" | "test_harness.rs") {
                continue;
            }
            if !agents.contains(name) {
                missing.push(name.to_string());
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "src/app/ feature modules missing from the AGENTS.md module index: {missing:?}. \
             Add a bullet for each (AGENTS.md → \"Keep docs in sync\") — a module absent \
             from the map is the worktree_clean.rs gap the June-2026 review caught."
        );
    }

    /// Load-bearing "trap" anchors are a sparse, machine-checked
    /// *discoverability* signal — NOT a comment-style change. An ordinary "why"
    /// comment stays inline and dense (spyc runs ~22% comment density by
    /// design); a `SPYC-TRAP(<slug>)` marks the rare invariant whose failure is
    /// *silent* — "undo this and queries return wrong rows / the session
    /// crashes only over SSH" — the kind a future edit destroys without
    /// noticing, and which is *harder*, not easier, to find buried in the
    /// comment flood. The terse code anchor derefs to the full rationale in
    /// ARCHITECTURE.md, keyed by the slug — a stable join that survives the doc
    /// being reworded (the heading text is deliberately NOT the key).
    ///
    /// This guard pins BOTH ends against the slug so a reference can't rot
    /// green: every `SPYC-TRAP(<slug>)` in `src/` must resolve to a
    /// `<!-- SPYC-TRAP: <slug> -->` marker in ARCHITECTURE.md (no dangling
    /// code→doc ref), AND every marker must have at least one code referrer (no
    /// orphan store entry). See AGENTS.md → "Load-bearing trap anchors".
    #[test]
    fn traps_resolve_against_architecture_anchors() {
        // Assemble the sigil so this guard's own source (already skipped by
        // `scan_rs`) can never register as an anchor.
        let sigil = format!("{}{}", "SPYC-", "TRAP");
        let code_open = format!("{sigil}(");
        let store_open = format!("<!-- {sigil}: ");

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        // Code side: `SPYC-TRAP(<slug>)` across production src/.
        let mut code: Vec<(String, String)> = Vec::new(); // (slug, file)
        scan_rs(&root.join("src"), &mut |path, src| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            for slug in slugs_between(src, &code_open, ")") {
                code.push((slug, name.clone()));
            }
        });

        // Store side: `<!-- SPYC-TRAP: <slug> -->` in ARCHITECTURE.md.
        let arch =
            std::fs::read_to_string(root.join("ARCHITECTURE.md")).expect("read ARCHITECTURE.md");
        let store = slugs_between(&arch, &store_open, " -->");

        // No duplicate store entries — an ambiguous deref target.
        let mut dupes: Vec<&String> = store
            .iter()
            .filter(|s| store.iter().filter(|x| x == s).count() > 1)
            .collect();
        dupes.sort();
        dupes.dedup();
        assert!(
            dupes.is_empty(),
            "duplicate SPYC-TRAP markers in ARCHITECTURE.md: {dupes:?}. \
             Each slug names one rationale section — merge or rename."
        );

        // Forward: every code anchor resolves to a store marker.
        let mut dangling: Vec<String> = code
            .iter()
            .filter(|(slug, _)| !store.contains(slug))
            .map(|(slug, file)| format!("{slug} ({file})"))
            .collect();
        dangling.sort();
        dangling.dedup();
        assert!(
            dangling.is_empty(),
            "SPYC-TRAP anchor(s) with no matching `<!-- SPYC-TRAP: <slug> -->` \
             section in ARCHITECTURE.md: {dangling:?}. Add the rationale section \
             (keyed by the slug) in the same commit. See AGENTS.md → trap anchors."
        );

        // Reverse: every store marker has at least one code referrer.
        let mut orphans: Vec<String> = store
            .iter()
            .filter(|slug| !code.iter().any(|(s, _)| s == *slug))
            .cloned()
            .collect();
        orphans.sort();
        orphans.dedup();
        assert!(
            orphans.is_empty(),
            "orphan SPYC-TRAP section(s) in ARCHITECTURE.md (no `SPYC-TRAP(<slug>)` \
             referrer in src/): {orphans:?}. The code site went away — drop the \
             section, or restore the anchor. See AGENTS.md → trap anchors."
        );
    }

    /// Every `<slug>` framed by `open`…`close`, where a slug is a run of
    /// `[a-z0-9-]`. Anything else (a doc template written `(<slug>)`) is skipped,
    /// so prose examples never register as real anchors.
    fn slugs_between(hay: &str, open: &str, close: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = hay;
        while let Some(i) = rest.find(open) {
            let after = &rest[i + open.len()..];
            let Some(j) = after.find(close) else { break };
            let slug = &after[..j];
            if !slug.is_empty()
                && slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                out.push(slug.to_string());
            }
            rest = &after[j + close.len()..];
        }
        out
    }

    /// Recursively read every `.rs` under `dir`, skipping whole-file test
    /// modules (`*_tests.rs`, `mod_tests.rs` / `test_harness.rs`, and `tests/` /
    /// `*_tests/` dirs — they carry no in-file `#[cfg(test)]` marker, so the
    /// production-split heuristic would misread them). Calls `f(path, source)`.
    fn scan_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path, &str)) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                let dname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dname == "tests" || dname.ends_with("_tests") {
                    continue;
                }
                scan_rs(&path, f);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "mod_tests.rs"
                    || name == "test_harness.rs"
                    || name.ends_with("_tests.rs")
                {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read .rs");
                f(&path, &src);
            }
        }
    }

    /// Reasoning-leakage slop in committed comments — the "thinking out loud"
    /// that LLM authoring leaks into code. The offender that prompted this: a
    /// test debating itself above a one-line assert (five lines of "...let's
    /// check / Actually looking at the code / Wait, actually ..."). It reads as
    /// careless and taints the credibility of the genuine "why" comments around
    /// it. A curated, high-signal phrase list — NOT a density cap, and
    /// deliberately NOT the adverb "actually" (legitimate technical English,
    /// ~55 uses in this tree). See AGENTS.md → "Comments state what IS".
    #[test]
    fn comments_carry_no_reasoning_leakage() {
        // Multi-word deliberation phrases only, so a wrapped doc line or the
        // bare adverb can't false-trip (validated against the whole tree).
        const SLOP: &[&str] = &[
            "wait, actually",
            "let's check",
            "let's see",
            "let me check",
            "let me look",
            "let me think",
            "looking at the code",
            " hmm",
            "i think ",
            "i'm not sure",
            "not sure if",
            "i guess",
            "i suspect",
            "on second thought",
            "never mind",
            "nevermind",
            "scratch that",
            "as i said",
            "so basically",
            "to be honest",
            "honestly,",
        ];
        let manifest = env!("CARGO_MANIFEST_DIR");
        let root = std::path::Path::new(manifest).join("src");
        let mut offenders = Vec::new();
        scan_all_rs(&root, &mut |path, src| {
            // Skip this guard's own home — it spells the phrases out above.
            if path.file_name().and_then(|n| n.to_str()) == Some("mod_tests.rs") {
                return;
            }
            let rel = path.strip_prefix(manifest).unwrap_or(path).display();
            for (i, line) in src.lines().enumerate() {
                let Some(idx) = line.find("//") else { continue };
                let comment = line[idx..].to_ascii_lowercase();
                if let Some(p) = SLOP.iter().find(|p| comment.contains(**p)) {
                    offenders.push(format!("{rel}:{}  ({p})", i + 1));
                }
            }
        });
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "reasoning-leakage in comments — delete it; state the decision or \
             invariant, not the thought process behind it (AGENTS.md → \
             \"Comments state what IS\"):\n{}",
            offenders.join("\n")
        );
    }

    /// Walk every `.rs` under `dir`, INCLUDING test files — comment slop in a
    /// test is still slop (the offender that prompted the guard was a test).
    /// `scan_rs`'s sibling without the production-only skip list.
    fn scan_all_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path, &str)) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                scan_all_rs(&path, f);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let src = std::fs::read_to_string(&path).expect("read .rs");
                f(&path, &src);
            }
        }
    }
}

#[cfg(test)]
mod refresh_debounce_tests {
    use super::super::should_fire_refresh;
    use std::time::{Duration, Instant};

    const QUIET: Duration = Duration::from_millis(500);
    const MAX_DEFER: Duration = Duration::from_secs(1);

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// MVU Phase 2 pins the RefreshQuiet armed instant to the
    /// `should_fire_refresh` predicate edge: it must be true AT the armed
    /// instant and false 1 ms before, so the scheduler can never drive the
    /// recv wait to zero by arming before the predicate can fire. The
    /// `fire_at` here is the exact formula used to arm `Deadline::RefreshQuiet`.
    #[test]
    fn fires_exactly_at_the_armed_edge() {
        let base = Instant::now();
        let last_refresh = base;
        let last_event = at(base, 100);
        let first_event = at(base, 100);
        // App::run arms at: max(last_refresh+QUIET, min(last_event+QUIET,
        // first+MAX_DEFER)) = max(base+500, min(base+600, base+1100)) = base+600.
        let fire_at = (last_refresh + QUIET).max((last_event + QUIET).min(first_event + MAX_DEFER));
        assert_eq!(fire_at, at(base, 600));
        assert!(should_fire_refresh(
            Some(last_event),
            last_refresh,
            Some(first_event),
            fire_at,
            QUIET,
            MAX_DEFER
        ));
        // 1 ms before the edge (base+599): the predicate must NOT fire.
        assert!(!should_fire_refresh(
            Some(last_event),
            last_refresh,
            Some(first_event),
            at(base, 599),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn no_pending_event_never_fires() {
        let base = Instant::now();
        assert!(!should_fire_refresh(
            None,
            base,
            None,
            at(base, 5_000),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn fires_after_trailing_quiet() {
        let base = Instant::now();
        // Last event at t=0, now at t=600 ms → 600 ms of quiet → fire.
        assert!(should_fire_refresh(
            Some(base),
            base,
            Some(base),
            at(base, 600),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn waits_during_trailing_quiet_window() {
        let base = Instant::now();
        // Last event at t=400, now at t=500 → only 100 ms of quiet → wait.
        assert!(!should_fire_refresh(
            Some(at(base, 400)),
            base,
            Some(base),
            at(base, 500),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn max_defer_breaks_starvation_under_continuous_activity() {
        // The regression: events keep arriving so trailing-quiet is
        // never met, but the first-event-of-this-stretch was >= max_defer
        // ago → fire anyway so markers don't stay stale forever.
        let base = Instant::now();
        let still_active = at(base, 1_100); // last event 100 ms ago — NOT quiet
        let now = at(base, 1_200);
        let first_event = base; // 1.2 s ago, > MAX_DEFER (1 s)
        assert!(should_fire_refresh(
            Some(still_active),
            base,
            Some(first_event),
            now,
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn rate_limit_blocks_back_to_back_fires() {
        let base = Instant::now();
        // Trailing quiet met but last_refresh was only 100 ms ago → wait.
        assert!(!should_fire_refresh(
            Some(base),
            at(base, 400), // last_refresh 100 ms before now
            Some(base),
            at(base, 500),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn rate_limit_also_gates_max_defer_path() {
        // Even when max-defer would fire, we still respect the rate
        // limit so we never refresh twice within `refresh_quiet`.
        let base = Instant::now();
        let now = at(base, 1_200);
        assert!(!should_fire_refresh(
            Some(at(base, 1_100)), // not quiet
            at(base, 900),         // last_refresh 300 ms ago — too recent
            Some(base),            // first_event 1.2 s ago — max_defer hit
            now,
            QUIET,
            MAX_DEFER
        ));
    }
}

#[cfg(test)]
mod post_chord_bounce_tests {
    use super::super::{POST_CHORD_BOUNCE_WINDOW, is_post_chord_bounce};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::{Duration, Instant};

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn swallows_same_key_bounce_in_window_when_idle() {
        // `^a-j` just completed; a stray `j` within the window with the
        // resolver idle is a bounce → swallow.
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(is_post_chord_bounce(stamp, press('j'), false));
    }

    #[test]
    fn does_not_swallow_when_resolver_pending() {
        // The regression: a fresh `^a` made the resolver pending, so the
        // incoming `j` completes a NEW chord and must reach the resolver.
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('j'), true));
    }

    #[test]
    fn does_not_swallow_different_key() {
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('k'), false));
    }

    #[test]
    fn does_not_swallow_with_modifiers() {
        // A second `^a` (Ctrl-A) must never be swallowed as a bounce.
        let stamp = Some((Instant::now(), KeyCode::Char('a')));
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(!is_post_chord_bounce(stamp, ctrl_a, false));
    }

    #[test]
    fn does_not_swallow_after_window_expires() {
        let past = Instant::now()
            .checked_sub(POST_CHORD_BOUNCE_WINDOW + Duration::from_millis(40))
            .unwrap();
        let stamp = Some((past, KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('j'), false));
    }

    #[test]
    fn no_stamp_never_swallows() {
        assert!(!is_post_chord_bounce(None, press('j'), false));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::super::state::{Side, VSplit, VsplitMode};
    use super::super::{App, StatusPosition};
    use ratatui::layout::Rect;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn no_pane_top_status_at_row_0() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert_eq!(l.list.y, 1);
        assert_eq!(l.prompt.y, 23);
    }

    #[test]
    fn no_pane_bottom_status_at_last_row() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Bottom);
        assert_eq!(l.list.y, 0);
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
    }

    #[test]
    fn pane_open_top_status_above_list() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert!(l.list.y > l.status.y);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(div.y + 1, pane.y);
        // prompt sits in the top region, above the divider.
        assert!(l.prompt.y < div.y);
    }

    #[test]
    fn pane_open_bottom_status_below_pane() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Bottom);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(l.list.y, 0);
        assert_eq!(l.list.y + l.list.height, div.y);
        assert_eq!(div.y + 1, pane.y);
        // prompt one above status, both at the very bottom.
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
        // pane ends at the row above prompt.
        assert!(pane.y + pane.height <= l.prompt.y);
    }

    /// The top overlay / TopPane pager paints `top_unit`; it must always sit
    /// inside the frame. The bug: it was built as `status.y + Σheights`, so
    /// with bottom status (status on the last row) it anchored off-screen and
    /// panicked in `Buffer::set_string`.
    #[test]
    fn top_unit_stays_within_frame_all_configs() {
        for pane_open in [false, true] {
            for status_pos in [StatusPosition::Top, StatusPosition::Bottom] {
                let l = App::compute_layout(area(80, 24), pane_open, 50, status_pos);
                let tu = l.top_unit;
                assert_eq!(tu.x, 0);
                assert_eq!(tu.y, 0, "top_unit must anchor at the frame top");
                assert!(
                    tu.y + tu.height <= 24,
                    "top_unit overflows the frame: {tu:?} ({pane_open}, {status_pos:?})"
                );
                assert!(tu.height > 0);
            }
        }
    }

    #[test]
    fn top_unit_is_the_region_above_the_divider() {
        // With a pane open, the overlay region ends exactly at the divider
        // under both status positions.
        for status_pos in [StatusPosition::Top, StatusPosition::Bottom] {
            let l = App::compute_layout(area(80, 24), true, 50, status_pos);
            let div = l.divider.unwrap();
            assert_eq!(
                l.top_unit.y + l.top_unit.height,
                div.y,
                "top_unit should span up to the divider ({status_pos:?})"
            );
        }
    }

    /// BottomPane zoom (`pane_pct >= 100`, top status): the pane fills the
    /// frame below a single status row. status + prompt share that top row,
    /// the divider sits just below it, and there's no file list — so a zoomed
    /// session still surfaces spyc's flash / chord-arming / prompt.
    #[test]
    fn bottom_pane_zoom_keeps_one_top_status_row() {
        let l = App::compute_layout(area(80, 24), true, 100, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert_eq!(l.status.height, 1);
        // The flash / arming / prompt line shares the single top row.
        assert_eq!(l.prompt, l.status);
        let div = l.divider.unwrap();
        let pane = l.pane.unwrap();
        assert_eq!(div.y, 1, "divider sits just below the status row");
        assert_eq!(pane.y, 2, "pane starts below status + divider");
        assert_eq!(pane.y + pane.height, 24, "pane runs to the frame bottom");
        assert_eq!(l.list.height, 0, "no file list while the pane is zoomed");
    }

    /// TopList zoom (`pane_pct == 0`, top status): the list fills the body
    /// above a single pane **tab bar** at the very bottom (so the hidden
    /// pane's tabs stay visible and `^a <n>` can fullscreen one). There is no
    /// pane rect — the pty runs off-screen until un-zoom.
    #[test]
    fn list_zoom_keeps_bottom_tab_bar_and_no_pane() {
        let l = App::compute_layout(area(80, 24), true, 0, StatusPosition::Top);
        assert_eq!(l.status.y, 0, "status bar stays at the top");
        assert!(l.list.height > 0, "the list fills the body");
        let div = l.divider.unwrap();
        assert_eq!(div.y, 23, "tab bar at the very bottom row");
        assert_eq!(l.prompt.y, 22, "prompt sits just above the tab bar");
        assert!(
            l.pane.is_none(),
            "no pane rect — the pty runs off-screen while the list is zoomed"
        );
    }

    /// `carve_vsplit` TopOnly: splits only the list region into
    /// left | divider(1) | right; status/divider/pane stay full-width. At 80
    /// cols, pct 45 → right 36, divider at 43, left 43.
    #[test]
    fn carve_vsplit_top_only_splits_just_the_list() {
        let base = App::compute_layout(area(80, 24), true, 50, StatusPosition::Top);
        let l = App::carve_vsplit(
            base,
            VSplit {
                width_pct: 45,
                mode: VsplitMode::TopOnly,
                focus: Side::Right,
            },
            area(80, 24),
            None,
        );
        // List shrinks to the left column.
        assert_eq!(l.list.x, 0);
        assert_eq!(l.list.width, 43);
        // 1-column divider, then the right column to the frame edge.
        let vd = l.vdivider.unwrap();
        assert_eq!((vd.x, vd.width), (43, 1));
        let right = l.right.unwrap();
        assert_eq!((right.x, right.width), (44, 36));
        assert_eq!(right.x + right.width, 80, "right column reaches the edge");
        // The right column + divider mirror the left list exactly — same top,
        // same height — so they reserve the prompt row instead of claiming it,
        // keeping the two columns symmetrical.
        assert_eq!(right.y, l.list.y);
        assert_eq!(
            right.height, l.list.height,
            "right matches the left list height"
        );
        assert_eq!(
            right.y + right.height,
            l.list.y + l.list.height,
            "right ends where the left list does (prompt row reserved)"
        );
        assert_eq!(
            vd.y + vd.height,
            l.list.y + l.list.height,
            "divider ends where the left list does"
        );
        // The prompt/flash/command row is the shared lowest line BELOW both
        // columns, so it keeps the FULL frame width (a flash message or `:`
        // command line is global, not column-scoped). The status bar and pane
        // are full-width too (TopOnly).
        assert_eq!(
            l.prompt.width, 80,
            "prompt/flash row spans the full frame width"
        );
        assert_eq!(l.status.width, 80);
        assert_eq!(l.pane.unwrap().width, 80);
    }

    /// Regression: a `TopOnly` split must NOT truncate the shared bottom
    /// prompt/flash/command row at the divider — it spans the full frame width
    /// whether or not a pty pane is open (a flash like a long worktree path was
    /// being cut at the left column). The columns always end above this row, so
    /// nothing renders beside it. (`FullHeight` is different — its right column
    /// owns that row's full height, so it stays clamped, covered separately.)
    #[test]
    fn carve_vsplit_top_only_prompt_row_spans_full_width() {
        for has_pane in [false, true] {
            let l = App::carve_vsplit(
                App::compute_layout(area(80, 24), has_pane, 50, StatusPosition::Top),
                VSplit {
                    width_pct: 45,
                    mode: VsplitMode::TopOnly,
                    focus: Side::Left,
                },
                area(80, 24),
                None,
            );
            assert_eq!(
                l.prompt.width, 80,
                "TopOnly prompt/flash row must be full-width (has_pane={has_pane})"
            );
        }
    }

    /// `top_unit` (the V/D overlay + TopPane-pager region) follows the column
    /// the overlay opened in — not the current focus — so a V/D stays *inside*
    /// its column even when `^a l`/`^a h` moves focus to the other one. Scoped to
    /// the left column for an overlay in `a`, the right column's rect for `b`.
    /// Both sit below the shared status row (`y == list.y`), so the status bar
    /// stays visible.
    #[test]
    fn carve_vsplit_top_unit_follows_overlay_column() {
        let mk = |overlay_side| {
            App::carve_vsplit(
                App::compute_layout(area(80, 24), true, 50, StatusPosition::Top),
                VSplit {
                    width_pct: 45,
                    mode: VsplitMode::TopOnly,
                    // Focus deliberately opposite the overlay column, to prove
                    // `top_unit` tracks the overlay, not focus.
                    focus: Side::Left,
                },
                area(80, 24),
                overlay_side,
            )
        };
        // Overlay in the left column → scoped to the left (width 43, at x 0).
        let l = mk(Some(Side::Left));
        assert_eq!((l.top_unit.x, l.top_unit.width), (0, 43));
        // Overlay in the right column (focus still Left) → top_unit is the right
        // column's rect — the overlay stayed in `b`.
        let r = mk(Some(Side::Right));
        assert_eq!((r.top_unit.x, r.top_unit.width), (44, 36));
        assert_eq!(
            Some(r.top_unit),
            r.right,
            "top_unit == the right column rect"
        );
        // Both sit below the status row (row 0) so the status bar still renders.
        assert!(l.top_unit.y >= 1 && r.top_unit.y >= 1);
    }

    /// `carve_vsplit` FullHeight: the divider runs the whole frame height, the
    /// left-column chrome (incl. the PTY pane) is clamped to the left width,
    /// and the right column spans the full frame height.
    #[test]
    fn carve_vsplit_full_height_clamps_left_chrome() {
        let base = App::compute_layout(area(80, 24), true, 50, StatusPosition::Top);
        let l = App::carve_vsplit(
            base,
            VSplit {
                width_pct: 45,
                mode: VsplitMode::FullHeight,
                focus: Side::Right,
            },
            area(80, 24),
            None,
        );
        assert_eq!(l.status.width, 43, "status clamped to left column");
        assert_eq!(l.list.width, 43);
        assert_eq!(l.prompt.width, 43);
        assert_eq!(
            l.pane.unwrap().width,
            43,
            "PTY pane confined under left col"
        );
        let right = l.right.unwrap();
        assert_eq!((right.x, right.width), (44, 36));
        assert_eq!((right.y, right.height), (0, 24), "right spans full height");
        assert_eq!(l.vdivider.unwrap().height, 24, "divider runs full height");
    }

    /// `carve_vsplit` refuses (single-column passthrough) when the frame is too
    /// narrow to host two usable columns — never builds a 0/1-col rect.
    #[test]
    fn carve_vsplit_too_narrow_stays_single_column() {
        let base = App::compute_layout(area(30, 24), true, 50, StatusPosition::Top);
        let l = App::carve_vsplit(
            base,
            VSplit {
                width_pct: 50,
                mode: VsplitMode::TopOnly,
                focus: Side::Left,
            },
            area(30, 24),
            None,
        );
        assert!(l.right.is_none(), "no right column when too narrow");
        assert!(l.vdivider.is_none());
        assert_eq!(l.list.width, 30, "list keeps the full width");
    }
}

#[cfg(test)]
mod history_bucket_tests {
    use super::super::{HistoryBucket, PromptKind, history_bucket_for};

    #[test]
    fn pane_command_and_cwd_use_distinct_buckets() {
        // The bug this guards: both pane prompts shared one bucket, so
        // directories typed at "pane cwd:" leaked into the "pane
        // command:" Up/Down browse.
        assert_eq!(
            history_bucket_for(Some(&PromptKind::PaneNewTabCmd)),
            HistoryBucket::PaneCmd
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::PaneNewTabCwd)),
            HistoryBucket::PaneCwd
        );
        assert_ne!(HistoryBucket::PaneCmd, HistoryBucket::PaneCwd);
    }

    #[test]
    fn jump_and_command_stay_isolated() {
        assert_eq!(
            history_bucket_for(Some(&PromptKind::Jump)),
            HistoryBucket::Jump
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::Command)),
            HistoryBucket::Command
        );
    }

    #[test]
    fn shell_and_path_prompts_fall_back_to_shell_bucket() {
        assert_eq!(
            history_bucket_for(Some(&PromptKind::ShellCmd)),
            HistoryBucket::Shell
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::CopyTo)),
            HistoryBucket::Shell
        );
        // Normal mode (no prompt) also resolves to the default bucket.
        assert_eq!(history_bucket_for(None), HistoryBucket::Shell);
    }
}

#[cfg(test)]
mod format_uptime_tests {
    use super::super::format_uptime;

    #[test]
    fn seconds_only() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn minutes_and_seconds() {
        assert_eq!(format_uptime(60), "1m 0s");
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn hours_and_minutes() {
        assert_eq!(format_uptime(3600), "1h 00m");
        assert_eq!(format_uptime(3725), "1h 02m");
    }

    #[test]
    fn days_and_hours() {
        assert_eq!(format_uptime(86_400), "1d 0h");
        assert_eq!(format_uptime(90_000), "1d 1h");
    }
}

#[cfg(test)]
mod format_elapsed_hms_tests {
    use super::super::format_elapsed_hms;

    #[test]
    fn seconds_then_minutes() {
        assert_eq!(format_elapsed_hms(0), "0s");
        assert_eq!(format_elapsed_hms(59), "59s");
        assert_eq!(format_elapsed_hms(60), "1m 0s");
        assert_eq!(format_elapsed_hms(125), "2m 5s");
    }

    /// The deliberate divergence from `format_uptime`: past one hour the
    /// live timer keeps seconds (and never coarsens to days), so a long
    /// `!make` keeps ticking second-by-second.
    #[test]
    fn hours_keep_seconds_and_never_roll_to_days() {
        assert_eq!(format_elapsed_hms(3600), "1h 0m 0s");
        assert_eq!(format_elapsed_hms(3725), "1h 2m 5s");
        assert_eq!(format_elapsed_hms(90_000), "25h 0m 0s");
    }
}

#[cfg(test)]
mod eof_marker_tests {
    use super::super::eof_marker_line;

    fn flat(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn exit_zero_renders_with_tail() {
        let line = eof_marker_line("exit 0");
        assert_eq!(flat(&line), "[EOF — exit 0]");
    }

    #[test]
    fn killed_status_renders() {
        let line = eof_marker_line("killed (12s)");
        assert_eq!(flat(&line), "[EOF — killed (12s)]");
    }

    #[test]
    fn marker_is_dim() {
        use ratatui::style::Modifier;
        let line = eof_marker_line("exit 1");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
    }
}

#[cfg(test)]
mod strip_crlf_tests {
    use super::super::util::strip_crlf;

    #[test]
    fn crlf_collapses_to_lf() {
        assert_eq!(strip_crlf(b"a\r\nb\r\nc"), b"a\nb\nc");
    }

    #[test]
    fn passthrough_when_no_carriage_return() {
        assert_eq!(
            strip_crlf(b"hello world\nplain text"),
            b"hello world\nplain text"
        );
    }

    #[test]
    fn bare_cr_collapses_to_last_frame() {
        // git/npm/cargo progress: same line, multiple updates separated
        // by bare CR. We keep only the final frame.
        let input = b"Counting: 18%\rCounting: 27%\rCounting: 100%, done.\n";
        assert_eq!(strip_crlf(input), b"Counting: 100%, done.\n");
    }

    #[test]
    fn bare_cr_with_no_trailing_newline() {
        // Mid-stream view: last frame still wins, no terminator yet.
        assert_eq!(
            strip_crlf(b"Counting: 18%\rCounting: 50%"),
            b"Counting: 50%"
        );
    }

    #[test]
    fn mixed_crlf_and_bare_cr_across_lines() {
        let input = b"line1\r\nProgress: 10%\rProgress: 100%\r\nline3";
        assert_eq!(strip_crlf(input), b"line1\nProgress: 100%\nline3");
    }

    #[test]
    fn strips_soh_from_git_log_commit_message() {
        // Real-world: git log emits \x01 (SOH) in some commit-message
        // rendering paths -- e.g. when the original message contained
        // pasted control bytes. Without stripping, ratatui draws a
        // visible-but-zero-width glyph the host terminal consumes,
        // misaligning the rest of the line.
        let input = b"    \x01\tsrc/Foo.cs\n    \x01\tsrc/Bar.cs";
        assert_eq!(strip_crlf(input), b"    \tsrc/Foo.cs\n    \tsrc/Bar.cs");
    }

    #[test]
    fn strips_other_ascii_control_bytes() {
        // \b (BS), \v (VT), \f (FF), \x1c (FS), \x7f (DEL).
        let input = b"a\x08b\x0bc\x0cd\x1ce\x7ff";
        assert_eq!(strip_crlf(input), b"abcdef");
    }

    #[test]
    fn keeps_tab_newline_and_esc() {
        // \t, \n, and \x1b (ESC for ANSI) survive pass 3.
        let input = b"a\tb\nc\x1b[31md";
        assert_eq!(strip_crlf(input), b"a\tb\nc\x1b[31md");
    }
}

#[cfg(test)]
mod matcher_tests {
    use super::super::Matcher;

    // The allocation-free ASCII fast path in `Matcher::matches` must stay
    // behaviorally identical to the old `name.to_lowercase().contains(q)`.
    #[test]
    fn substring_is_case_insensitive_both_directions() {
        let m = Matcher::build("env");
        assert!(m.matches(".ENV"));
        assert!(m.matches(".envrc"));
        assert!(m.matches("Environment.toml"));
        assert!(!m.matches("readme.md"));

        // Uppercase query lowercases at build time.
        let m = Matcher::build("ENV");
        assert!(m.matches(".env"));
    }

    #[test]
    fn substring_empty_query_matches_everything() {
        let m = Matcher::build("");
        assert!(m.matches("anything"));
        assert!(m.matches(""));
    }

    #[test]
    fn substring_unicode_falls_back_to_lowercase() {
        // Non-ASCII names take the to_lowercase path; case folding must hold.
        let m = Matcher::build("café");
        assert!(m.matches("CAFÉ.txt"));
        assert!(m.matches("le-café"));
        assert!(!m.matches("coffee"));
    }

    #[test]
    fn substring_needle_longer_than_name_is_no_match() {
        let m = Matcher::build("readme");
        assert!(!m.matches("rd"));
    }

    #[test]
    fn glob_skips_alloc_for_lowercase_ascii_but_stays_case_insensitive() {
        let m = Matcher::build("*.RS");
        assert!(m.matches("main.rs")); // already-lowercase fast path
        assert!(m.matches("MAIN.RS")); // uppercase → lowercase fallback
        assert!(!m.matches("main.py"));
    }
}

#[cfg(test)]
mod git_key_tests {
    use super::super::RowData;
    use crate::fs::EntryKind;
    use std::path::PathBuf;

    fn row(name: &str, display: &str, kind: EntryKind) -> RowData {
        RowData {
            path: PathBuf::from("/repo").join(name),
            display: display.to_string(),
            kind,
            deleted: false,
        }
    }

    /// `git.files` keys files by bare basename and dirs by `basename/` — which
    /// equals `display` for every kind except executables, whose `*` decoration
    /// the git map never carries. `git_key` must strip exactly that suffix so
    /// the lookup matches; everything else passes `display` through untouched.
    #[test]
    fn strips_only_the_executable_star() {
        // Executable: the `*` is stripped back to the bare basename key.
        assert_eq!(row("run", "run*", EntryKind::Executable).git_key(), "run");
        // Regular file / symlink / other: display is the key already.
        assert_eq!(row("a.rs", "a.rs", EntryKind::File).git_key(), "a.rs");
        assert_eq!(row("link", "link", EntryKind::Symlink).git_key(), "link");
        // Directory: the trailing `/` IS the key (it's how the map stores dirs),
        // so it must survive.
        assert_eq!(row("sub", "sub/", EntryKind::Dir).git_key(), "sub/");
    }

    /// A file genuinely named `foo*` decorates to `foo**`; stripping one `*`
    /// still yields its real basename key `foo*`.
    #[test]
    fn strips_just_one_star_for_a_starred_name() {
        assert_eq!(
            row("foo*", "foo**", EntryKind::Executable).git_key(),
            "foo*"
        );
    }
}

#[cfg(test)]
mod hud_constant_tests {
    use super::super::App;

    // The activity-HUD process constants must be snapshotted at construction
    // so the pure render pass reads no OS/env per frame.
    #[test]
    fn view_snapshots_pid_and_term_at_construction() {
        let app = App::test_app(std::env::temp_dir());
        assert_eq!(app.view.hud_pid, std::process::id());
        // `hud_term` mirrors $TERM, or "?" when unset — never reads env again.
        let expected = std::env::var("TERM").unwrap_or_else(|_| "?".to_string());
        assert_eq!(app.view.hud_term, expected);
    }
}
