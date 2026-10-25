//! An agy session's thinking history for `^a t`: each turn's prompt, then the
//! `thinking` of its planner responses, oldest first. Responses, tool calls and
//! their output are left out.
//!
//! `transcript.jsonl` is read forward within a window, keeping a bounded number
//! of steps, so a turn's prompt stays with its thoughts however much tool output
//! lies between them. A field `transcript.jsonl` cut is hydrated from
//! `transcript_full.jsonl` by [`super::load_full_records`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::{
    HydratedRecord, MAX_FULL_SCAN_BYTES, STEP_ORDER_SLACK, field_is_truncated, load_full_records,
    open_regular_file, strip_user_request_tags, window_reader,
};
use crate::ui::theme::Theme;

/// What one read may keep and cost.
#[derive(Clone, Copy)]
struct Limits {
    /// Reasoning steps kept, the newest.
    steps: usize,
    /// Pager lines kept, from the newest step back; a kept step whose turn
    /// began before them keeps that turn's prompt.
    lines: usize,
    /// Bytes of `transcript.jsonl` read, from its end.
    scan_bytes: u64,
}

const LIMITS: Limits = Limits {
    steps: 2048,
    lines: crate::state::MAX_TRANSCRIPT_LINES,
    scan_bytes: 256 * 1024 * 1024,
};

const OMITTED: &str = "[… older thinking history omitted …]";

/// One field of a record, and whether `transcript.jsonl` cut it.
struct Piece {
    step: Option<u64>,
    text: String,
    cut: bool,
}

impl Piece {
    fn of(val: &serde_json::Value, field: &str) -> Self {
        Self {
            step: val["step_index"].as_u64(),
            text: val[field].as_str().unwrap_or_default().to_owned(),
            cut: field_is_truncated(val, field),
        }
    }

    /// The untruncated text, when `transcript_full.jsonl` had it.
    fn hydrate(
        &mut self,
        full: &mut HashMap<u64, HydratedRecord>,
        take: fn(HydratedRecord) -> Option<String>,
    ) {
        if self.cut
            && let Some(text) = self.step.and_then(|s| full.remove(&s)).and_then(take)
        {
            self.text = text;
        }
    }
}

/// A planner response's reasoning, carrying the prompt of its turn when it is
/// the first kept step of that turn.
struct Thought {
    prompt: Option<Piece>,
    thinking: Piece,
}

/// The pager lines of `path`'s thinking history, empty when it has none.
pub fn render_thinking_history(
    path: &Path,
    theme: &Theme,
    width: Option<usize>,
) -> Vec<Line<'static>> {
    render(path, theme, width, LIMITS)
}

fn render(path: &Path, theme: &Theme, width: Option<usize>, limits: Limits) -> Vec<Line<'static>> {
    let Some((mut thoughts, mut omitted)) = collect(path, limits) else {
        return Vec::new();
    };
    let needed: HashSet<u64> = thoughts
        .iter()
        .flat_map(|t| t.prompt.iter().chain([&t.thinking]))
        .filter(|piece| piece.cut)
        .filter_map(|piece| piece.step)
        .collect();
    let mut full = load_full_records(path, &needed, false, MAX_FULL_SCAN_BYTES);
    let mut blocks: VecDeque<(Option<String>, Vec<Line<'static>>)> = VecDeque::new();
    let mut carried: Option<String> = None;
    for thought in &mut thoughts {
        if let Some(prompt) = &mut thought.prompt {
            prompt.hydrate(&mut full, |h| h.content);
            let text = strip_user_request_tags(&prompt.text).trim();
            carried = (!text.is_empty()).then(|| text.to_owned());
        }
        thought.thinking.hydrate(&mut full, |h| h.thinking);
        let body = crate::ui::markdown::render(thought.thinking.text.trim(), theme, width);
        if !body.is_empty() {
            blocks.push_back((carried.take(), body));
        }
    }
    omitted |= keep_newest_lines(&mut blocks, limits.lines);
    format(blocks, omitted, theme)
}

/// The newest `limits.steps` thoughts in the last `limits.scan_bytes` of
/// `path`, and whether older ones were left out. `None` when `path` isn't a
/// readable regular file.
fn collect(path: &Path, limits: Limits) -> Option<(VecDeque<Thought>, bool)> {
    if limits.steps == 0 {
        return None;
    }
    let (mut reader, mut omitted) = window_reader(open_regular_file(path)?, limits.scan_bytes)?;
    let mut buf = Vec::new();
    let mut thoughts: VecDeque<Thought> = VecDeque::new();
    let mut prompt: Option<Piece> = None;
    loop {
        match crate::state::read_line_capped(
            &mut reader,
            &mut buf,
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
        ) {
            Ok(None) | Err(_) => break,
            Ok(Some(false)) => continue,
            Ok(Some(true)) => {}
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            continue;
        };
        if !line.contains("PLANNER_RESPONSE") && !line.contains("USER_INPUT") {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match (val["source"].as_str(), val["type"].as_str()) {
            // Every prompt starts a turn, even one with no text of its own.
            (Some("USER_EXPLICIT"), Some("USER_INPUT")) => {
                prompt = Some(Piece::of(&val, "content"));
            }
            (Some("MODEL"), Some("PLANNER_RESPONSE")) => {
                let thinking = Piece::of(&val, "thinking");
                if !thinking.cut && thinking.text.trim().is_empty() {
                    continue;
                }
                // agy can write a step again, even after a later one: the copy
                // written last wins, in the first copy's place.
                if let Some(step) = thinking.step
                    && let Some(earlier) = thoughts
                        .iter_mut()
                        .rev()
                        .take_while(|t| {
                            t.thinking
                                .step
                                .is_none_or(|s| s.saturating_add(STEP_ORDER_SLACK) >= step)
                        })
                        .find(|t| t.thinking.step == Some(step))
                {
                    earlier.thinking = thinking;
                    continue;
                }
                thoughts.push_back(Thought {
                    prompt: prompt.take(),
                    thinking,
                });
                if thoughts.len() > limits.steps {
                    drop_oldest(&mut thoughts, |t| &mut t.prompt);
                    omitted = true;
                }
            }
            _ => {}
        }
    }
    Some((thoughts, omitted))
}

/// Drops the oldest entry, handing its prompt to the next when that one has
/// none, so a turn cut in two keeps its heading.
fn drop_oldest<T, P>(entries: &mut VecDeque<T>, prompt: fn(&mut T) -> &mut Option<P>) {
    if let Some(mut oldest) = entries.pop_front()
        && let Some(taken) = prompt(&mut oldest).take()
        && let Some(next) = entries.front_mut()
    {
        prompt(next).get_or_insert(taken);
    }
}

/// Drops the oldest blocks until the rest fit in `max_lines`, always keeping
/// the newest. Whether any were dropped.
fn keep_newest_lines(
    blocks: &mut VecDeque<(Option<String>, Vec<Line<'static>>)>,
    max_lines: usize,
) -> bool {
    // A block is a blank, its prompt's lines and a blank, then its body.
    let height = |block: &(Option<String>, Vec<Line<'static>>)| {
        block.0.as_deref().map_or(0, |p| p.lines().count() + 1) + block.1.len() + 1
    };
    let mut total: usize = blocks.iter().map(height).sum();
    let mut dropped = false;
    while blocks.len() > 1 && total > max_lines {
        // The next block may inherit the dropped one's prompt.
        let before: usize = blocks.iter().take(2).map(height).sum();
        drop_oldest(blocks, |b| &mut b.0);
        total = total - before + blocks.front().map_or(0, height);
        dropped = true;
    }
    dropped
}

fn format(
    blocks: VecDeque<(Option<String>, Vec<Line<'static>>)>,
    omitted: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = if omitted && !blocks.is_empty() {
        let notice = Style::default()
            .fg(theme.status_suffix)
            .add_modifier(Modifier::ITALIC);
        out.push(Line::from(Span::styled(OMITTED, notice)));
        false
    } else {
        true
    };
    for (prompt, body) in blocks {
        if let Some(prompt) = prompt {
            crate::state::push_transcript_prompt(
                &mut out,
                &mut last_was_blank,
                &prompt,
                user_style,
            );
        }
        crate::state::push_transcript_blank(&mut out, &mut last_was_blank);
        out.extend(body);
        last_was_blank = out
            .last()
            .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn flatten(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `rows` as `dir/transcript.jsonl`, and `full` as its
    /// `transcript_full.jsonl` when there are any.
    fn transcript(dir: &Path, rows: &[&str], full: &[&str]) -> std::path::PathBuf {
        let compact = dir.join("transcript.jsonl");
        std::fs::write(&compact, rows.join("\n") + "\n").unwrap();
        if !full.is_empty() {
            std::fs::write(dir.join("transcript_full.jsonl"), full.join("\n") + "\n").unwrap();
        }
        compact
    }

    fn shown(path: &Path, limits: Limits) -> String {
        flatten(&render(path, &Theme::default(), None, limits))
    }

    #[test]
    fn a_turns_prompt_heads_its_thoughts_and_a_cut_thought_is_hydrated() {
        let dir = tempfile::tempdir().unwrap();
        let compact = transcript(
            dir.path(),
            &[
                r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>orphan prompt without thinking</USER_REQUEST>"}"#,
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","content":"direct reply"}"#,
                r#"{"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>inspect reasoning</USER_REQUEST>"}"#,
                r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"First thought step.","content":"interlude"}"#,
                r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"trunc\n<truncated 41 bytes>\n","truncated_fields":["thinking"]}"#,
            ],
            &[
                r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Hydrated second thought step from full transcript."}"#,
            ],
        );
        let rendered = shown(&compact, LIMITS);
        assert!(!rendered.contains("orphan prompt"), "{rendered}");
        assert!(
            !rendered.contains("direct reply") && !rendered.contains("interlude"),
            "{rendered}"
        );
        let prompt = rendered.find("❯ inspect reasoning").expect(&rendered);
        let first = rendered.find("First thought step.").expect(&rendered);
        let second = rendered
            .find("Hydrated second thought step from full transcript.")
            .expect(&rendered);
        assert!(prompt < first && first < second, "{rendered}");
        assert!(!rendered.contains("<truncated"), "{rendered}");
        assert!(!rendered.contains("omitted"), "{rendered}");
    }

    #[test]
    fn a_cut_prompt_is_hydrated() {
        let dir = tempfile::tempdir().unwrap();
        let compact = transcript(
            dir.path(),
            &[
                r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>the head of a long","truncated_fields":["content"]}"#,
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Reading it."}"#,
            ],
            &[
                r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>the head of a long prompt, whole</USER_REQUEST>"}"#,
            ],
        );
        let rendered = shown(&compact, LIMITS);
        assert!(
            rendered.contains("❯ the head of a long prompt, whole"),
            "{rendered}"
        );
    }

    #[test]
    fn a_prompt_with_no_text_still_starts_a_turn() {
        let dir = tempfile::tempdir().unwrap();
        let compact = transcript(
            dir.path(),
            &[
                r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>a turn with no thinking</USER_REQUEST>"}"#,
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","content":"done"}"#,
                r#"{"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"","media":[{"mime_type":"image/png","uri":"/tmp/shot.png"}]}"#,
                r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Looking at the image."}"#,
            ],
            &[],
        );
        let rendered = shown(&compact, LIMITS);
        assert!(rendered.contains("Looking at the image."), "{rendered}");
        assert!(!rendered.contains("a turn with no thinking"), "{rendered}");
    }

    #[test]
    fn the_read_reaches_past_v_s_tail_and_a_step_cap_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let mut f = std::fs::File::create(&compact).unwrap();
        for row in [
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>early turn prompt</USER_REQUEST>"}"#,
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Early turn thought."}"#,
            r#"{"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>second turn prompt</USER_REQUEST>"}"#,
            r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Second turn first thought."}"#,
        ] {
            writeln!(f, "{row}").unwrap();
        }
        // More tool output than `^a v` reads, between a turn's prompt and its
        // last thought.
        let blob = "x".repeat(512 * 1024);
        for step in 5..15 {
            writeln!(
                f,
                r#"{{"step_index":{step},"source":"SYSTEM","type":"GENERIC","status":"DONE","content":"{blob}"}}"#
            )
            .unwrap();
        }
        writeln!(
            f,
            r#"{{"step_index":15,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Second turn final thought."}}"#
        )
        .unwrap();
        f.flush().unwrap();
        assert!(
            std::fs::metadata(&compact).unwrap().len() > crate::state::MAX_TRANSCRIPT_TAIL_BYTES
        );

        let whole = shown(&compact, LIMITS);
        assert!(
            whole.contains("❯ early turn prompt") && whole.contains("Early turn thought."),
            "{whole}"
        );
        assert!(whole.contains("Second turn final thought."), "{whole}");
        assert!(!whole.contains(OMITTED), "{whole}");

        // One step kept: the newest, still under its turn's prompt.
        let capped = shown(&compact, Limits { steps: 1, ..LIMITS });
        assert!(capped.starts_with(OMITTED), "{capped}");
        assert!(!capped.contains("Early turn thought"), "{capped}");
        assert!(!capped.contains("Second turn first thought"), "{capped}");
        let prompt = capped.find("❯ second turn prompt").expect(&capped);
        let last = capped.find("Second turn final thought.").expect(&capped);
        assert!(prompt < last, "{capped}");
    }

    #[test]
    fn a_window_or_line_cap_leaves_the_oldest_out_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let rows = [
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>first turn</USER_REQUEST>"}"#,
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Oldest thought."}"#,
            r#"{"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>second turn</USER_REQUEST>"}"#,
            r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Middle thought."}"#,
            r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Newest thought."}"#,
        ];
        let compact = transcript(dir.path(), &rows, &[]);
        let tail: u64 = rows[2..].iter().map(|r| r.len() as u64 + 1).sum();

        let windowed = shown(
            &compact,
            Limits {
                scan_bytes: tail,
                ..LIMITS
            },
        );
        assert!(windowed.starts_with(OMITTED), "{windowed}");
        assert!(!windowed.contains("Oldest thought."), "{windowed}");
        assert!(
            windowed.contains("❯ second turn") && windowed.contains("Newest thought."),
            "{windowed}"
        );

        // Room for one block: the newest, under the prompt of the turn it
        // shares with the block dropped before it.
        let lined = shown(&compact, Limits { lines: 4, ..LIMITS });
        assert!(lined.starts_with(OMITTED), "{lined}");
        assert!(!lined.contains("Middle thought."), "{lined}");
        let prompt = lined.find("❯ second turn").expect(&lined);
        let newest = lined.find("Newest thought.").expect(&lined);
        assert!(prompt < newest, "{lined}");
    }

    #[test]
    fn a_step_written_again_shows_once_as_written_last() {
        let dir = tempfile::tempdir().unwrap();
        // agy can write a step again after a later one; both copies were cut.
        let compact = transcript(
            dir.path(),
            &[
                r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>go</USER_REQUEST>"}"#,
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Draft\n<truncated 9 bytes>\n","truncated_fields":["thinking"]}"#,
                r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"The next step."}"#,
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Final\n<truncated 9 bytes>\n","truncated_fields":["thinking"]}"#,
            ],
            &[
                r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Final form of the step."}"#,
            ],
        );
        let rendered = shown(&compact, LIMITS);
        assert!(!rendered.contains("Draft"), "{rendered}");
        assert!(!rendered.contains("<truncated"), "{rendered}");
        assert_eq!(
            rendered.matches("Final form of the step.").count(),
            1,
            "{rendered}"
        );
        let prompt = rendered.find("❯ go").expect(&rendered);
        let step2 = rendered.find("Final form of the step.").expect(&rendered);
        let step3 = rendered.find("The next step.").expect(&rendered);
        assert!(prompt < step2 && step2 < step3, "{rendered}");
    }

    #[test]
    fn a_path_that_is_no_regular_file_has_no_history() {
        let dir = tempfile::tempdir().unwrap();
        assert!(render_thinking_history(dir.path(), &Theme::default(), None).is_empty());
        let missing = dir.path().join("transcript.jsonl");
        assert!(render_thinking_history(&missing, &Theme::default(), None).is_empty());
    }
}
