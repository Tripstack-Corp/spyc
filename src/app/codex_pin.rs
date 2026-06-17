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
fn assign_codex_sessions(
    unpinned: &[(usize, String, u64)],
    already_claimed: &HashSet<String>,
    snapshot: &[RolloutMeta],
) -> Vec<(usize, String)> {
    let mut claimed = already_claimed.clone();
    let mut out = Vec::new();
    for (idx, cwd, spawn) in unpinned {
        let best = snapshot
            .iter()
            .filter(|r| {
                !claimed.contains(&r.uuid)
                    && r.started_secs + START_SKEW_SECS >= *spawn
                    && cwd_eq(&r.cwd, cwd)
            })
            .min_by_key(|r| r.started_secs);
        if let Some(r) = best {
            claimed.insert(r.uuid.clone());
            out.push((*idx, r.uuid.clone()));
        }
    }
    out
}

/// Same-directory check tolerating the macOS `/private` symlink, in either
/// direction (the rollout's `session_meta` cwd vs the tab's canonicalized cwd).
fn cwd_eq(session_cwd: &str, tab_cwd: &str) -> bool {
    session_cwd == tab_cwd
        || session_cwd.strip_prefix("/private").unwrap_or(session_cwd) == tab_cwd
        || tab_cwd.strip_prefix("/private").unwrap_or(tab_cwd) == session_cwd
}

impl App {
    /// Whether any codex tab is still unpinned and recently spawned — i.e. worth
    /// a scan. Past [`PIN_WINDOW`] we stop (the resolver heuristic takes over),
    /// so the scan loop naturally quiesces once codex has written its rollout.
    fn needs_codex_pin(&self) -> bool {
        self.runtime.pane_tabs.as_ref().is_some_and(|tabs| {
            tabs.tabs().iter().any(|e| {
                e.info.codex_session_id.is_none()
                    && e.info.spawn_at.elapsed() < PIN_WINDOW
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
        let mut unpinned: Vec<(usize, String, u64)> = tabs
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
                (i, cwd, e.info.spawn_epoch_secs)
            })
            .collect();
        unpinned.sort_by_key(|(_, _, spawn)| *spawn);
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

    fn meta(uuid: &str, cwd: &str, started: u64) -> RolloutMeta {
        RolloutMeta {
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
        let unpinned = vec![(0usize, "/repo".to_string(), 1000u64)];
        let out = assign_codex_sessions(&unpinned, &HashSet::new(), &snap);
        assert_eq!(out, vec![(0, "mine".to_string())]);
    }

    #[test]
    fn two_fresh_panes_same_cwd_get_distinct_sessions() {
        // A spawned first (t=1000), B second (t=1010); each wrote its own
        // rollout. Spawn-ordered assignment must give A the earlier, B the later.
        let snap = vec![meta("sessB", "/repo", 1010), meta("sessA", "/repo", 1000)];
        let unpinned = vec![
            (0usize, "/repo".to_string(), 1000u64), // A
            (1usize, "/repo".to_string(), 1010u64), // B
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
        let unpinned = vec![(0usize, "/repo".to_string(), 1000u64)];
        let out = assign_codex_sessions(&unpinned, &claimed, &snap);
        assert_eq!(out, vec![(0, "free".to_string())]);

        // Wrong cwd → nothing to pin.
        let other = vec![(0usize, "/elsewhere".to_string(), 1000u64)];
        assert!(assign_codex_sessions(&other, &HashSet::new(), &snap).is_empty());
    }

    #[test]
    fn no_rollout_after_spawn_leaves_unpinned() {
        let snap = vec![meta("stale", "/repo", 500)]; // all predate spawn
        let unpinned = vec![(0usize, "/repo".to_string(), 1000u64)];
        assert!(assign_codex_sessions(&unpinned, &HashSet::new(), &snap).is_empty());
    }

    #[test]
    fn cwd_eq_handles_private_symlink_both_directions() {
        assert!(cwd_eq("/repo", "/repo"));
        assert!(cwd_eq("/private/var/x", "/var/x"));
        assert!(cwd_eq("/var/x", "/private/var/x"));
        assert!(!cwd_eq("/a", "/b"));
    }
}
