//! Pure Antigravity (`agy`) Orchestrator helpers: detecting outer Orchestrator
//! wrappers, formatting concrete `PLANNER_RESPONSE.model` slugs, and
//! classifying Planner vs Executor roles from the outer wrapper without
//! guessing from model-family heuristics.
//!
//! Only `transcript.jsonl`'s latest `PLANNER_RESPONSE.model` feeds the active
//! step model, and only while the session's outer model is an Orchestrator
//! wrapper (`Orchestrator (Carbon)`, `Orchestrator (Argon)`, bare
//! `Orchestrator`, or `gemini-pro-orchestrator*`). When the outer name carries
//! a parenthesized planner, a concrete step matching that planner is `plan`
//! and any other concrete step is `exec`; when the outer name names no
//! planner, no role is shown (except for explicit `*-worker` / `(Orchestrator)`
//! worker slugs, which are always `exec`).

/// The role an underlying Orchestrator step model is playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Planner,
    Executor,
}

impl Role {
    /// Compact label for the activity row (`plan` / `exec`).
    #[must_use]
    pub const fn short(self) -> &'static str {
        match self {
            Self::Planner => "plan",
            Self::Executor => "exec",
        }
    }

    /// Full label for `:agent hud dump`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Executor => "executor",
        }
    }
}

/// Resolved active underlying model for an Orchestrator session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedActiveModel {
    pub raw: String,
    pub display: String,
    pub compact: String,
    pub role: Option<Role>,
}

/// Whether `name` is a named Orchestrator worker model (such as
/// `gemini-flash-worker-agy` or `Gemini 4 Hydrogen (Orchestrator)`), which is
/// a concrete execution model rather than the outer wrapper.
fn is_orchestrator_worker(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    if has_worker_slug_token(trimmed) {
        return true;
    }
    strip_concrete_orchestrator_suffix(trimmed).is_some()
}

/// Whether `name` is an outer Orchestrator wrapper label (e.g.
/// `Orchestrator (Carbon)`, `Orchestrator (Argon)`, bare `Orchestrator`,
/// `gemini-pro-orchestrator-g1-pro`) rather than a concrete single or worker
/// model.
#[must_use]
pub fn is_orchestrator_wrapper(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || is_orchestrator_worker(trimmed) {
        return false;
    }
    trimmed.to_ascii_lowercase().contains("orchestrator")
}

/// Whether `name` is an empty or generic placeholder (`auto`) rather than a
/// concrete model name.
fn is_placeholder_model(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto")
}

/// Whether `name` identifies a concrete model that actually executed a step.
#[must_use]
pub fn is_concrete_step_model(name: &str) -> bool {
    !is_placeholder_model(name) && !is_orchestrator_wrapper(name)
}

/// Format a runtime model slug or display name into a readable label
/// (`gemini-4-hydrogen-high` → `Gemini 4 Hydrogen (High)`).
fn format_model_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(stripped) = strip_concrete_orchestrator_suffix(trimmed) {
        return stripped.to_owned();
    }
    if trimmed.contains(char::is_whitespace) || trimmed.contains(['(', ')']) {
        return trimmed.to_owned();
    }
    if trimmed.to_ascii_uppercase().starts_with("MODEL_") || trimmed.contains('_') {
        return trimmed.to_owned();
    }
    if !trimmed.contains('-') {
        return format_token(trimmed);
    }
    format_slug(trimmed)
}

/// Shorten a formatted model name so `Orch → plan <name>` / `Orch → exec <name>`
/// fits inside the 32-column activity-row cell without clipping the effort.
fn compact_model_name(display: &str) -> String {
    let trimmed = display.trim();
    if let Some(rest) = trimmed.strip_prefix("Gemini ") {
        let mut parts = rest.splitn(2, ' ');
        if let (Some(first), Some(tail)) = (parts.next(), parts.next()) {
            // Drop single-digit generation prefix (`Gemini 4 Carbon` → `Carbon`)
            // while keeping dotted versions (`Gemini 3.8 Flash` → `3.8 Flash`).
            if first.bytes().all(|b| b.is_ascii_digit()) {
                return tail.to_owned();
            }
            return rest.to_owned();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("Claude ") {
        return rest.to_owned();
    }
    trimmed.to_owned()
}

/// Classify an active concrete step model as Planner or Executor.
///
/// Explicit worker models are always `Executor`. Otherwise, when `outer_model`
/// names a parenthesized planner (e.g. `Orchestrator (Carbon)`), a step
/// matching that planner is `Planner` and any other concrete step is
/// `Executor`. When `outer_model` names no planner (bare `Orchestrator`),
/// returns `None` rather than guessing from model names.
fn classify_role(raw_model: &str, outer_model: &str) -> Option<Role> {
    if !is_concrete_step_model(raw_model) {
        return None;
    }
    if is_orchestrator_worker(raw_model) {
        return Some(Role::Executor);
    }
    let planner_hint = outer_planner_hint(outer_model)?;
    let hint_tokens = word_tokens(planner_hint);
    if hint_tokens.is_empty() {
        return None;
    }
    let step_tokens = word_tokens(raw_model);
    if hint_tokens.iter().all(|t| step_tokens.contains(t)) {
        Some(Role::Planner)
    } else {
        Some(Role::Executor)
    }
}

/// Resolve the active underlying model for an Orchestrator session from the
/// transcript's latest concrete `PLANNER_RESPONSE.model`.
///
/// Returns `None` when `outer_model` is not an Orchestrator wrapper or when no
/// concrete routed step has been observed since the latest model switch.
#[must_use]
pub fn resolve_active_model(
    outer_model: Option<&str>,
    transcript_model: Option<&str>,
) -> Option<ResolvedActiveModel> {
    let outer = outer_model.filter(|m| is_orchestrator_wrapper(m))?;
    let raw = transcript_model?.trim();
    if !is_concrete_step_model(raw) {
        return None;
    }
    let display = format_model_name(raw);
    let compact = compact_model_name(&display);
    let role = classify_role(raw, outer);
    Some(ResolvedActiveModel {
        raw: raw.to_owned(),
        display,
        compact,
        role,
    })
}

fn has_worker_slug_token(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let needle = b"-worker";
    bytes.windows(needle.len()).enumerate().any(|(i, w)| {
        w == needle
            && bytes
                .get(i + needle.len())
                .is_none_or(|b| !b.is_ascii_alphanumeric())
    })
}

/// Strips a trailing `(<spaces>orchestrator<spaces>)` suffix when a concrete
/// model name precedes it (`Gemini 4 Hydrogen (Orchestrator)` → `Some("Gemini 4 Hydrogen")`).
fn strip_concrete_orchestrator_suffix(trimmed: &str) -> Option<&str> {
    let without_close = trimmed.strip_suffix(')')?;
    let (prefix, inner) = without_close.rsplit_once('(')?;
    if !inner.trim().eq_ignore_ascii_case("orchestrator") {
        return None;
    }
    let base = prefix.trim();
    if base.is_empty() || base.to_ascii_lowercase().starts_with("orchestrator") {
        return None;
    }
    Some(base)
}

/// Extracts the parenthesized planner hint from `Orchestrator (<hint>)`.
fn outer_planner_hint(outer: &str) -> Option<&str> {
    if !is_orchestrator_wrapper(outer) {
        return None;
    }
    let without_close = outer.trim().strip_suffix(')')?;
    let (prefix, inner) = without_close.rsplit_once('(')?;
    if !prefix.to_ascii_lowercase().contains("orchestrator") {
        return None;
    }
    let hint = inner.trim();
    (!hint.is_empty()).then_some(hint)
}

fn word_tokens(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|tok| !tok.is_empty())
        .map(str::to_owned)
        .collect()
}

fn format_slug(slug: &str) -> String {
    let mut parts: Vec<&str> = slug.split('-').filter(|p| !p.is_empty()).collect();
    // Drop trailing `-agy` marker on worker slugs (e.g. `gemini-flash-worker-agy`).
    if parts.len() > 2 && parts.last().is_some_and(|p| p.eq_ignore_ascii_case("agy")) {
        parts.pop();
    }
    let effort = parts
        .last()
        .and_then(|last| effort_display(last))
        .inspect(|_| {
            parts.pop();
        });
    let mut words: Vec<String> = Vec::with_capacity(parts.len());
    let mut i = 0;
    while i < parts.len() {
        // Merge adjacent numeric version components (`3-8` → `3.8`, `5-5` → `5.5`).
        if i + 1 < parts.len()
            && is_digits(parts[i])
            && is_digits(parts[i + 1])
            && !words.is_empty()
        {
            words.push(format!("{}.{}", parts[i], parts[i + 1]));
            i += 2;
        } else {
            words.push(format_token(parts[i]));
            i += 1;
        }
    }
    let base = words.join(" ");
    match effort {
        Some(eff) if !base.is_empty() => format!("{base} ({eff})"),
        _ => base,
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn effort_display(token: &str) -> Option<&'static str> {
    match token.to_ascii_lowercase().as_str() {
        "low" => Some("Low"),
        "medium" => Some("Medium"),
        "high" => Some("High"),
        "xhigh" => Some("XHigh"),
        "max" => Some("Max"),
        "minimal" => Some("Minimal"),
        _ => None,
    }
}

fn format_token(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "gpt" => "GPT".into(),
        "oss" => "OSS".into(),
        "chatgpt" => "ChatGPT".into(),
        "openai" => "OpenAI".into(),
        lower => {
            if is_digits(token) {
                return token.to_owned();
            }
            let mut chars = lower.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut out = String::with_capacity(lower.len());
                    for c in first.to_uppercase() {
                        out.push(c);
                    }
                    out.push_str(chars.as_str());
                    out
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_worker_and_placeholder_detection_distinguish_each_kind() {
        assert!(is_orchestrator_wrapper("Orchestrator"));
        assert!(is_orchestrator_wrapper("Orchestrator (Carbon)"));
        assert!(is_orchestrator_wrapper("Orchestrator (Argon)"));
        assert!(is_orchestrator_wrapper("gemini-pro-orchestrator-g1-pro"));
        assert!(!is_orchestrator_wrapper("Gemini 4 Carbon (High)"));
        assert!(!is_orchestrator_wrapper("gemini-flash-worker-agy"));
        assert!(!is_orchestrator_wrapper("Gemini 4 Hydrogen (Orchestrator)"));
        assert!(!is_orchestrator_wrapper(""));

        assert!(is_orchestrator_worker("gemini-flash-worker-agy"));
        assert!(is_orchestrator_worker("Gemini 4 Hydrogen (Orchestrator)"));
        assert!(!is_orchestrator_worker("Orchestrator (Carbon)"));
        assert!(!is_orchestrator_worker("gemini-4-hydrogen-high"));

        assert!(is_placeholder_model(""));
        assert!(is_placeholder_model("auto"));
        assert!(is_placeholder_model("AUTO"));
        assert!(!is_placeholder_model("gemini-4-carbon-high"));

        assert!(is_concrete_step_model("gemini-4-hydrogen-high"));
        assert!(is_concrete_step_model("gemini-flash-worker-agy"));
        assert!(!is_concrete_step_model("auto"));
        assert!(!is_concrete_step_model("Orchestrator (Carbon)"));
        assert!(!is_concrete_step_model("Orchestrator"));
    }

    #[test]
    fn formats_slugs_and_compacts_for_the_activity_cell() {
        assert_eq!(
            format_model_name("gemini-4-carbon-high"),
            "Gemini 4 Carbon (High)"
        );
        assert_eq!(
            format_model_name("gemini-4-hydrogen-high"),
            "Gemini 4 Hydrogen (High)"
        );
        assert_eq!(
            format_model_name("gemini-3-8-flash-low"),
            "Gemini 3.8 Flash (Low)"
        );
        assert_eq!(
            format_model_name("gemini-flash-worker-agy"),
            "Gemini Flash Worker"
        );
        assert_eq!(
            format_model_name("Gemini 4 Hydrogen (Orchestrator)"),
            "Gemini 4 Hydrogen"
        );
        assert_eq!(format_model_name("claude-sonnet-5-5"), "Claude Sonnet 5.5");
        assert_eq!(
            format_model_name("MODEL_PLACEHOLDER_M401"),
            "MODEL_PLACEHOLDER_M401"
        );

        assert_eq!(
            compact_model_name("Gemini 4 Carbon (High)"),
            "Carbon (High)"
        );
        assert_eq!(
            compact_model_name("Gemini 4 Hydrogen (High)"),
            "Hydrogen (High)"
        );
        assert_eq!(
            compact_model_name("Gemini 3.8 Flash (Medium)"),
            "3.8 Flash (Medium)"
        );
        assert_eq!(compact_model_name("Gemini Flash Worker"), "Flash Worker");
        assert_eq!(compact_model_name("Claude Sonnet 5.5"), "Sonnet 5.5");
    }

    #[test]
    fn role_follows_the_outer_planner_and_omits_role_when_bare() {
        // When the outer wrapper names its planner, matching steps are `plan`
        // and any other concrete step — including a Claude executor — is `exec`.
        assert_eq!(
            classify_role("gemini-4-carbon-high", "Orchestrator (Carbon)"),
            Some(Role::Planner)
        );
        assert_eq!(
            classify_role("gemini-4-hydrogen-high", "Orchestrator (Carbon)"),
            Some(Role::Executor)
        );
        assert_eq!(
            classify_role("claude-sonnet-5-5", "Orchestrator (Carbon)"),
            Some(Role::Executor)
        );
        assert_eq!(
            classify_role("gemini-4-argon-high", "Orchestrator (Argon)"),
            Some(Role::Planner)
        );
        assert_eq!(
            classify_role("gemini-4-hydrogen-high", "Orchestrator (Argon)"),
            Some(Role::Executor)
        );

        // Bare `Orchestrator` names no planner: omit the role rather than guess,
        // except for an explicit worker slug.
        assert_eq!(classify_role("gemini-4-carbon-high", "Orchestrator"), None);
        assert_eq!(
            classify_role("gemini-4-hydrogen-high", "Orchestrator"),
            None
        );
        assert_eq!(
            classify_role("gemini-flash-worker-agy", "Orchestrator"),
            Some(Role::Executor)
        );
    }

    #[test]
    fn resolution_gates_on_orchestrator_wrapper_and_concrete_step_model() {
        // Non-orchestrator outer model never resolves an active sub-model.
        assert_eq!(
            resolve_active_model(
                Some("Gemini 4 Carbon (High)"),
                Some("gemini-4-hydrogen-high"),
            ),
            None
        );

        // Pending before any concrete step model arrives.
        assert_eq!(
            resolve_active_model(Some("Orchestrator (Carbon)"), None),
            None
        );
        assert_eq!(
            resolve_active_model(Some("Orchestrator (Carbon)"), Some("Orchestrator (Carbon)")),
            None
        );

        // Concrete executor step under `Orchestrator (Carbon)`.
        let exec = resolve_active_model(
            Some("Orchestrator (Carbon)"),
            Some("gemini-4-hydrogen-high"),
        )
        .expect("resolved executor");
        assert_eq!(exec.display, "Gemini 4 Hydrogen (High)");
        assert_eq!(exec.compact, "Hydrogen (High)");
        assert_eq!(exec.role, Some(Role::Executor));

        // Bare `Orchestrator` resolves the step model with no role.
        let bare = resolve_active_model(Some("Orchestrator"), Some("gemini-4-carbon-high"))
            .expect("resolved bare");
        assert_eq!(bare.compact, "Carbon (High)");
        assert_eq!(bare.role, None);
    }
}
