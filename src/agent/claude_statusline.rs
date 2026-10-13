//! What spyc keeps of the input Claude Code gives its `statusLine` command: the
//! plan's rate-limit windows and the context window, as Claude Code itself
//! computed them. The cwd, transcript path, cost, model and everything else stay
//! behind; only this projection travels to spyc.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Statusline input past this size isn't projected. It still reaches the
/// wrapped command whole.
pub const MAX_PAYLOAD: usize = 256 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statusline {
    /// The conversation the numbers describe, checked against the tab's own.
    pub session_id: Option<String>,
    pub context: Option<ContextWindow>,
    pub five_hour: Option<RateWindow>,
    pub seven_day: Option<RateWindow>,
}

impl Statusline {
    pub const fn is_empty(&self) -> bool {
        self.context.is_none() && self.five_hour.is_none() && self.seven_day.is_none()
    }

    /// The bounds [`project`] holds an input to, for one that arrived already
    /// projected: whatever sent it is trusted no further than that.
    #[must_use]
    pub fn bounded(self) -> Self {
        let window = |w: Option<RateWindow>| {
            w.filter(|w| w.used_percent <= 100).map(|w| RateWindow {
                resets_at: w.resets_at.filter(|at| *at > 0),
                ..w
            })
        };
        Self {
            session_id: self
                .session_id
                .filter(|id| crate::state::sessions::is_uuid(id)),
            context: self
                .context
                .filter(|c| (1..=MAX_WINDOW).contains(&c.size))
                .map(|c| ContextWindow {
                    used_percent: c.used_percent.filter(|p| *p <= 100),
                    ..c
                }),
            five_hour: window(self.five_hour),
            seven_day: window(self.seven_day),
        }
    }
}

/// No model's window is bigger than this.
const MAX_WINDOW: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextWindow {
    /// The model's window, in tokens.
    pub size: u64,
    /// Claude Code's own share of the window in use; `None` before a request.
    pub used_percent: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateWindow {
    pub used_percent: u8,
    /// When the window resets, in Unix seconds.
    pub resets_at: Option<i64>,
}

/// The projection of one statusline input. Absent, malformed and out-of-range
/// fields are dropped, never guessed.
pub fn project(payload: &Value) -> Statusline {
    let context = &payload["context_window"];
    let limits = &payload["rate_limits"];
    Statusline {
        session_id: payload["session_id"]
            .as_str()
            .filter(|id| crate::state::sessions::is_uuid(id))
            .map(str::to_owned),
        context: context["context_window_size"]
            .as_u64()
            .filter(|size| (1..=MAX_WINDOW).contains(size))
            .map(|size| ContextWindow {
                size,
                used_percent: percent(&context["used_percentage"]),
            }),
        five_hour: window(&limits["five_hour"]),
        seven_day: window(&limits["seven_day"]),
    }
}

/// [`project`] over raw input, empty when it isn't a JSON object or is too big.
pub fn project_bytes(input: &[u8]) -> Statusline {
    if input.len() > MAX_PAYLOAD {
        return Statusline::default();
    }
    serde_json::from_slice::<Value>(input)
        .ok()
        .filter(Value::is_object)
        .map(|payload| project(&payload))
        .unwrap_or_default()
}

fn window(value: &Value) -> Option<RateWindow> {
    Some(RateWindow {
        used_percent: percent(&value["used_percentage"])?,
        resets_at: resets_at(&value["resets_at"]),
    })
}

/// A finite share, rounded and held to 0–100.
fn percent(value: &Value) -> Option<u8> {
    let share = value.as_f64().filter(|share| share.is_finite())?;
    // The clamp keeps the cast in range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(share.round().clamp(0.0, 100.0) as u8)
}

/// Unix seconds, milliseconds or an RFC 3339 time, as seconds.
fn resets_at(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        let secs = if n > 1_000_000_000_000 { n / 1000 } else { n };
        return (secs > 0).then_some(secs);
    }
    value
        .as_str()?
        .parse::<Timestamp>()
        .ok()
        .map(Timestamp::as_second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SESSION: &str = "c1a0de00-0000-4000-8000-000000000001";

    /// The fields Claude Code 2.1.296's statusline schema documents, beside
    /// the ones spyc must never carry.
    fn input() -> Value {
        json!({
            "session_id": SESSION,
            "transcript_path": "/PRIVATE/PATH/SENTINEL.jsonl",
            "cwd": "/PRIVATE/CWD/SENTINEL",
            "model": {"id": "claude-opus-5-5", "display_name": "PRIVATE_MODEL_SENTINEL"},
            "cost": {"total_cost_usd": 12.5},
            "context_window": {
                "total_input_tokens": 336_600,
                "context_window_size": 1_000_000,
                "current_usage": {"input_tokens": 6},
                "used_percentage": 33.66,
                "remaining_percentage": 66.34
            },
            "rate_limits": {
                "five_hour": {"used_percentage": 23.4, "resets_at": 1_791_900_000},
                "seven_day": {"used_percentage": 61, "resets_at": "2026-10-16T09:00:00Z"}
            }
        })
    }

    #[test]
    fn the_projection_keeps_the_windows_and_nothing_private() {
        let projected = project(&input());
        assert_eq!(projected.session_id.as_deref(), Some(SESSION));
        assert_eq!(
            projected.context,
            Some(ContextWindow {
                size: 1_000_000,
                used_percent: Some(34)
            })
        );
        assert_eq!(
            projected.five_hour,
            Some(RateWindow {
                used_percent: 23,
                resets_at: Some(1_791_900_000)
            })
        );
        let seven = projected.seven_day.unwrap();
        assert_eq!(seven.used_percent, 61);
        assert_eq!(
            seven.resets_at,
            Some(
                "2026-10-16T09:00:00Z"
                    .parse::<Timestamp>()
                    .unwrap()
                    .as_second()
            )
        );
        let wire = serde_json::to_string(&projected).unwrap();
        assert!(
            !wire.contains("SENTINEL") && !wire.contains("12.5"),
            "{wire}"
        );
    }

    #[test]
    fn absent_or_malformed_fields_are_dropped_never_guessed() {
        let mut payload = input();
        payload["context_window"]["used_percentage"] = Value::Null;
        payload["rate_limits"]["five_hour"]["used_percentage"] = json!("23");
        payload["rate_limits"]["seven_day"]["used_percentage"] = json!(140.0);
        payload["rate_limits"]["seven_day"]["resets_at"] = json!(1_791_900_000_123_i64);
        payload["session_id"] = json!("../../etc");
        let projected = project(&payload);
        assert_eq!(projected.context.unwrap().used_percent, None);
        assert_eq!(projected.five_hour, None, "a string is not a share");
        assert_eq!(
            projected.seven_day.unwrap().used_percent,
            100,
            "held to 0–100"
        );
        assert_eq!(projected.seven_day.unwrap().resets_at, Some(1_791_900_000));
        assert_eq!(projected.session_id, None);
        payload["context_window"]["context_window_size"] = json!(0);
        assert_eq!(project(&payload).context, None);
        assert!(project_bytes(b"[1,2]").is_empty());
        assert!(project_bytes(b"not json").is_empty());
        let huge = vec![b' '; MAX_PAYLOAD + 1];
        assert!(project_bytes(&huge).is_empty());
        assert!(!project_bytes(input().to_string().as_bytes()).is_empty());
    }

    #[test]
    fn a_report_from_the_wire_is_held_to_the_same_bounds() {
        let wire = Statusline {
            session_id: Some("not a uuid".into()),
            context: Some(ContextWindow {
                size: MAX_WINDOW + 1,
                used_percent: Some(5),
            }),
            five_hour: Some(RateWindow {
                used_percent: 200,
                resets_at: Some(5),
            }),
            seven_day: Some(RateWindow {
                used_percent: 7,
                resets_at: Some(-1),
            }),
        }
        .bounded();
        assert_eq!(wire.session_id, None);
        assert_eq!(wire.context, None);
        assert_eq!(wire.five_hour, None);
        assert_eq!(
            wire.seven_day,
            Some(RateWindow {
                used_percent: 7,
                resets_at: None
            })
        );
        assert_eq!(
            project(&input()).bounded(),
            project(&input()),
            "a real one survives"
        );
    }
}
