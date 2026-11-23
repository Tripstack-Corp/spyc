//! What spyc keeps of the input Antigravity (`agy`) gives its `statusLine`
//! command: the model and effort, context flags, multi-bucket quota windows,
//! plan tier, approval mode, sandbox state, live subagents, background task
//! count and queued prompt count. The email, cwd, transcript path, title, cost
//! and subagent role text stay behind; only this bounded projection travels to
//! spyc.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// At most this many quota buckets are kept from one statusline report.
pub const MAX_QUOTAS: usize = 8;
/// At most this many subagents are kept from one statusline report.
pub const MAX_SUBAGENTS: usize = 16;
/// No quota window lasts a year; a reset further out is ignored.
const MAX_RESET_SECS: f64 = (366 * 86_400) as f64;
const MAX_COUNT: u32 = 10_000;
const FIVE_HOURS: i64 = 5 * 3_600;
const SEVEN_DAYS: i64 = 7 * 86_400;

pub const ARGON_BUCKET: &str = "gemini-argon-wtu-weekly";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxState {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_network: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaBucket {
    pub key: String,
    /// Used share in basis points (`0..=10_000`, i.e. `0.00%`–`100.00%`).
    pub used_percent_x100: u16,
    /// When the bucket resets, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaGroup {
    Gemini,
    Argon,
    ThirdParty,
}

impl QuotaBucket {
    /// Rounded whole-percent share used (`0..=100`).
    #[must_use]
    pub fn used_percent(&self) -> u8 {
        let rounded = (u32::from(self.used_percent_x100.min(10_000)) + 50) / 100;
        u8::try_from(rounded.min(100)).unwrap_or(100)
    }

    /// Remaining share in basis points (`0..=10_000`).
    #[must_use]
    pub const fn remaining_percent_x100(&self) -> u16 {
        10_000u16.saturating_sub(self.used_percent_x100)
    }

    /// Known window length for pace calculations, inferred from `key`.
    #[must_use]
    pub fn window_secs(&self) -> Option<i64> {
        match self.key.as_str() {
            "gemini-5h" | "3p-5h" => Some(FIVE_HOURS),
            "gemini-weekly" | "3p-weekly" | ARGON_BUCKET => Some(SEVEN_DAYS),
            _ => None,
        }
    }

    /// Which model family this bucket counts.
    #[must_use]
    pub fn group(&self) -> Option<QuotaGroup> {
        if self.key.starts_with("gemini-argon-") {
            Some(QuotaGroup::Argon)
        } else if self.key.starts_with("gemini-") {
            Some(QuotaGroup::Gemini)
        } else if self.key.starts_with("3p-") {
            Some(QuotaGroup::ThirdParty)
        } else {
            None
        }
    }

    /// Whether this bucket is a 5-hour window rather than a 7-day window.
    #[must_use]
    pub fn is_five_hour(&self) -> bool {
        self.key.ends_with("-5h")
    }
}

/// The quota buckets relevant to the model currently running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveQuotas<'a> {
    pub five_hour: Option<&'a QuotaBucket>,
    pub seven_day: Option<&'a QuotaBucket>,
    /// Separate Google AI Pro Argon weekly bucket when it isn't already `seven_day`.
    pub argon: Option<&'a QuotaBucket>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgySubagent {
    pub name: String,
    #[serde(default)]
    pub running: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgyStatusline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub exceeds_200k: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quotas: Vec<QuotaBucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subagents: Vec<AgySubagent>,
    #[serde(default)]
    pub task_count: u32,
    #[serde(default)]
    pub pending_input_count: u32,
}

impl AgyStatusline {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.effort.is_none()
            && !self.exceeds_200k
            && self.plan_tier.is_none()
            && self.approval_mode.is_none()
            && self.sandbox.is_none()
            && self.active_agent.is_none()
            && self.quotas.is_empty()
            && self.subagents.is_empty()
            && self.task_count == 0
            && self.pending_input_count == 0
    }

    /// Enforce the same bounds on a report received from the wire.
    #[must_use]
    pub fn bounded(self) -> Self {
        let mut quotas: Vec<QuotaBucket> = self
            .quotas
            .into_iter()
            .filter_map(|mut q| {
                q.key = label(&q.key, 40)?;
                if q.used_percent_x100 > 10_000 {
                    return None;
                }
                q.resets_at = q.resets_at.filter(|at| *at > 0);
                Some(q)
            })
            .collect();
        quotas.sort_by(|a, b| a.key.cmp(&b.key));
        quotas.truncate(MAX_QUOTAS);
        let mut subagents: Vec<AgySubagent> = self
            .subagents
            .into_iter()
            .filter_map(|mut s| {
                s.name = label(&s.name, 32)?;
                Some(s)
            })
            .collect();
        subagents.sort_by_key(|s| !s.running);
        subagents.truncate(MAX_SUBAGENTS);
        Self {
            model: self.model.and_then(|s| label(&s, 80)),
            effort: self.effort.and_then(|s| label(&s, 16)),
            exceeds_200k: self.exceeds_200k,
            plan_tier: self.plan_tier.and_then(|s| label(&s, 40)),
            approval_mode: self.approval_mode.and_then(|s| label(&s, 24)),
            sandbox: self.sandbox,
            active_agent: self.active_agent.and_then(|s| label(&s, 40)),
            quotas,
            subagents,
            task_count: self.task_count.min(MAX_COUNT),
            pending_input_count: self.pending_input_count.min(MAX_COUNT),
        }
    }

    /// Pick the 5-hour and 7-day quota buckets governing `model` (falling back
    /// to this report's own `model` when `None`), plus Google AI Pro's separate
    /// Argon weekly bucket when present and not already shown as `seven_day`.
    #[must_use]
    pub fn active_quotas(&self, model: Option<&str>) -> ActiveQuotas<'_> {
        let usable: Vec<&QuotaBucket> = self.quotas.iter().filter(|q| !q.disabled).collect();
        if usable.is_empty() {
            return ActiveQuotas::default();
        }
        let find = |key: &str| usable.iter().copied().find(|q| q.key == key);
        let model_lower = model.or(self.model.as_deref()).unwrap_or("").to_lowercase();
        let argon_bucket = find(ARGON_BUCKET);
        let gemini_5h = find("gemini-5h");
        let gemini_wk = find("gemini-weekly");
        let tp_5h = find("3p-5h");
        let tp_wk = find("3p-weekly");

        let (five_hour, seven_day, argon) = if model_lower.contains("orchestrator") {
            (gemini_5h, gemini_wk, argon_bucket)
        } else if model_lower.contains("argon") {
            match argon_bucket {
                Some(argon) => (None, Some(argon), None),
                None => (gemini_5h, gemini_wk, None),
            }
        } else if model_lower.is_empty()
            || ["gemini", "hydrogen", "neon", "carbon", "flash"]
                .iter()
                .any(|kw| model_lower.contains(kw))
        {
            (gemini_5h, gemini_wk, argon_bucket)
        } else if tp_5h.is_some() || tp_wk.is_some() {
            (tp_5h, tp_wk, argon_bucket)
        } else {
            (gemini_5h, gemini_wk, argon_bucket)
        };

        ActiveQuotas {
            five_hour,
            seven_day,
            argon,
        }
    }
}

/// Whether `payload` carries Antigravity-specific statusline fields (so a
/// Claude Code payload — which also documents `exceeds_200k_tokens` — is never
/// misread as Antigravity).
pub fn is_agy_payload(payload: &Value) -> bool {
    payload["product"].as_str() == Some("antigravity") || payload["quota"].is_object()
}

/// Project one Antigravity `statusLine` payload at `now_secs` (Unix seconds,
/// used only when a quota bucket gives `reset_in_seconds` without `reset_time`).
pub fn project_at(payload: &Value, now_secs: i64) -> Option<AgyStatusline> {
    if !is_agy_payload(payload) {
        return None;
    }
    let model = payload["model"]["display_name"]
        .as_str()
        .or_else(|| payload["model"]["id"].as_str())
        .and_then(|s| label(s, 80));
    let effort = payload["model"]["effort"]
        .as_str()
        .and_then(|s| label(s, 16));
    let exceeds_200k = payload["exceeds_200k_tokens"].as_bool().unwrap_or(false);
    let plan_tier = payload["plan_tier"].as_str().and_then(|s| label(s, 40));
    let approval_mode = payload["cycle_mode"].as_str().and_then(|s| label(s, 24));
    let sandbox = payload["sandbox"].as_object().and_then(|obj| {
        let enabled = obj.get("enabled")?.as_bool()?;
        let allow_network = obj.get("allow_network").and_then(Value::as_bool);
        Some(SandboxState {
            enabled,
            allow_network,
        })
    });
    let active_agent = payload["agent"]["name"].as_str().and_then(|s| label(s, 40));
    let quotas = project_quotas(&payload["quota"], now_secs);
    let subagents = project_subagents(&payload["subagents"]);
    let task_count = bounded_count(&payload["task_count"]);
    let pending_input_count = bounded_count(&payload["pending_input_count"]);
    let projected = AgyStatusline {
        model,
        effort,
        exceeds_200k,
        plan_tier,
        approval_mode,
        sandbox,
        active_agent,
        quotas,
        subagents,
        task_count,
        pending_input_count,
    };
    (!projected.is_empty()).then_some(projected)
}

fn project_quotas(value: &Value, now_secs: i64) -> Vec<QuotaBucket> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut quotas: Vec<QuotaBucket> = map
        .iter()
        .filter_map(|(key, entry)| {
            let key = label(key, 40)?;
            let remaining = entry["remaining_fraction"]
                .as_f64()
                .filter(|v| v.is_finite())?
                .clamp(0.0, 1.0);
            // Clamped to 0.0..=10_000.0 so the cast is in range.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let used_percent_x100 =
                ((1.0 - remaining) * 10_000.0).round().clamp(0.0, 10_000.0) as u16;
            let resets_at = quota_reset(entry, now_secs);
            let disabled = entry["disabled"].as_bool().unwrap_or(false);
            Some(QuotaBucket {
                key,
                used_percent_x100,
                resets_at,
                disabled,
            })
        })
        .collect();
    quotas.sort_by(|a, b| a.key.cmp(&b.key));
    quotas.truncate(MAX_QUOTAS);
    quotas
}

fn quota_reset(entry: &Value, now_secs: i64) -> Option<i64> {
    if let Some(secs) = entry["reset_time"]
        .as_str()
        .and_then(|s| s.parse::<Timestamp>().ok())
        .map(Timestamp::as_second)
        .filter(|secs| *secs > 0)
    {
        return Some(secs);
    }
    let in_secs = entry["reset_in_seconds"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= MAX_RESET_SECS)?;
    #[allow(clippy::cast_possible_truncation)]
    let delta = in_secs.round() as i64;
    now_secs.checked_add(delta).filter(|secs| *secs > 0)
}

fn project_subagents(value: &Value) -> Vec<AgySubagent> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<AgySubagent> = list
        .iter()
        .filter_map(|item| {
            let name = item["name"].as_str().and_then(|s| label(s, 32))?;
            let running = item["status"].as_str() == Some("running");
            Some(AgySubagent { name, running })
        })
        .collect();
    // Running subagents come first so truncation never drops a live one.
    out.sort_by_key(|s| !s.running);
    out.truncate(MAX_SUBAGENTS);
    out
}

fn bounded_count(value: &Value) -> u32 {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n.min(u64::from(MAX_COUNT))).ok())
        .unwrap_or(0)
}

fn label(value: &str, max_len: usize) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()
        && trimmed.len() <= max_len
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:/ ()-".contains(&b)))
    .then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_payload() -> Value {
        json!({
            "model": {
                "id": "Gemini 4 Hydrogen (High)",
                "display_name": "Gemini 4 Hydrogen (High)",
                "effort": "high"
            },
            "version": "1.5.14",
            "product": "antigravity",
            "agent_state": "working",
            "plan_tier": "Google AI Pro",
            "cwd": "/PRIVATE/CWD/SENTINEL",
            "email": "PRIVATE_EMAIL@example.com",
            "conversation_title": "PRIVATE_TITLE_SENTINEL",
            "transcript_path": "/PRIVATE/PATH/SENTINEL.pb",
            "cost": { "total_usd": 1.23 },
            "exceeds_200k_tokens": false,
            "quota": {
                "3p-5h": {
                    "remaining_fraction": 1,
                    "reset_time": "1970-01-01T00:00:00.000Z",
                    "reset_in_seconds": 0,
                    "disabled": true
                },
                "3p-weekly": {
                    "remaining_fraction": 0,
                    "reset_time": "2026-11-28T12:30:20.000Z",
                    "reset_in_seconds": 508_371
                },
                "gemini-5h": {
                    "remaining_fraction": 0.750_473_8,
                    "reset_time": "2026-11-22T16:27:31.000Z",
                    "reset_in_seconds": 4_202
                },
                "gemini-argon-wtu-weekly": {
                    "remaining_fraction": 0.081_787_6,
                    "reset_time": "2026-11-28T12:30:27.000Z",
                    "reset_in_seconds": 508_378
                },
                "gemini-weekly": {
                    "remaining_fraction": 0.516_439_8,
                    "reset_time": "2026-11-28T12:30:11.000Z",
                    "reset_in_seconds": 508_362
                }
            },
            "sandbox": { "enabled": true, "allow_network": false },
            "cycle_mode": "accept-edits",
            "subagents": [
                { "name": "done-worker", "role": "PRIVATE_ROLE_SENTINEL", "status": "completed" },
                { "name": "research", "role": "PRIVATE_ROLE_SENTINEL", "status": "running" }
            ],
            "task_count": 2,
            "pending_input_count": 1
        })
    }

    #[test]
    fn projects_agy_fields_and_keeps_private_ones_out() {
        let agy = project_at(&sample_payload(), 1_795_000_000).expect("agy payload");
        assert_eq!(agy.model.as_deref(), Some("Gemini 4 Hydrogen (High)"));
        assert_eq!(agy.effort.as_deref(), Some("high"));
        assert_eq!(agy.plan_tier.as_deref(), Some("Google AI Pro"));
        assert_eq!(agy.approval_mode.as_deref(), Some("accept-edits"));
        assert_eq!(
            agy.sandbox,
            Some(SandboxState {
                enabled: true,
                allow_network: Some(false),
            })
        );
        assert_eq!(agy.task_count, 2);
        assert_eq!(agy.pending_input_count, 1);
        assert_eq!(agy.quotas.len(), 5);
        // Running subagent is ordered ahead of completed ones.
        assert_eq!(
            agy.subagents,
            vec![
                AgySubagent {
                    name: "research".into(),
                    running: true,
                },
                AgySubagent {
                    name: "done-worker".into(),
                    running: false,
                },
            ]
        );
        let wire = serde_json::to_string(&agy).unwrap();
        assert!(!wire.contains("SENTINEL"), "{wire}");
        assert!(!wire.contains("PRIVATE_EMAIL"), "{wire}");
        assert!(!wire.contains("1.23"), "{wire}");
    }

    #[test]
    fn active_quotas_follow_the_model_family_and_ignore_disabled_buckets() {
        let agy = project_at(&sample_payload(), 1_795_000_000).unwrap();

        let gemini = agy.active_quotas(Some("Gemini 4 Hydrogen (High)"));
        assert_eq!(gemini.five_hour.unwrap().key, "gemini-5h");
        assert_eq!(gemini.five_hour.unwrap().used_percent_x100, 2495);
        assert_eq!(gemini.five_hour.unwrap().used_percent(), 25);
        assert_eq!(gemini.seven_day.unwrap().key, "gemini-weekly");
        assert_eq!(gemini.seven_day.unwrap().used_percent(), 48);
        assert_eq!(gemini.argon.unwrap().key, ARGON_BUCKET);
        assert_eq!(gemini.argon.unwrap().used_percent(), 92);

        let argon = agy.active_quotas(Some("Gemini 4 Argon (High)"));
        assert!(argon.five_hour.is_none());
        assert_eq!(argon.seven_day.unwrap().key, ARGON_BUCKET);
        assert!(argon.argon.is_none());

        // 3p-5h is disabled in the fixture, so only 3p-weekly is surfaced.
        let claude = agy.active_quotas(Some("Claude Sonnet 4.5"));
        assert!(claude.five_hour.is_none());
        assert_eq!(claude.seven_day.unwrap().key, "3p-weekly");
        assert_eq!(claude.seven_day.unwrap().used_percent(), 100);
        assert_eq!(claude.argon.unwrap().key, ARGON_BUCKET);
    }

    #[test]
    fn epoch_zero_reset_is_dropped_and_relative_reset_fallback_works() {
        let payload = json!({
            "product": "antigravity",
            "quota": {
                "gemini-5h": {
                    "remaining_fraction": 0.5,
                    "reset_time": "1970-01-01T00:00:00.000Z",
                    "reset_in_seconds": 3600
                },
                "gemini-weekly": {
                    "remaining_fraction": 0.25,
                    "reset_time": "1970-01-01T00:00:00.000Z",
                    "reset_in_seconds": 0
                }
            }
        });
        let agy = project_at(&payload, 1_795_000_000).unwrap();
        let five = agy.quotas.iter().find(|q| q.key == "gemini-5h").unwrap();
        assert_eq!(five.resets_at, Some(1_795_003_600));
        let weekly = agy
            .quotas
            .iter()
            .find(|q| q.key == "gemini-weekly")
            .unwrap();
        assert_eq!(weekly.resets_at, None);
    }

    #[test]
    fn claude_payload_is_not_misread_as_agy() {
        let claude = json!({
            "session_id": "c1a0de00-0000-4000-8000-000000000001",
            "model": { "id": "claude-opus-5-5", "display_name": "PRIVATE_MODEL_SENTINEL" },
            "context_window": { "context_window_size": 1_000_000, "used_percentage": 33.66 },
            "exceeds_200k_tokens": false,
            "rate_limits": { "five_hour": { "used_percentage": 23.4 } }
        });
        assert!(!is_agy_payload(&claude));
        assert!(project_at(&claude, 1_795_000_000).is_none());
    }

    #[test]
    fn bounded_sorts_quotas_before_truncating_like_project_quotas() {
        let mut raw = AgyStatusline::default();
        for key in [
            "z-1", "y-1", "x-1", "w-1", "v-1", "u-1", "t-1", "s-1", "a-1",
        ] {
            raw.quotas.push(QuotaBucket {
                key: key.into(),
                used_percent_x100: 1000,
                resets_at: None,
                disabled: false,
            });
        }
        let bounded = raw.bounded();
        assert_eq!(bounded.quotas.len(), MAX_QUOTAS);
        assert_eq!(bounded.quotas[0].key, "a-1");
        assert!(bounded.quotas.iter().all(|q| q.key != "z-1"));
    }
}
