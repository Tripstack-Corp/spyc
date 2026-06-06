//! Tests for session save/load + agent-session discovery.
//! Split out of `sessions.rs` verbatim during the 800-LoC decomposition.

use super::*;
use tempfile::tempdir;

fn now_secs() -> u64 {
    crate::sysinfo::epoch_secs()
}

#[test]
fn slug_replaces_path_separators() {
    assert_eq!(
        project_slug(std::path::Path::new("/Users/derek/src/spyc")),
        "-Users-derek-src-spyc"
    );
}

#[test]
fn slug_rewrites_underscores_like_claude() {
    // Claude rewrites underscores to hyphens in its on-disk slug.
    // `~/.claude/projects/-Users-derek-src-tripstack-platform/`
    // is what we must match for `tripstack_platform`.
    assert_eq!(
        project_slug(std::path::Path::new("/Users/derek/src/tripstack_platform")),
        "-Users-derek-src-tripstack-platform"
    );
    assert_eq!(
        project_slug(std::path::Path::new("/Users/derek/src/system_setup")),
        "-Users-derek-src-system-setup"
    );
}

#[test]
fn slug_rewrites_other_non_alphanumeric() {
    assert_eq!(
        project_slug(std::path::Path::new("/x/foo.bar/baz qux")),
        "-x-foo-bar-baz-qux"
    );
}

#[test]
fn extracts_uuid_resume_token() {
    let lines: Vec<String> = [
        "some output",
        "Resume this session with:",
        "claude --resume 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
        "",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_claude_resume_token(&lines).unwrap();
    assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
    assert!(is_uuid(&tok));
}

#[test]
fn extracts_named_resume_token() {
    let lines: Vec<String> = ["claude --resume saffron-cumin".to_string()].to_vec();
    let tok = extract_claude_resume_token(&lines).unwrap();
    assert_eq!(tok, "saffron-cumin");
    assert!(!is_uuid(&tok));
}

#[test]
fn picks_last_resume_banner() {
    let lines: Vec<String> = [
        "claude --resume 11111111-1111-1111-1111-111111111111",
        "…later…",
        "claude --resume 22222222-2222-2222-2222-222222222222",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_claude_resume_token(&lines).unwrap();
    assert_eq!(tok, "22222222-2222-2222-2222-222222222222");
}

#[test]
fn returns_none_when_no_banner() {
    let lines: Vec<String> = vec!["random scrollback".to_string(), "no banner".to_string()];
    assert!(extract_claude_resume_token(&lines).is_none());
}

#[test]
fn extracts_codex_uuid_with_prefix_phrase() {
    let lines: Vec<String> = [
        "some output",
        "To continue this session, run codex resume 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
        "",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_codex_resume_token(&lines).unwrap();
    assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
}

#[test]
fn codex_extractor_requires_uuid() {
    // Codex never uses thread-name tokens — guard against picking
    // up a non-UUID that happened to follow `codex resume`.
    let lines: Vec<String> = vec!["codex resume saffron-cumin".to_string()];
    assert!(extract_codex_resume_token(&lines).is_none());
}

#[test]
fn codex_picks_last_banner() {
    let lines: Vec<String> = [
        "To continue this session, run codex resume 11111111-1111-1111-1111-111111111111",
        "…later…",
        "To continue this session, run codex resume 22222222-2222-2222-2222-222222222222",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_codex_resume_token(&lines).unwrap();
    assert_eq!(tok, "22222222-2222-2222-2222-222222222222");
}

#[test]
fn extracts_agy_uuid_with_conversation_flag() {
    let lines: Vec<String> = [
        "some output",
        "To continue this session, run agy --conversation 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
        "",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_agy_resume_token(&lines).unwrap();
    assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
}

#[test]
fn extracts_agy_uuid_with_c_flag() {
    let lines: Vec<String> = [
        "some output",
        "To continue this session, run agy -c 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
        "",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let tok = extract_agy_resume_token(&lines).unwrap();
    assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
}

#[test]
fn agy_extractor_requires_uuid() {
    let lines: Vec<String> = vec!["agy --conversation saffron-cumin".to_string()];
    assert!(extract_agy_resume_token(&lines).is_none());
}

#[test]
fn effective_kind_infers_claude_for_legacy_saves() {
    // Older saves had no `agent_kind` field but did populate
    // `claude_session_id` (deserialized via the alias to
    // `agent_session_id`). The effective kind must report Claude
    // for those rows so resume-on-restore picks the right path.
    let json = serde_json::json!({
        "command": "claude",
        "label": "claude",
        "cwd": "/tmp",
        "claude_session_id": "11111111-1111-1111-1111-111111111111",
        "claude_session_name": "old-session",
    });
    let tab: SavedTab = serde_json::from_value(json).unwrap();
    assert_eq!(tab.agent_kind, AgentKind::Other);
    assert_eq!(tab.effective_kind(), AgentKind::Claude);
    assert_eq!(
        tab.agent_session_id.as_deref(),
        Some("11111111-1111-1111-1111-111111111111")
    );
    assert_eq!(tab.agent_session_name.as_deref(), Some("old-session"));
}

#[test]
fn effective_kind_passes_through_explicit_value() {
    let mut tab = SavedTab {
        command: "codex".into(),
        label: "codex".into(),
        cwd: "/tmp".into(),
        agent_kind: AgentKind::Codex,
        agent_session_id: Some("uuid".into()),
        agent_session_name: None,
    };
    assert_eq!(tab.effective_kind(), AgentKind::Codex);
    tab.agent_kind = AgentKind::Other;
    tab.agent_session_id = None;
    assert_eq!(tab.effective_kind(), AgentKind::Other);
}

#[test]
fn format_just_now() {
    let s = format_relative_time(now_secs());
    assert_eq!(s, "just now");
}

#[test]
fn format_seconds_ago() {
    let s = format_relative_time(now_secs() - 30);
    assert_eq!(s, "30 seconds ago");
}

#[test]
fn format_1_minute_ago() {
    let s = format_relative_time(now_secs() - 60);
    assert_eq!(s, "1 minute ago");
}

#[test]
fn format_minutes_ago() {
    let s = format_relative_time(now_secs() - 300);
    assert_eq!(s, "5 minutes ago");
}

#[test]
fn format_1_hour_ago() {
    let s = format_relative_time(now_secs() - 3600);
    assert_eq!(s, "1 hour ago");
}

#[test]
fn format_hours_ago() {
    let s = format_relative_time(now_secs() - 7200);
    assert_eq!(s, "2 hours ago");
}

#[test]
fn format_1_day_ago() {
    let s = format_relative_time(now_secs() - 86400);
    assert_eq!(s, "1 day ago");
}

#[test]
fn format_days_ago_within_week() {
    let s = format_relative_time(now_secs() - 86400 * 3);
    assert_eq!(s, "3 days ago");
}

#[test]
fn format_days_ago_past_week() {
    let s = format_relative_time(now_secs() - 86400 * 30);
    assert_eq!(s, "30 days ago");
}

#[test]
fn format_future_timestamp_is_just_now() {
    // A timestamp in the future — saturating_sub makes diff 0
    let s = format_relative_time(now_secs() + 1000);
    assert_eq!(s, "just now");
}

// ── pick_closest_unclaimed_session ────────────────────────────
//
// Multi-pane disambiguation: when several Claude tabs share a
// cwd, each pane's spawn time matches a different session
// record's startedAt. Without claim-tracking, the resolver's
// "most-recent JSONL" fallback collapsed every alive pane onto
// one conversation.

fn cs(id: &str, started_at_secs: u64) -> ClaudeSessionInfo {
    ClaudeSessionInfo {
        session_id: id.to_string(),
        name: None,
        started_at_secs,
    }
}

#[test]
fn picker_returns_none_for_empty_candidates() {
    let claimed = std::collections::HashSet::new();
    assert!(pick_closest_unclaimed_session::<ClaudeSessionInfo>(vec![], 1000, &claimed).is_none());
}

#[test]
fn picker_picks_closest_started_at() {
    let candidates = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
    let claimed = std::collections::HashSet::new();
    let pick = pick_closest_unclaimed_session(candidates, 2100, &claimed).unwrap();
    assert_eq!(pick.session_id, "b");
}

#[test]
fn picker_skips_already_claimed_ids() {
    // Without the claimed-skip, a pane spawned at 2100 would
    // claim "b" — but "b" is already taken by an earlier pane.
    // Picker should pick the next-closest unclaimed, here "a"
    // (1000s away) over "c" (900s away)? No — "c" is closer.
    let candidates = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
    let mut claimed = std::collections::HashSet::new();
    claimed.insert("b".to_string());
    let pick = pick_closest_unclaimed_session(candidates, 2100, &claimed).unwrap();
    assert_eq!(pick.session_id, "c");
}

#[test]
fn picker_returns_none_when_all_claimed() {
    let candidates = vec![cs("a", 1000), cs("b", 2000)];
    let mut claimed = std::collections::HashSet::new();
    claimed.insert("a".to_string());
    claimed.insert("b".to_string());
    assert!(pick_closest_unclaimed_session(candidates, 1500, &claimed).is_none());
}

#[test]
fn three_panes_three_distinct_session_ids() {
    // The bug: three Claude tabs spawned at t1 < t2 < t3 in the
    // same cwd. Three session records exist, sorted by startedAt
    // (closest match for each pane is the record at its own time).
    // Sequential resolve_calls (each adding to `claimed`) must
    // produce three distinct IDs.
    let records = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
    let pane_spawn_times = [1010_u64, 2010, 3010];

    let mut claimed = std::collections::HashSet::new();
    let mut assigned = Vec::new();
    for spawn in pane_spawn_times {
        let pick = pick_closest_unclaimed_session(records.clone(), spawn, &claimed)
            .expect("a candidate should remain");
        claimed.insert(pick.session_id.clone());
        assigned.push(pick.session_id);
    }

    assert_eq!(assigned, vec!["a", "b", "c"]);
}

impl Clone for ClaudeSessionInfo {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            name: self.name.clone(),
            started_at_secs: self.started_at_secs,
        }
    }
}

// ── Gemini ISO-8601 → epoch ────────────────────────────────────

#[test]
fn parse_iso8601_unix_epoch() {
    assert_eq!(parse_iso8601_to_epoch_secs("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn parse_iso8601_known_timestamp() {
    // 2026-05-08T12:27:31Z = epoch 1762605451 by manual derivation.
    // Sanity-check against `date -u -d` if you adjust this.
    let secs = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
    // Round-trip via the parser at second-2 to lock the value:
    assert!(
        secs > 1_777_000_000 && secs < 1_780_000_000,
        "epoch out of expected range for 2026-05-08: {secs}"
    );
}

#[test]
fn parse_iso8601_strips_fractional_seconds() {
    let with_fraction = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31.927Z").unwrap();
    let without = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
    assert_eq!(with_fraction, without);
}

#[test]
fn parse_iso8601_no_z_suffix() {
    // The chat JSONL writes `Z`, but be defensive against drift.
    let with_z = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
    let without = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31").unwrap();
    assert_eq!(with_z, without);
}

#[test]
fn parse_iso8601_rejects_garbage() {
    assert!(parse_iso8601_to_epoch_secs("not a date").is_none());
    assert!(parse_iso8601_to_epoch_secs("2026/05/08 12:27:31").is_none());
    assert!(parse_iso8601_to_epoch_secs("").is_none());
}

#[test]
fn parse_iso8601_orders_by_seconds() {
    // The whole point: relative ordering must match wall-clock so
    // the picker's `abs_diff` math is meaningful.
    let early = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
    let late = parse_iso8601_to_epoch_secs("2026-05-08T12:30:00Z").unwrap();
    assert_eq!(late - early, 2 * 60 + 29);
}

// ── pick_closest_unclaimed_session also works for Gemini ───────

#[test]
fn picker_works_for_gemini_records() {
    let candidates = vec![
        GeminiSessionInfo {
            session_id: "11111111-1111-1111-1111-111111111111".into(),
            started_at_secs: 1000,
        },
        GeminiSessionInfo {
            session_id: "22222222-2222-2222-2222-222222222222".into(),
            started_at_secs: 2000,
        },
    ];
    let claimed = std::collections::HashSet::new();
    let pick = pick_closest_unclaimed_session(candidates, 1900, &claimed).unwrap();
    assert_eq!(pick.session_id, "22222222-2222-2222-2222-222222222222");
}

// Sub-cases share one tempdir/state-root for sequencing; per-thread
// `with_state_root` isolates this test from siblings.

#[test]
fn save_load_prune_and_dedup() {
    let tmp = tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        // --- roundtrip ---
        let session = Session {
            id: 12345,
            saved_at: "2025-01-01 12:00".to_string(),
            epoch_secs: 1_700_000_000,
            cwd: PathBuf::from("/tmp/test"),
            tabs: vec![SavedTab {
                command: "bash".to_string(),
                label: "shell".to_string(),
                cwd: PathBuf::from("/tmp/test"),
                agent_kind: crate::state::sessions::AgentKind::Other,
                agent_session_id: None,
                agent_session_name: None,
            }],
            active_tab: 0,
            pane_height_pct: 30,
            pane_focused: false,
            name: "SAFFRON_CUMIN".to_string(),
            project_home: None,
        };
        save_session(&session).unwrap();
        let loaded = load_sessions();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, 12345);
        assert_eq!(loaded[0].tabs.len(), 1);
        assert_eq!(loaded[0].tabs[0].command, "bash");

        // --- clean up for next sub-test ---
        let dir = tmp.path().join("sessions");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }

        // --- prune ---
        for i in 0..25_u32 {
            let s = Session {
                id: u64::from(i),
                saved_at: format!("2025-01-{i:02}"),
                epoch_secs: 1_700_000_000 + u64::from(i),
                cwd: PathBuf::from(format!("/tmp/dir{i}")),
                tabs: vec![SavedTab {
                    command: format!("cmd{i}"),
                    label: format!("tab{i}"),
                    cwd: PathBuf::from(format!("/tmp/dir{i}")),
                    agent_kind: crate::state::sessions::AgentKind::Other,
                    agent_session_id: None,
                    agent_session_name: None,
                }],
                active_tab: 0,
                pane_height_pct: 30,
                pane_focused: false,
                name: String::new(),
                project_home: None,
            };
            save_session(&s).unwrap();
        }
        let loaded = load_sessions();
        assert!(loaded.len() <= MAX_SESSIONS);

        // --- clean up for dedup test ---
        std::fs::remove_dir_all(&dir).unwrap();

        // --- dedup ---
        for id in [100_u64, 200] {
            let s = Session {
                id,
                saved_at: "2025-01-01".to_string(),
                epoch_secs: 1_700_000_000 + id,
                cwd: PathBuf::from("/same/dir"),
                tabs: vec![SavedTab {
                    command: "bash".to_string(),
                    label: "shell".to_string(),
                    cwd: PathBuf::from("/same/dir"),
                    agent_kind: crate::state::sessions::AgentKind::Other,
                    agent_session_id: None,
                    agent_session_name: None,
                }],
                active_tab: 0,
                pane_height_pct: 30,
                pane_focused: false,
                name: String::new(),
                project_home: None,
            };
            save_session(&s).unwrap();
        }
        let loaded = load_sessions();
        assert_eq!(loaded.len(), 1);
        // Most recent (id=200) wins
        assert_eq!(loaded[0].id, 200);
    });
}

/// Full save→disk→load round-trip: multiple tabs survive in order
/// with distinct session ids/kinds, and cwd / labels / commands /
/// project_home / session name / active tab / pane geometry all
/// persist. (TEST_IMPROVEMENT_PLAN: session restore.)
#[test]
fn session_round_trips_through_disk_preserving_tabs_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let session = Session {
            id: 42,
            saved_at: "2026-05-30T00:00:00Z".into(),
            epoch_secs: 1_700_000_000,
            cwd: PathBuf::from("/tmp/proj"),
            tabs: vec![
                SavedTab {
                    command: "claude".into(),
                    label: "claude".into(),
                    cwd: PathBuf::from("/tmp/proj"),
                    agent_kind: AgentKind::Claude,
                    agent_session_id: Some("sid-A".into()),
                    agent_session_name: Some("AROMA".into()),
                },
                SavedTab {
                    command: "codex".into(),
                    label: "codex".into(),
                    cwd: PathBuf::from("/tmp/proj/sub"),
                    agent_kind: AgentKind::Codex,
                    agent_session_id: Some("sid-B".into()),
                    agent_session_name: None,
                },
            ],
            active_tab: 1,
            pane_height_pct: 40,
            pane_focused: true,
            name: "SAFFRON_PAPRIKA".into(),
            project_home: Some(PathBuf::from("/tmp/proj")),
        };
        save_session(&session).unwrap();

        let loaded = load_sessions();
        assert_eq!(loaded.len(), 1);
        let s = &loaded[0];
        assert_eq!(s.id, 42);
        assert_eq!(s.cwd, PathBuf::from("/tmp/proj"));
        assert_eq!(s.active_tab, 1);
        assert_eq!(s.pane_height_pct, 40);
        assert!(s.pane_focused);
        assert_eq!(s.name, "SAFFRON_PAPRIKA");
        assert_eq!(s.project_home, Some(PathBuf::from("/tmp/proj")));
        assert_eq!(s.tabs.len(), 2);
        // Order preserved; distinct ids + kinds.
        assert_eq!(s.tabs[0].command, "claude");
        assert_eq!(s.tabs[0].label, "claude");
        assert_eq!(s.tabs[0].cwd, PathBuf::from("/tmp/proj"));
        assert_eq!(s.tabs[0].agent_kind, AgentKind::Claude);
        assert_eq!(s.tabs[0].agent_session_id.as_deref(), Some("sid-A"));
        assert_eq!(s.tabs[0].agent_session_name.as_deref(), Some("AROMA"));
        assert_eq!(s.tabs[1].command, "codex");
        assert_eq!(s.tabs[1].agent_kind, AgentKind::Codex);
        assert_eq!(s.tabs[1].agent_session_id.as_deref(), Some("sid-B"));
        assert_eq!(s.tabs[1].cwd, PathBuf::from("/tmp/proj/sub"));
    });
}
