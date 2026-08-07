//! Option B: pin each codex pane's session uuid at/after spawn so `^a v`
//! resolves to the *exact* rollout — bulletproof against resume/rollover and
//! against two fresh codex panes sharing a cwd, which the mtime heuristic in
//! [`crate::state::codex_transcript`] can't fully disambiguate.
//!
//! A `codex resume <uuid>` pane is pinned at launch (`open_pane_tab_in`). A
//! fresh `codex` pane has no id until codex writes its rollout, so an off-thread
//! scan of `~/.codex/sessions` snapshots `(uuid, cwd, start)` and the pure
//! [`assign_codex_sessions`] claims an unclaimed rollout for each unpinned codex
//! tab — processed in spawn order with a shared claimed-set, so two panes in the
//! same cwd deterministically get *distinct* sessions. Pattern mirrors
//! `agent_status`: a landing slot + in-flight flag + a worker that wakes the
//! loop; the kick/apply run in the pre-recv scan, never the render pass.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use super::{App, Message};
use crate::state::codex_transcript::RolloutMeta;
use crate::state::sessions::AgentKind;

/// Stop trying to pin a codex tab this long after spawn. Codex writes its
/// rollout within a second or two; past this the mtime heuristic resolver
/// handles `^a v` (still correct for everything but the same-cwd-twins edge).
const PIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

/// The scan worker waits this long before reading the session dir, so repeated
/// kicks while the rollout hasn't appeared yet *poll* rather than busy-spin.
const SCAN_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// A rollout counts as a fresh tab's session when it started at/after the
/// pane's spawn, modulo small clock jitter (same machine clock for both).
const START_SKEW_SECS: u64 = 5;

/// Pure assignment (the `route.rs` / `focus.rs` template): pin an unclaimed
/// rollout to each unpinned codex tab. `unpinned` is `(tab index, canonical
/// cwd, spawn secs)` **sorted by spawn ascending** so an earlier pane claims the
/// earlier rollout; `already_claimed` holds uuids pinned to other tabs. For each
/// tab, take the earliest-started cwd-matching rollout that began at/after its
/// spawn and isn't yet claimed. Returns `(tab index, uuid)` to apply.
/// One unpinned codex tab, as the pure assignment sees it.
#[derive(Debug, Clone)]
pub(super) struct UnpinnedTab {
    pub idx: usize,
    /// Canonicalized by the caller so this stays a plain string compare.
    pub cwd: String,
    pub spawn: u64,
    /// `resume` with no uuid on the command line. Such a tab appends to a
    /// rollout that *predates* it, so the start-time filter can never match —
    /// it needs the liveness fallback instead.
    pub resuming: bool,
}

fn assign_codex_sessions(
    unpinned: &[UnpinnedTab],
    already_claimed: &HashSet<String>,
    snapshot: &[RolloutMeta],
) -> Vec<(usize, String)> {
    let mut claimed = already_claimed.clone();
    let mut out = Vec::new();
    for tab in unpinned {
        // A fresh session writes a rollout that starts after the pane did.
        let best = snapshot
            .iter()
            .filter(|r| {
                !claimed.contains(&r.uuid)
                    && r.started_secs + START_SKEW_SECS >= tab.spawn
                    && cwd_eq(&r.cwd, &tab.cwd)
            })
            .min_by_key(|r| r.started_secs)
            // A resumed session does not: codex appends to the ORIGINAL rollout
            // with a frozen `session_meta`, so its `started_secs` can predate
            // the pane by weeks and the filter above can never match. What does
            // hold is that the file is being written *now* — so fall back to
            // the most recently touched matching rollout that has grown since
            // this pane spawned.
            //
            // Gated on `resuming`: a fresh pane whose rollout has not appeared
            // yet must stay unpinned and retry, never adopt an old one.
            .or_else(|| {
                if !tab.resuming {
                    return None;
                }
                snapshot
                    .iter()
                    .filter(|r| {
                        !claimed.contains(&r.uuid)
                            && r.mtime_secs >= tab.spawn
                            && cwd_eq(&r.cwd, &tab.cwd)
                    })
                    .max_by_key(|r| r.mtime_secs)
            });
        if let Some(r) = best {
            claimed.insert(r.uuid.clone());
            out.push((tab.idx, r.uuid.clone()));
        }
    }
    out
}

/// Whether a codex tab is recent enough to keep trying to pin: within
/// [`PIN_WINDOW`] of its last output, or of its spawn if it hasn't emitted yet.
///
/// Pure so the spawn-vs-output distinction is testable without a live pane — it is
/// the fix for the wrong-transcript bug and deserves to be pinned by a test rather
/// than inferred from a timer.
fn codex_pin_window_open(
    last_output_at: Option<std::time::Instant>,
    spawn_at: std::time::Instant,
) -> bool {
    last_output_at.unwrap_or(spawn_at).elapsed() < PIN_WINDOW
}

/// Same-directory check tolerating the macOS `/private` symlink, in either
/// direction (the rollout's `session_meta` cwd vs the tab's canonicalized cwd).
fn cwd_eq(session_cwd: &str, tab_cwd: &str) -> bool {
    session_cwd == tab_cwd
        || session_cwd.strip_prefix("/private").unwrap_or(session_cwd) == tab_cwd
        || tab_cwd.strip_prefix("/private").unwrap_or(tab_cwd) == session_cwd
}

impl App {
    /// Whether any codex tab is still unpinned and recently ACTIVE — i.e. worth a
    /// scan.
    ///
    /// The window runs from the pane's last output, not its spawn, and that
    /// distinction is the whole bug it fixes. codex writes its rollout when it first
    /// does something, not when it starts; a spawn-anchored window closed 30s after
    /// launch, so a tab you opened and read for a minute before typing was never
    /// pinned — permanently. `^a v` then fell through to the mtime heuristic, which
    /// hands every pane in a cwd to whichever codex session is *busiest*, including
    /// another spyc instance's. That is exactly how it came to show the wrong
    /// conversation.
    ///
    /// Still quiesces, on two independent conditions: a pinned tab drops out
    /// immediately (`codex_session_id` is set), and an idle one drops out
    /// [`PIN_WINDOW`] after it stops emitting. So a codex that never writes a
    /// rollout costs one scan per output burst while it is active, not forever.
    fn needs_codex_pin(&self) -> bool {
        self.runtime.pane_tabs.as_ref().is_some_and(|tabs| {
            tabs.tabs().iter().any(|e| {
                e.info.codex_session_id.is_none()
                    && codex_pin_window_open(e.info.last_output_at, e.info.spawn_at)
                    && crate::agent::detect(&e.info.command).kind() == AgentKind::Codex
            })
        })
    }

    /// Kick an off-thread `~/.codex/sessions` scan when a codex tab still needs
    /// pinning and none is in flight. The snapshot lands in `codex_pin_pending`
    /// and wakes the loop (`Message::CodexSessionReady`); `apply_codex_session_pins`
    /// does the assignment. The scan reads every rollout's first line, so it runs
    /// OFF the loop — never on the render/input path.
    //
    // `&mut self` is deliberate (the stores go through interior mutability): it
    // is the structural guarantee that the `&self` draw pass can't call this.
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub(crate) fn kick_codex_session_scan(&mut self) {
        if !self.needs_codex_pin() || self.runtime.codex_scan_in_flight.load(Ordering::Acquire) {
            return;
        }
        self.runtime
            .codex_scan_in_flight
            .store(true, Ordering::Release);
        let pending = std::sync::Arc::clone(&self.runtime.codex_pin_pending);
        let flight = std::sync::Arc::clone(&self.runtime.codex_scan_in_flight);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            // Brief wait so a not-yet-written rollout gets a chance to appear and
            // repeated kicks poll at ~SCAN_DELAY rather than spin.
            std::thread::sleep(SCAN_DELAY);
            let snapshot = crate::state::codex_transcript::scan_rollout_metas();
            *pending.lock().unwrap() = Some(snapshot);
            flight.store(false, Ordering::Release);
            if let Some(tx) = wake {
                let _ = tx.send(Message::CodexSessionReady);
            }
        });
    }

    /// Drain a landed rollout snapshot and pin unclaimed sessions onto unpinned
    /// codex tabs (spawn-ordered, shared claimed-set — see [`assign_codex_sessions`]).
    /// Returns `false`: a pin doesn't change the rendered frame, so it never
    /// forces a redraw. Called from the pre-recv scan.
    pub(crate) fn apply_codex_session_pins(&mut self) -> bool {
        let snapshot = {
            let mut slot = self.runtime.codex_pin_pending.lock().unwrap();
            match slot.take() {
                Some(s) => s,
                None => return false,
            }
        };
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            return false;
        };
        let claimed: HashSet<String> = tabs
            .tabs()
            .iter()
            .filter_map(|e| e.info.codex_session_id.clone())
            .collect();
        let mut unpinned: Vec<UnpinnedTab> = tabs
            .tabs()
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.info.codex_session_id.is_none()
                    && crate::agent::detect(&e.info.command).kind() == AgentKind::Codex
            })
            .map(|(i, e)| {
                // Canonicalize once here (impure) so the pure assignment compares
                // plain strings — handles a symlinked pane cwd vs the canonical
                // path codex records in session_meta.
                let cwd = std::fs::canonicalize(&e.info.cwd).map_or_else(
                    |_| e.info.cwd.to_string_lossy().into_owned(),
                    |c| c.to_string_lossy().into_owned(),
                );
                UnpinnedTab {
                    idx: i,
                    cwd,
                    spawn: e.info.spawn_epoch_secs,
                    resuming: crate::state::codex_transcript::is_resume_without_id(&e.info.command),
                }
            })
            .collect();
        unpinned.sort_by_key(|t| t.spawn);
        for (idx, uuid) in assign_codex_sessions(&unpinned, &claimed, &snapshot) {
            if let Some(entry) = tabs.tabs_mut().get_mut(idx) {
                entry.info.codex_session_id = Some(uuid);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rollout whose file was last written at `started` — the fresh-session
    /// shape, where start and last-write coincide closely enough.
    fn meta(uuid: &str, cwd: &str, started: u64) -> RolloutMeta {
        RolloutMeta {
            mtime_secs: started,
            uuid: uuid.to_string(),
            cwd: cwd.to_string(),
            started_secs: started,
        }
    }

    #[test]
    fn assigns_the_earliest_after_spawn_rollout() {
        let snap = vec![
            meta("old", "/repo", 100),   // predates spawn
            meta("mine", "/repo", 1005), // just after spawn
            meta("later", "/repo", 2000),
        ];
        let unpinned = vec![UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1000,
            resuming: false,
        }];
        let out = assign_codex_sessions(&unpinned, &HashSet::new(), &snap);
        assert_eq!(out, vec![(0, "mine".to_string())]);
    }

    #[test]
    fn two_fresh_panes_same_cwd_get_distinct_sessions() {
        // A spawned first (t=1000), B second (t=1010); each wrote its own
        // rollout. Spawn-ordered assignment must give A the earlier, B the later.
        let snap = vec![meta("sessB", "/repo", 1010), meta("sessA", "/repo", 1000)];
        let unpinned = vec![
            UnpinnedTab {
                idx: 0,
                cwd: "/repo".to_string(),
                spawn: 1000,
                resuming: false,
            }, // A
            UnpinnedTab {
                idx: 1,
                cwd: "/repo".to_string(),
                spawn: 1010,
                resuming: false,
            }, // B
        ];
        let out = assign_codex_sessions(&unpinned, &HashSet::new(), &snap);
        assert_eq!(
            out,
            vec![(0, "sessA".to_string()), (1, "sessB".to_string())]
        );
    }

    #[test]
    fn respects_already_claimed_and_cwd() {
        let snap = vec![meta("taken", "/repo", 1001), meta("free", "/repo", 1002)];
        let mut claimed = HashSet::new();
        claimed.insert("taken".to_string());
        let unpinned = vec![UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1000,
            resuming: false,
        }];
        let out = assign_codex_sessions(&unpinned, &claimed, &snap);
        assert_eq!(out, vec![(0, "free".to_string())]);

        // Wrong cwd → nothing to pin.
        let other = vec![UnpinnedTab {
            idx: 0,
            cwd: "/elsewhere".to_string(),
            spawn: 1000,
            resuming: false,
        }];
        assert!(assign_codex_sessions(&other, &HashSet::new(), &snap).is_empty());
    }

    /// A rollout whose `session_meta` is frozen far in the past but which is
    /// being appended to right now — the resumed-session shape.
    fn resumed_meta(uuid: &str, cwd: &str, started: u64, mtime: u64) -> RolloutMeta {
        RolloutMeta {
            uuid: uuid.to_string(),
            cwd: cwd.to_string(),
            started_secs: started,
            mtime_secs: mtime,
        }
    }

    #[test]
    fn a_resumed_session_pins_despite_a_frozen_start_time() {
        // `codex resume --last` appends to the ORIGINAL rollout, so its
        // `started_secs` predates the pane by weeks and the start-time filter
        // can never match. Liveness is the only usable signal.
        let snap = vec![resumed_meta("resumed", "/repo", 1, 2_000)];
        let tab = UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1_000,
            resuming: true,
        };
        assert_eq!(
            assign_codex_sessions(&[tab], &HashSet::new(), &snap),
            vec![(0, "resumed".to_string())],
            "a resumed tab must pin to the rollout it is actively writing"
        );
    }

    #[test]
    fn a_fresh_tab_never_adopts_a_live_old_rollout() {
        // The gate on `resuming`. A fresh `codex` whose own rollout has not
        // appeared yet must stay unpinned and retry — adopting the old one
        // (kept live by some other codex) is exactly the wrong-transcript bug.
        let snap = vec![resumed_meta("someone-elses", "/repo", 1, 2_000)];
        let tab = UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1_000,
            resuming: false,
        };
        assert!(
            assign_codex_sessions(&[tab], &HashSet::new(), &snap).is_empty(),
            "only a resuming tab may fall back to liveness"
        );
    }

    #[test]
    fn two_resuming_panes_do_not_share_one_rollout() {
        // The claimed-set must still hold on the fallback path.
        let snap = vec![
            resumed_meta("older", "/repo", 1, 2_000),
            resumed_meta("newer", "/repo", 2, 2_500),
        ];
        let tabs = vec![
            UnpinnedTab {
                idx: 0,
                cwd: "/repo".to_string(),
                spawn: 1_000,
                resuming: true,
            },
            UnpinnedTab {
                idx: 1,
                cwd: "/repo".to_string(),
                spawn: 1_000,
                resuming: true,
            },
        ];
        let out = assign_codex_sessions(&tabs, &HashSet::new(), &snap);
        assert_eq!(out.len(), 2, "both panes pin");
        assert_ne!(out[0].1, out[1].1, "and to different rollouts");
    }

    #[test]
    fn a_stale_rollout_is_not_adopted_even_when_resuming() {
        // Liveness means "grown since this pane spawned". A rollout last
        // touched before the pane existed is somebody else's history.
        let snap = vec![resumed_meta("stale", "/repo", 1, 500)];
        let tab = UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1_000,
            resuming: true,
        };
        assert!(
            assign_codex_sessions(&[tab], &HashSet::new(), &snap).is_empty(),
            "a rollout that has not grown since spawn is not this pane's"
        );
    }

    #[test]
    fn no_rollout_after_spawn_leaves_unpinned() {
        let snap = vec![meta("stale", "/repo", 500)]; // all predate spawn
        let unpinned = vec![UnpinnedTab {
            idx: 0,
            cwd: "/repo".to_string(),
            spawn: 1000,
            resuming: false,
        }];
        assert!(assign_codex_sessions(&unpinned, &HashSet::new(), &snap).is_empty());
    }

    #[test]
    fn cwd_eq_handles_private_symlink_both_directions() {
        assert!(cwd_eq("/repo", "/repo"));
        assert!(cwd_eq("/private/var/x", "/var/x"));
        assert!(cwd_eq("/var/x", "/private/var/x"));
        assert!(!cwd_eq("/a", "/b"));
    }
    /// **The wrong-transcript bug (#230).** codex writes its rollout when it first
    /// does something, not at spawn. A window anchored to spawn therefore closed on
    /// a tab you opened and read for a while before typing — leaving it permanently
    /// unpinned, so `^a v` fell through to the mtime heuristic and showed whichever
    /// codex session was busiest (possibly another spyc instance's).
    #[test]
    fn the_pin_window_follows_output_not_spawn() {
        use std::time::{Duration, Instant};
        // `checked_sub`: an `Instant` can sit near the monotonic clock's origin on a
        // freshly booted machine, where subtracting would underflow.
        let long_ago = Instant::now()
            .checked_sub(PIN_WINDOW + Duration::from_secs(5))
            .expect("monotonic clock is past PIN_WINDOW in any real run");

        // Spawned well outside the window and never emitted: nothing to pin against
        // yet, and no reason to keep scanning.
        assert!(
            !codex_pin_window_open(None, long_ago),
            "an idle, never-active tab should not hold the scan open"
        );

        // Same old spawn, but it just emitted — its rollout exists NOW, which is
        // precisely when a pin becomes possible. The old spawn-anchored check gave
        // up here, which is the bug.
        assert!(
            codex_pin_window_open(Some(Instant::now()), long_ago),
            "recent output must re-open the window regardless of spawn age"
        );

        // Output that has itself gone stale closes it again, so the scan quiesces
        // instead of running forever against a codex that never writes a rollout.
        let a_window_ago = Instant::now()
            .checked_sub(PIN_WINDOW)
            .expect("monotonic clock is past PIN_WINDOW in any real run");
        assert!(!codex_pin_window_open(Some(long_ago), a_window_ago));
    }

    /// A freshly spawned tab that hasn't emitted yet is still worth scanning — the
    /// common case, and the one the original window was written for.
    #[test]
    fn a_fresh_tab_is_scannable_before_any_output() {
        assert!(codex_pin_window_open(None, std::time::Instant::now()));
    }
}
