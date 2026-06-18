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
        // Same vertical band as the list (TopOnly).
        assert_eq!((right.y, right.height), (l.list.y, l.list.height));
        // Everything else stays full-width.
        assert_eq!(l.status.width, 80);
        assert_eq!(l.pane.unwrap().width, 80);
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
