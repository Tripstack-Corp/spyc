//! Read an Antigravity (`agy`) conversation transcript from its on-disk JSONL
//! into styled pager lines (`^a v`) or gallery image metadata (`^a g`).

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

/// Resolve the conversation JSONL for the Agy session running in the pane.
///
/// Prefers the id pinned to THIS pane (agy's `conversationId`, reported by its
/// status hook), falling back to the session whose start time is closest to the
/// pane's spawn time. Proximity is a guess that collides whenever two panes start
/// within the same second — which is every `-r` restore.
///
/// A pinned id with no transcript on disk yet returns `None` rather than
/// proximity-matching: `None` makes `^a v` flash "no transcript found" and close,
/// which is the right answer for a conversation we've positively identified but
/// have nothing on disk for. Showing a *different* conversation's history — what
/// proximity-matching would do here — is worse than showing none.
/// The `command` field of the query is unused.
pub fn resolve_active_jsonl(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    resolve_under_home(Path::new(&std::env::var_os("HOME")?), q)
}

/// [`resolve_active_jsonl`] with `home` passed in, so the resolution order is
/// testable without mutating the process environment.
fn resolve_under_home(home: &Path, q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    use crate::state::sessions::agy_transcript_path;

    if let Some(id) = q.session_id {
        let path = agy_transcript_path(home, id);
        return path.exists().then_some(path);
    }

    let best = crate::state::sessions::find_agy_sessions(q.cwd)
        .into_iter()
        .min_by_key(|s| s.started_at_secs.abs_diff(q.spawn_epoch_secs))?;
    Some(agy_transcript_path(home, &best.session_id))
}

/// Parse an Agy conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Model prose is
/// rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain.
///
/// When a rendered record in `transcript.jsonl` marks `content` (or
/// `tool_calls` while `show_tool_calls` is on) in `truncated_fields`,
/// its untruncated counterpart is resolved by `step_index` from the
/// sibling `transcript_full.jsonl` when present.
pub fn render_transcript(
    path: &Path,
    theme: &Theme,
    width: Option<usize>,
    show_tool_calls: bool,
) -> Vec<Line<'static>> {
    let Some(file) = open_regular_file(path) else {
        return Vec::new();
    };
    let Ok(text) =
        crate::state::read_file_tail_lossy(file, crate::state::MAX_TRANSCRIPT_TAIL_BYTES)
    else {
        return Vec::new();
    };
    let (records, needed_steps) = parse_rendered_records(&text, show_tool_calls);
    let full_records = load_full_records(path, &needed_steps, show_tool_calls, MAX_FULL_SCAN_BYTES);

    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true;
    let mut last_was_prose = false;

    for record in &records {
        let hydrated = record["step_index"]
            .as_u64()
            .and_then(|step| full_records.get(&step));
        render_one_record(
            record,
            hydrated,
            theme,
            width,
            show_tool_calls,
            user_style,
            tool_style,
            &mut out,
            &mut last_was_blank,
            &mut last_was_prose,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_one_record(
    record: &serde_json::Value,
    hydrated: Option<&HydratedRecord>,
    theme: &Theme,
    width: Option<usize>,
    show_tool_calls: bool,
    user_style: Style,
    tool_style: Style,
    out: &mut Vec<Line<'static>>,
    last_was_blank: &mut bool,
    last_was_prose: &mut bool,
) {
    let source = record["source"].as_str().unwrap_or("");
    let msg_type = record["type"].as_str().unwrap_or("");
    let content_truncated = field_is_truncated(record, "content");
    let raw_content = hydrated
        .filter(|_| content_truncated)
        .and_then(|h| h.content.as_deref())
        .or_else(|| record["content"].as_str())
        .unwrap_or("");
    let unhydrated_content =
        content_truncated && hydrated.and_then(|h| h.content.as_ref()).is_none();

    if source == "USER_EXPLICIT" && msg_type == "USER_INPUT" {
        if raw_content.is_empty() && !unhydrated_content {
            return;
        }
        let stripped = strip_user_request_tags(raw_content).trim();
        if !stripped.is_empty() {
            crate::state::push_transcript_prompt(out, last_was_blank, stripped, user_style);
        }
        if unhydrated_content {
            crate::state::push_transcript_blank(out, last_was_blank);
            out.push(Line::from("[truncated]"));
            *last_was_blank = false;
        }
        *last_was_prose = false;
    } else if source == "MODEL" && msg_type == "PLANNER_RESPONSE" {
        if !raw_content.trim().is_empty() {
            crate::state::push_agent_markdown(out, last_was_blank, raw_content, theme, width);
            *last_was_prose = true;
        }
        if unhydrated_content {
            crate::state::push_transcript_blank(out, last_was_blank);
            out.push(Line::from("[truncated]"));
            *last_was_blank = false;
            *last_was_prose = true;
        }

        if show_tool_calls {
            let tools_truncated = field_is_truncated(record, "tool_calls");
            let labels: Vec<String> = if tools_truncated
                && let Some(hydrated_labels) = hydrated.and_then(|h| h.tool_labels.as_ref())
            {
                hydrated_labels.clone()
            } else if let Some(tool_calls) = record["tool_calls"].as_array() {
                let mut v: Vec<String> = tool_calls.iter().map(tool_call_label).collect();
                if tools_truncated {
                    v.push("\u{2699} [truncated]".to_owned());
                }
                v
            } else if tools_truncated {
                vec!["\u{2699} [truncated]".to_owned()]
            } else {
                Vec::new()
            };
            if !labels.is_empty() {
                if *last_was_prose {
                    crate::state::push_transcript_blank(out, last_was_blank);
                    *last_was_prose = false;
                }
                for label in labels {
                    out.push(Line::from(Span::styled(label, tool_style)));
                    *last_was_blank = false;
                }
            }
        }
    }
}

fn is_rendered_record(val: &serde_json::Value) -> bool {
    let source = val["source"].as_str().unwrap_or("");
    let msg_type = val["type"].as_str().unwrap_or("");
    (source == "USER_EXPLICIT" && msg_type == "USER_INPUT")
        || (source == "MODEL" && msg_type == "PLANNER_RESPONSE")
}

/// Maximum tail bytes scanned in `transcript.jsonl` when indexing gallery images.
const MAX_IMAGE_SCAN_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum number of images returned for the `^a g` gallery.
const MAX_GALLERY_IMAGES: usize = 4096;
/// Maximum total (and per-record) bytes hydrated from `transcript_full.jsonl`.
const MAX_FULL_HYDRATION_BYTES: usize = crate::state::MAX_TRANSCRIPT_TAIL_BYTES as usize;
/// Maximum trailing bytes of `transcript_full.jsonl` scanned backward when hydrating truncated steps.
const MAX_FULL_SCAN_BYTES: u64 = 64 * 1024 * 1024;
/// Chunk size for the reverse `transcript_full.jsonl` scanner.
const BACKWARD_CHUNK_BYTES: usize = 64 * 1024;

/// Open `path` as a regular file without blocking if `path` is a named pipe.
fn open_regular_file(path: &Path) -> Option<std::fs::File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// Scan up to the trailing `MAX_IMAGE_SCAN_BYTES` of an Agy `transcript.jsonl`
/// and index up to the newest `MAX_GALLERY_IMAGES` user-attached images in
/// chronological order for the `^a g` gallery.
///
/// Agy stores attached media on `USER_INPUT` records as a `"media"` array of
/// `{ "mime_type": "image/...", "uri": "/abs/path" }` references rather than
/// inline base64. If a record's `"content"` or `"media"` was truncated in
/// `transcript.jsonl`, its untruncated step is resolved from sibling
/// `transcript_full.jsonl` when present.
pub fn index_images(path: &Path) -> Vec<super::transcript_images::TranscriptImage> {
    index_images_with_limits(path, MAX_IMAGE_SCAN_BYTES, MAX_GALLERY_IMAGES)
}

fn index_images_with_limits(
    path: &Path,
    max_scan_bytes: u64,
    max_images: usize,
) -> Vec<super::transcript_images::TranscriptImage> {
    let Some(file) = open_regular_file(path) else {
        return Vec::new();
    };
    let (candidates, needed_steps, oversized) = scan_image_candidates(file, max_scan_bytes);
    if oversized > 0 {
        crate::debug_log::log(&format!(
            "agy_transcript: skipped {oversized} record(s) over {} bytes in {}",
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
            path.display()
        ));
    }
    let full_records = load_full_records(path, &needed_steps, false, MAX_FULL_SCAN_BYTES);
    let mut batches = Vec::new();
    let mut total_collected = 0usize;
    for (line_no, record) in candidates.iter().rev() {
        if total_collected >= max_images {
            break;
        }
        let hydrated = record["step_index"]
            .as_u64()
            .and_then(|step| full_records.get(&step));
        let remaining = max_images - total_collected;
        let mut batch = Vec::new();
        index_image_record(path, record, hydrated, *line_no, remaining, &mut batch);
        if !batch.is_empty() {
            total_collected += batch.len();
            batches.push(batch);
        }
    }
    batches.reverse();
    let mut out = Vec::with_capacity(total_collected);
    for batch in batches {
        for mut img in batch {
            img.seq = out.len() + 1;
            out.push(img);
        }
    }
    if out.len() == max_images {
        crate::debug_log::log("agy_transcript: gallery scan reached its byte or image limit");
    }
    out
}

fn scan_image_candidates(
    mut file: std::fs::File,
    max_scan_bytes: u64,
) -> (Vec<(usize, serde_json::Value)>, HashSet<u64>, usize) {
    let file_len = file.metadata().map_or(0, |m| m.len());
    let hit_scan_cap = file_len > max_scan_bytes;
    if hit_scan_cap {
        crate::debug_log::log("agy_transcript: gallery scan reached its byte or image limit");
        if file
            .seek(SeekFrom::Start(file_len - max_scan_bytes - 1))
            .is_err()
        {
            return (Vec::new(), HashSet::new(), 0);
        }
    }
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    if hit_scan_cap {
        let _ = crate::state::read_line_capped(
            &mut reader,
            &mut buf,
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
        );
    }
    let mut oversized = 0usize;
    let mut seen_steps = HashSet::new();
    let mut candidates = Vec::new();
    let mut needed = HashSet::new();
    for line_no in 0.. {
        match crate::state::read_line_capped(
            &mut reader,
            &mut buf,
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
        ) {
            Ok(None) | Err(_) => break,
            Ok(Some(false)) => {
                oversized += 1;
                continue;
            }
            Ok(Some(true)) => {}
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            continue;
        };
        if !line.contains("\"media\"") {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["source"].as_str() != Some("USER_EXPLICIT")
            || val["type"].as_str() != Some("USER_INPUT")
        {
            continue;
        }
        if let Some(step) = val["step_index"].as_u64() {
            if !seen_steps.insert(step) {
                continue;
            }
            if needs_full_image_record(&val) {
                needed.insert(step);
            }
        }
        candidates.push((line_no, val));
    }
    (candidates, needed, oversized)
}

fn field_is_truncated(val: &serde_json::Value, name: &str) -> bool {
    val["truncated_fields"]
        .as_array()
        .is_some_and(|fields| fields.iter().any(|f| f.as_str() == Some(name)))
}

fn needs_full_image_record(val: &serde_json::Value) -> bool {
    field_is_truncated(val, "content") || field_is_truncated(val, "media")
}

/// Resolve a media `uri` from an Agy transcript into an on-disk path.
///
/// Handles `file://` and `file://localhost/` URLs with percent-decoding (preserving
/// raw non-UTF-8 bytes on Unix), and anchors relative paths to the transcript
/// directory (or the enclosing session directory when the transcript lives under
/// `.system_generated/logs`) rather than spyc's mutable process working directory.
fn resolve_image_uri(transcript_path: &Path, uri: &str) -> Option<PathBuf> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = if let Some(rest) = trimmed.strip_prefix("file://") {
        let raw_path = if rest.starts_with('/') {
            rest
        } else if let Some(after_host) = rest.strip_prefix("localhost")
            && after_host.starts_with('/')
        {
            after_host
        } else {
            return None;
        };
        let decoded: Vec<u8> = percent_encoding::percent_decode_str(raw_path).collect();
        if decoded.is_empty() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(decoded))
        }
        #[cfg(not(unix))]
        {
            PathBuf::from(String::from_utf8(decoded).ok()?)
        }
    } else {
        PathBuf::from(trimmed)
    };
    if path.is_absolute() {
        return Some(path);
    }
    let transcript_dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let direct = transcript_dir.join(&path);
    if direct.exists() {
        return Some(direct);
    }
    for ancestor in transcript_dir.ancestors().skip(1).take(2) {
        let candidate = ancestor.join(&path);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    Some(direct)
}

fn index_image_record(
    transcript_path: &Path,
    record: &serde_json::Value,
    hydrated: Option<&HydratedRecord>,
    line_no: usize,
    max_images: usize,
    out: &mut Vec<super::transcript_images::TranscriptImage>,
) {
    let media = hydrated
        .filter(|_| field_is_truncated(record, "media"))
        .and_then(|h| h.media.as_ref())
        .or_else(|| record["media"].as_array());
    let Some(media) = media else {
        return;
    };
    let raw_content = hydrated
        .filter(|_| field_is_truncated(record, "content"))
        .and_then(|h| h.content.as_deref())
        .or_else(|| record["content"].as_str())
        .unwrap_or("");
    let text = strip_user_request_tags(raw_content);
    let labels = super::transcript_images::image_labels(text);
    let excerpt = super::transcript_images::excerpt_of(text);
    let timestamp = record["created_at"]
        .as_str()
        .or_else(|| record["timestamp"].as_str())
        .map(str::to_owned)
        .or_else(|| hydrated.and_then(|h| h.timestamp.clone()));
    let mut nth = 0usize;
    for item in media {
        if out.len() >= max_images {
            break;
        }
        let media_type = item["mime_type"].as_str().unwrap_or("image");
        if media_type != "image" && !media_type.starts_with("image/") {
            continue;
        }
        let Some(uri) = item["uri"].as_str() else {
            continue;
        };
        let Some(img_path) = resolve_image_uri(transcript_path, uri) else {
            continue;
        };
        let bytes_len = std::fs::metadata(&img_path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map_or(0, |m| {
                m.len().min(super::transcript_images::MAX_IMAGE_BYTES) as usize
            });
        out.push(super::transcript_images::TranscriptImage {
            seq: out.len() + 1,
            agent_label: labels.get(nth).cloned(),
            media_type: media_type.to_owned(),
            bytes_len,
            timestamp: timestamp.clone(),
            prompt_excerpt: excerpt.clone(),
            line: line_no,
            block: nth,
            source: super::transcript_images::ImageSource::Agy { path: img_path },
        });
        nth += 1;
    }
}

fn needs_full_record(val: &serde_json::Value, show_tool_calls: bool) -> bool {
    field_is_truncated(val, "content") || (show_tool_calls && field_is_truncated(val, "tool_calls"))
}

fn parse_rendered_records(
    text: &str,
    show_tool_calls: bool,
) -> (Vec<serde_json::Value>, HashSet<u64>) {
    let mut records = Vec::new();
    let mut needed = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if !is_rendered_record(&val) {
            continue;
        }
        if needs_full_record(&val, show_tool_calls)
            && let Some(step) = val["step_index"].as_u64()
        {
            needed.insert(step);
        }
        records.push(val);
    }
    (records, needed)
}

/// Lightweight projection of one hydrated record from `transcript_full.jsonl` so
/// multi-megabyte tool-call arguments and thinking traces are dropped as soon
/// as the step is parsed.
struct HydratedRecord {
    content: Option<String>,
    tool_labels: Option<Vec<String>>,
    media: Option<Vec<serde_json::Value>>,
    timestamp: Option<String>,
}

impl HydratedRecord {
    fn from_value(val: &serde_json::Value, show_tool_calls: bool) -> Self {
        Self {
            content: val["content"].as_str().map(str::to_owned),
            tool_labels: show_tool_calls
                .then(|| {
                    val["tool_calls"]
                        .as_array()
                        .map(|calls| calls.iter().map(tool_call_label).collect())
                })
                .flatten(),
            media: val["media"].as_array().cloned(),
            timestamp: val["created_at"]
                .as_str()
                .or_else(|| val["timestamp"].as_str())
                .map(str::to_owned),
        }
    }
}

#[cfg(test)]
std::thread_local! {
    static FULL_LOAD_COUNTS: std::cell::Cell<(usize, usize, usize)> = const { std::cell::Cell::new((0, 0, 0)) };
}

/// Extract the leading `"step_index": N` field from an Agy JSONL record without
/// parsing the full JSON object. Agy records emit `"step_index"` as the first
/// field of each line; we check only the opening key so multi-megabyte payloads
/// are never parsed unless their step is in `remaining`.
fn extract_leading_step(line: &str) -> Option<u64> {
    let after_brace = line.trim_start().strip_prefix('{')?.trim_start();
    let after_key = after_brace.strip_prefix("\"step_index\"")?.trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();
    let digits_len = after_colon.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return None;
    }
    after_colon[..digits_len].parse::<u64>().ok()
}

fn load_full_records(
    compact_path: &Path,
    needed: &HashSet<u64>,
    show_tool_calls: bool,
    _scan_window: u64,
) -> HashMap<u64, HydratedRecord> {
    #[cfg(test)]
    FULL_LOAD_COUNTS.with(|c| c.set((0, 0, 0)));
    let mut found = HashMap::new();
    if needed.is_empty() {
        return found;
    }
    let full_path = compact_path.with_file_name("transcript_full.jsonl");
    let Some(file) = open_regular_file(&full_path) else {
        return found;
    };
    let mut remaining = needed.clone();
    scan_full_records_backward(file, &mut remaining, show_tool_calls, &mut found);
    found
}

/// Accumulates trailing chunks of a JSONL line while scanning backward from the
/// end of `transcript_full.jsonl`. Once the accumulated bytes for a single line
/// exceed `MAX_FULL_HYDRATION_BYTES`, the line cannot be hydrated anyway: we
/// mark it `oversize` and keep only the newest prefix chunk so
/// `extract_leading_step` can still inspect the leading `"step_index"` field
/// without shifting a multi-megabyte buffer on every 64 KiB chunk.
struct BackwardLineBuffer {
    parts: Vec<Vec<u8>>,
    total_len: usize,
    oversize: bool,
}

impl BackwardLineBuffer {
    const fn new() -> Self {
        Self {
            parts: Vec::new(),
            total_len: 0,
            oversize: false,
        }
    }

    const fn is_empty(&self) -> bool {
        self.total_len == 0 && !self.oversize
    }

    fn prepend(&mut self, slice: &[u8]) {
        if slice.is_empty() {
            return;
        }
        if self.oversize || self.total_len + slice.len() > MAX_FULL_HYDRATION_BYTES {
            self.oversize = true;
            self.parts.clear();
            let head_len = slice.len().min(128);
            self.parts.push(slice[..head_len].to_vec());
            self.total_len = head_len;
        } else {
            self.parts.push(slice.to_vec());
            self.total_len += slice.len();
        }
    }

    fn take(&mut self) -> (Vec<u8>, bool) {
        let oversize = self.oversize;
        self.oversize = false;
        let total = std::mem::take(&mut self.total_len);
        if self.parts.len() <= 1 {
            return (self.parts.pop().unwrap_or_default(), oversize);
        }
        let mut out = Vec::with_capacity(total);
        for part in self.parts.drain(..).rev() {
            out.extend_from_slice(&part);
        }
        (out, oversize)
    }
}

/// Scan `transcript_full.jsonl` backward from EOF in 64 KiB chunks so the
/// newest truncated steps (the ones visible at the bottom of `^a v`) are found
/// and hydrated first without streaming the whole file from step 1.
fn scan_full_records_backward(
    mut file: std::fs::File,
    remaining: &mut HashSet<u64>,
    show_tool_calls: bool,
    found: &mut HashMap<u64, HydratedRecord>,
) {
    let Ok(meta) = file.metadata() else {
        return;
    };
    let file_len = meta.len();
    let scan_floor = file_len.saturating_sub(MAX_FULL_SCAN_BYTES);
    let mut pos = file_len;
    let mut chunk_buf = vec![0u8; BACKWARD_CHUNK_BYTES];
    let mut line_buf = BackwardLineBuffer::new();
    let mut hydrated_bytes = 0usize;

    while pos > scan_floor && !remaining.is_empty() {
        let read_len = ((pos - scan_floor) as usize).min(BACKWARD_CHUNK_BYTES);
        pos -= read_len as u64;
        if file.seek(SeekFrom::Start(pos)).is_err()
            || file.read_exact(&mut chunk_buf[..read_len]).is_err()
        {
            return;
        }
        #[cfg(test)]
        FULL_LOAD_COUNTS.with(|c| {
            let (lines, parsed, bytes) = c.get();
            c.set((lines, parsed, bytes + read_len));
        });
        let chunk = &chunk_buf[..read_len];
        let mut end = chunk.len();
        for i in (0..chunk.len()).rev() {
            if chunk[i] == b'\n' {
                line_buf.prepend(&chunk[i + 1..end]);
                end = i;
                let (raw_line, oversize) = line_buf.take();
                if inspect_full_line(
                    &raw_line,
                    oversize,
                    remaining,
                    show_tool_calls,
                    &mut hydrated_bytes,
                    found,
                )
                .is_break()
                {
                    return;
                }
            }
        }
        line_buf.prepend(&chunk[..end]);
    }

    if !line_buf.is_empty() && !remaining.is_empty() {
        let (raw_line, oversize) = line_buf.take();
        let _ = inspect_full_line(
            &raw_line,
            oversize,
            remaining,
            show_tool_calls,
            &mut hydrated_bytes,
            found,
        );
    }
}

fn inspect_full_line(
    raw: &[u8],
    oversize: bool,
    remaining: &mut HashSet<u64>,
    show_tool_calls: bool,
    hydrated_bytes: &mut usize,
    found: &mut HashMap<u64, HydratedRecord>,
) -> ControlFlow<()> {
    let trimmed_bytes = raw.trim_ascii();
    if trimmed_bytes.is_empty() {
        return ControlFlow::Continue(());
    }
    #[cfg(test)]
    FULL_LOAD_COUNTS.with(|c| {
        let (lines, parsed, bytes) = c.get();
        c.set((lines + 1, parsed, bytes));
    });
    let Some(min_needed) = remaining.iter().copied().min() else {
        return ControlFlow::Break(());
    };
    let text = String::from_utf8_lossy(trimmed_bytes);
    let Some(candidate_step) = extract_leading_step(&text) else {
        return ControlFlow::Continue(());
    };
    if candidate_step < min_needed {
        return ControlFlow::Break(());
    }
    if !remaining.contains(&candidate_step) {
        return ControlFlow::Continue(());
    }
    if oversize
        || (*hydrated_bytes > 0
            && hydrated_bytes.saturating_add(text.len()) > MAX_FULL_HYDRATION_BYTES)
    {
        remaining.remove(&candidate_step);
        return if remaining.is_empty() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        };
    }
    #[cfg(test)]
    FULL_LOAD_COUNTS.with(|c| {
        let (lines, parsed, bytes) = c.get();
        c.set((lines, parsed + 1, bytes));
    });
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
        return ControlFlow::Continue(());
    };
    if is_rendered_record(&val)
        && val["step_index"].as_u64() == Some(candidate_step)
        && remaining.remove(&candidate_step)
    {
        *hydrated_bytes = hydrated_bytes.saturating_add(text.len());
        found.insert(
            candidate_step,
            HydratedRecord::from_value(&val, show_tool_calls),
        );
        if remaining.is_empty() || *hydrated_bytes >= MAX_FULL_HYDRATION_BYTES {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

/// Strip `<USER_REQUEST>` wrapper tags when well-formed. The closing tag must
/// follow the opening tag or the slice would panic on untrusted input containing
/// a stray `</USER_REQUEST>` before `<USER_REQUEST>`.
fn strip_user_request_tags(content: &str) -> &str {
    let open = "<USER_REQUEST>";
    if let Some(start) = content.find(open)
        && let Some(end) = content.find("</USER_REQUEST>")
        && start + open.len() <= end
    {
        &content[start + open.len()..end]
    } else {
        content
    }
}

/// Format a tool call line as `⚙ name(detail)` or `⚙ name`, unwrapping lazy
/// `call_mcp_tool` invocations to `mcp_<server>_<tool>` and extracting a
/// one-line summary of the most salient argument.
fn tool_call_label(tool: &serde_json::Value) -> String {
    let raw = tool["name"].as_str().unwrap_or("?");
    let args = if tool["args"].is_object() {
        &tool["args"]
    } else {
        &tool["parameters"]
    };
    let (name, detail_input);
    let parsed_mcp_args;
    if raw == "call_mcp_tool"
        && let Some(server) = decode_arg_scalar(&args["ServerName"])
        && let Some(inner) = decode_arg_scalar(&args["ToolName"])
        && !server.is_empty()
        && !inner.is_empty()
    {
        name = format!("mcp_{server}_{inner}");
        parsed_mcp_args = match &args["Arguments"] {
            serde_json::Value::String(s) => {
                serde_json::from_str::<serde_json::Value>(s).unwrap_or(serde_json::Value::Null)
            }
            other => other.clone(),
        };
        detail_input = &parsed_mcp_args;
    } else {
        name = raw.to_owned();
        detail_input = args;
    }

    match tool_use_detail(detail_input) {
        Some(detail) => format!("\u{2699} {name}({detail})"),
        None => format!("\u{2699} {name}"),
    }
}

/// Extract a short one-line summary from a tool call's arguments object.
fn tool_use_detail(input: &serde_json::Value) -> Option<String> {
    const KEYS: &[&str] = &[
        "toolSummary",
        "description",
        "Path",
        "TargetFile",
        "file_path",
        "path",
        "SearchPath",
        "DirectoryPath",
        "pattern",
        "Query",
        "query",
        "CommandLine",
        "command",
        "branch",
        "status",
        "url",
        "Url",
        "Uri",
        "Role",
        "Prompt",
        "prompt",
        "Action",
        "TaskId",
        "root",
        "id",
    ];
    for key in KEYS {
        let Some(raw) = decode_arg_scalar(&input[*key]) else {
            continue;
        };
        let first = raw.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(crate::state::truncate_chars(first, 80));
        }
    }
    None
}

/// Decode a scalar argument value from an Agy tool call. `transcript.jsonl`
/// encodes string values inside `args` as JSON string literals (e.g.
/// `"\"src/foo.rs\""`), whereas `parameters` stores plain JSON scalars.
fn decode_arg_scalar(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => {
            if let Ok(unquoted) = serde_json::from_str::<String>(s) {
                Some(unquoted)
            } else if let Ok(num) = serde_json::from_str::<i64>(s) {
                Some(num.to_string())
            } else {
                Some(s.trim_matches(|c| c == '"' || c == '\'').to_owned())
            }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let lines = render_transcript(
            Path::new("/nonexistent/x.jsonl"),
            &Theme::default(),
            None,
            true,
        );
        assert!(lines.is_empty());
    }

    /// A pinned `conversationId` addresses the transcript directly. The
    /// proximity fallback picks by spawn time, which collides across panes
    /// restored together — so a pane that KNOWS its conversation must not use it.
    #[test]
    fn pinned_conversation_id_resolves_its_own_transcript() {
        let home = tempfile::tempdir().unwrap();
        let id = "ec33ebf9-0cba-4100-8142-c61503f6c587";
        let expected = home
            .path()
            .join(".gemini/antigravity-cli/brain")
            .join(id)
            .join(".system_generated/logs/transcript.jsonl");
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected, "{}\n").unwrap();

        let cwd = home.path().to_path_buf();
        let query = |sid: Option<&'static str>| crate::agent::TranscriptQuery {
            cwd: &cwd,
            spawn_epoch_secs: 0,
            command: "agy",
            session_id: sid,
            codex_home: None,
        };

        assert_eq!(
            resolve_under_home(home.path(), query(Some(id))),
            Some(expected)
        );

        // A pin whose transcript isn't on disk yet resolves to nothing rather than
        // proximity-matching some other conversation — `^a v` then flashes
        // "no transcript found" and closes.
        assert_eq!(
            resolve_under_home(
                home.path(),
                query(Some("11111111-1111-1111-1111-111111111111"))
            ),
            None
        );
    }

    /// Flatten rendered lines into plain text for assertions.
    fn flatten(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_user_model_tools_and_survives_malformed_tags() {
        use std::io::Write;
        // Write JSONL via write_all (not writeln!) so the literal `{}`
        // braces in the JSON aren't parsed as format specifiers.
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let rows = [
            // Well-formed user input → tags stripped to inner text.
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>hello there</USER_REQUEST>"}"#,
            // Model response with prose + a tool call.
            r#"{"source":"MODEL","type":"PLANNER_RESPONSE","content":"sure thing","tool_calls":[{"name":"read_file"}]}"#,
            // Regression: closing tag before opening — must NOT panic;
            // falls back to the raw content.
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"</USER_REQUEST> oops <USER_REQUEST>"}"#,
        ];
        for r in rows {
            f.write_all(r.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        f.flush().unwrap();

        let lines = render_transcript(f.path(), &Theme::default(), None, true);
        let text = flatten(&lines);

        assert!(text.contains("hello there"), "user content rendered");
        assert!(text.contains("sure thing"), "model content rendered");
        assert!(text.contains("read_file"), "tool call rendered");
        // The malformed line rendered its raw content without panicking
        // (reaching this assertion at all proves the guard works).
        assert!(text.contains("oops"), "malformed line survived");

        // With tool calls hidden, the prose stays but the tool name is gone.
        let hidden = flatten(&render_transcript(f.path(), &Theme::default(), None, false));
        assert!(hidden.contains("sure thing"), "model prose kept");
        assert!(!hidden.contains("read_file"), "tool call hidden");
    }

    #[test]
    fn unwraps_call_mcp_tool_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let line = r#"{"source":"MODEL","type":"PLANNER_RESPONSE","content":"checking","tool_calls":[{"name":"call_mcp_tool","args":{"Arguments":"{}","ServerName":"\"spyc\"","ToolName":"\"get_spyc_context\""}},{"name":"call_mcp_tool","parameters":{"Arguments":{"query":"agent"},"ServerName":"other","ToolName":"search"}},{"name":"read_file","args":{"Path":"\"src/state/agy_transcript.rs\""}}]}"#;
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let text = flatten(&render_transcript(&path, &Theme::default(), None, true));
        assert!(
            text.contains("checking\n\n\u{2699} mcp_spyc_get_spyc_context\n\u{2699} mcp_other_search(agent)\n\u{2699} read_file(src/state/agy_transcript.rs)"),
            "expected blank line after prose and grouped tool calls with details, got:\n{text}"
        );
        assert!(!text.contains("call_mcp_tool"), "got: {text}");
    }

    #[test]
    fn separates_prose_from_subsequent_turn_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let rows = [
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"Running checks now."}"#,
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","tool_calls":[{"name":"run_command","args":{"CommandLine":"\"cargo test\"","toolSummary":"\"Run unit tests\""}}]}"#,
            r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","tool_calls":[{"name":"call_mcp_tool","args":{"Arguments":"{\"id\":1}","ServerName":"\"spyc\"","ToolName":"\"release_scope\""}}]}"#,
            r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","content":"All checks passed."}"#,
        ];
        std::fs::write(&path, rows.join("\n") + "\n").unwrap();

        let text = flatten(&render_transcript(&path, &Theme::default(), None, true));
        let expected = "Running checks now.\n\n\u{2699} run_command(Run unit tests)\n\u{2699} mcp_spyc_release_scope(1)\n\nAll checks passed.";
        assert_eq!(text, expected);
    }

    #[test]
    fn truncated_fields_fall_back_to_transcript_full_by_step_index() {
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        let compact_rows = [
            r#"{"step_index":10,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>trunc...","truncated_fields":["content"]}"#,
            r#"{"step_index":11,"source":"MODEL","type":"PLANNER_RESPONSE","content":"short...","tool_calls":[],"truncated_fields":["content","tool_calls"]}"#,
            r#"{"step_index":12,"source":"MODEL","type":"PLANNER_RESPONSE","content":"compact prose kept","truncated_fields":["thinking"]}"#,
        ];
        std::fs::write(&compact, compact_rows.join("\n") + "\n").unwrap();

        // Without transcript_full.jsonl present, compact content still renders.
        let fallback_text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(fallback_text.contains("trunc..."), "got: {fallback_text}");
        assert!(fallback_text.contains("short..."), "got: {fallback_text}");

        // Write transcript_full.jsonl with extra prefix records and swapped step order.
        let full_rows = [
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"earlier step not in tail"}"#,
            r#"{"step_index":11,"source":"MODEL","type":"PLANNER_RESPONSE","content":"full planner response body","tool_calls":[{"name":"call_mcp_tool","args":{"ServerName":"\"spyc\"","ToolName":"\"git_status\""}}]}"#,
            r#"{"step_index":10,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>full user prompt text</USER_REQUEST>"}"#,
            r#"{"step_index":12,"source":"MODEL","type":"PLANNER_RESPONSE","content":"full step 12 should not be used when only thinking was truncated"}"#,
        ];
        std::fs::write(&full, full_rows.join("\n") + "\n").unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(
            text.contains("full user prompt text"),
            "truncated user input should come from transcript_full.jsonl: {text}"
        );
        assert!(
            text.contains("full planner response body"),
            "truncated planner response should come from transcript_full.jsonl: {text}"
        );
        assert!(
            text.contains("\u{2699} mcp_spyc_git_status"),
            "truncated tool_calls should come from transcript_full.jsonl and unwrap: {text}"
        );
        assert!(
            text.contains("compact prose kept"),
            "record with only thinking truncated should use compact transcript: {text}"
        );
    }

    #[test]
    fn indexes_and_loads_user_media_images_and_filters_non_images() {
        let dir = tempfile::tempdir().unwrap();
        let img1 = dir.path().join("uploaded_media_1.png");
        let img2 = dir.path().join("uploaded_media_2.jpg");
        let pdf = dir.path().join("doc.pdf");
        std::fs::write(&img1, b"\x89PNG\r\n\x1a\nfirst").unwrap();
        std::fs::write(&img2, b"\xff\xd8\xffsecond").unwrap();
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();

        let transcript = dir.path().join("transcript.jsonl");
        let rows = [
            serde_json::json!({
                "step_index": 1,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:13:55Z",
                "content": "<USER_REQUEST>\ncompare [Image #1] and [Image #2] please\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nignored\n</ADDITIONAL_METADATA>",
                "media": [
                    {"mime_type": "application/pdf", "uri": pdf.display().to_string()},
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                    {"mime_type": "image/jpeg", "uri": format!("file://{}", img2.display())},
                ]
            })
            .to_string(),
            // Duplicate replay of step 1 must not double the gallery.
            serde_json::json!({
                "step_index": 1,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:13:55Z",
                "content": "<USER_REQUEST>compare [Image #1] and [Image #2] please</USER_REQUEST>",
                "media": [
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                ]
            })
            .to_string(),
            // Model step with media is ignored (only user attachments belong in ^a g).
            serde_json::json!({
                "step_index": 2,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "content": "model reply",
                "media": [
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                ]
            })
            .to_string(),
            // Separate user step re-attaching the first image stays a distinct row.
            serde_json::json!({
                "step_index": 3,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:20:00Z",
                "content": "<USER_REQUEST>again</USER_REQUEST>",
                "media": [
                    {"uri": img1.display().to_string()},
                ]
            })
            .to_string(),
        ];
        std::fs::write(&transcript, rows.join("\n") + "\n").unwrap();

        let images = index_images(&transcript);
        assert_eq!(images.len(), 3);

        assert_eq!(images[0].seq, 1);
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].bytes_len, 13);
        assert_eq!(images[0].timestamp.as_deref(), Some("2026-10-23T10:13:55Z"));
        assert_eq!(images[0].prompt_excerpt, "compare and please");
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[0]).unwrap(),
            b"\x89PNG\r\n\x1a\nfirst"
        );

        assert_eq!(images[1].seq, 2);
        assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
        assert_eq!(images[1].media_type, "image/jpeg");
        assert_eq!(images[1].bytes_len, 9);
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[1]).unwrap(),
            b"\xff\xd8\xffsecond"
        );

        assert_eq!(images[2].seq, 3);
        assert!(images[2].agent_label.is_none());
        assert_eq!(images[2].media_type, "image");
        assert_eq!(images[2].prompt_excerpt, "again");
    }

    #[test]
    fn truncated_image_step_falls_back_to_transcript_full_and_refuses_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("shot.png");
        std::fs::write(&img, b"png-bytes").unwrap();
        let huge = dir.path().join("huge.png");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(crate::state::transcript_images::MAX_IMAGE_BYTES + 1)
            .unwrap();

        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");
        std::fs::write(
            &compact,
            serde_json::json!({
                "step_index": 7,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>trunc...",
                "truncated_fields": ["content", "media"]
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        std::fs::write(
            &full,
            serde_json::json!({
                "step_index": 7,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T11:00:00Z",
                "content": "<USER_REQUEST>full image prompt [Image #1]</USER_REQUEST>",
                "media": [
                    {"mime_type": "image/png", "uri": img.display().to_string()},
                    {"mime_type": "image/png", "uri": huge.display().to_string()},
                    {"mime_type": "image/png", "uri": dir.path().display().to_string()},
                    {"mime_type": "image/png", "uri": dir.path().join("missing.png").display().to_string()},
                ]
            })
            .to_string()
                + "\n",
        )
        .unwrap();

        let spec = crate::agent::detect("agy")
            .transcript_images()
            .expect("agy declares transcript_images");
        assert!(matches!(
            spec.format,
            crate::state::transcript_images::Format::Agy
        ));

        let images = index_images(&compact);
        assert_eq!(images.len(), 4);
        assert_eq!(images[0].prompt_excerpt, "full image prompt");
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(
            crate::state::transcript_images::load(&compact, &images[0]).unwrap(),
            b"png-bytes"
        );
        // Oversized, directory, and missing paths fail cleanly on load.
        assert!(crate::state::transcript_images::load(&compact, &images[1]).is_err());
        assert_eq!(images[2].bytes_len, 0, "directory is not sized as a file");
        assert!(crate::state::transcript_images::load(&compact, &images[2]).is_err());
        assert!(crate::state::transcript_images::load(&compact, &images[3]).is_err());
    }

    #[test]
    fn load_full_records_skips_unneeded_steps_without_parsing_and_stops_once_found() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        std::fs::write(
            &compact,
            r#"{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"short...","truncated_fields":["content"]}"#.to_owned()
                + "\n",
        )
        .unwrap();

        let padding = "p".repeat(8 * 1024);
        let mut f = std::fs::File::create(&full).unwrap();
        for step in 1..=200 {
            if step == 196 {
                writeln!(
                    f,
                    r#"{{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"hydrated step 196 body"}}"#
                )
                .unwrap();
            } else if step == 200 {
                // Nested "step_index":196 inside content and tool parameters must not
                // trick extract_leading_step into parsing step 200.
                writeln!(
                    f,
                    r#"{{"step_index":200,"source":"MODEL","type":"PLANNER_RESPONSE","content":"quote {{\"step_index\":196}}","tool_calls":[{{"name":"read_file","parameters":{{"step_index":196,"Path":"src/lib.rs"}}}}]}}"#
                )
                .unwrap();
            } else {
                writeln!(
                    f,
                    r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"unneeded {padding}"}}"#
                )
                .unwrap();
            }
        }
        f.flush().unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(text.contains("hydrated step 196 body"), "got: {text}");
        let (lines_inspected, records_parsed, bytes_read) =
            FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert_eq!(
            records_parsed, 1,
            "only the needed step should be parsed in full (got {records_parsed})"
        );
        assert_eq!(
            lines_inspected, 5,
            "backward scan from EOF should inspect steps 200..=196 and stop (got {lines_inspected})"
        );
        assert!(
            bytes_read <= 64 * 1024,
            "backward scan should read only the trailing 64 KiB chunk (got {bytes_read})"
        );
    }

    #[test]
    fn load_full_records_stops_once_below_smallest_needed_step() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        std::fs::write(
            &compact,
            r#"{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"compact 196","truncated_fields":["content"]}"#.to_owned()
                + "\n",
        )
        .unwrap();

        let padding = "p".repeat(8 * 1024);
        let mut f = std::fs::File::create(&full).unwrap();
        for step in 1..=200 {
            if step == 196 {
                // Step 196 in transcript_full.jsonl is not a rendered record type.
                writeln!(
                    f,
                    r#"{{"step_index":196,"source":"SYSTEM","type":"TOOL_RESULT","content":"unrendered"}}"#
                )
                .unwrap();
            } else {
                writeln!(
                    f,
                    r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"unneeded {padding}"}}"#
                )
                .unwrap();
            }
        }
        f.flush().unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(
            text.contains("compact 196") && text.lines().any(|l| l == "[truncated]"),
            "missing rendered step must fall back to compact preview + [truncated], got:\n{text}"
        );
        let (lines_inspected, _records_parsed, bytes_read) =
            FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        // Steps 200..=196 and a few more below 196, since a step can be
        // written after a higher one: never the 200-line file.
        assert!(
            lines_inspected <= 20,
            "backward scan should stop shortly below step 196 (inspected {lines_inspected})"
        );
        assert!(
            bytes_read <= 4 * 64 * 1024,
            "backward scan should stop within a few 64 KiB chunks (read {bytes_read})"
        );
    }

    #[test]
    fn load_full_records_hydrates_older_step_past_unneeded_oversize_line() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        let compact_rows = [
            r#"{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview 196","truncated_fields":["content"]}"#,
            r#"{"step_index":198,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview 198","truncated_fields":["content"]}"#,
        ];
        std::fs::write(&compact, compact_rows.join("\n") + "\n").unwrap();

        let five_mib = "u".repeat(5 * 1024 * 1024);
        let mut f = std::fs::File::create(&full).unwrap();
        writeln!(
            f,
            r#"{{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"hydrated step 196"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"step_index":197,"source":"MODEL","type":"PLANNER_RESPONSE","content":"unneeded {five_mib}"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"step_index":198,"source":"MODEL","type":"PLANNER_RESPONSE","content":"hydrated step 198"}}"#
        )
        .unwrap();
        f.flush().unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        let (_lines_inspected, records_parsed, _bytes_read) =
            FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(
            text.contains("hydrated step 198"),
            "step 198 must be hydrated, got:\n{text}"
        );
        assert!(
            text.contains("hydrated step 196"),
            "step 196 before unneeded 5 MiB step 197 must still be hydrated, got:\n{text}"
        );
        assert_eq!(
            records_parsed, 2,
            "only needed steps 198 and 196 should be parsed (got {records_parsed})"
        );
    }

    #[test]
    fn load_full_records_skips_needed_oversize_step_without_parsing_and_continues() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        let compact_rows = [
            r#"{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview 196","truncated_fields":["content"]}"#,
            r#"{"step_index":198,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview 198","truncated_fields":["content"]}"#,
        ];
        std::fs::write(&compact, compact_rows.join("\n") + "\n").unwrap();

        let five_mib = "x".repeat(5 * 1024 * 1024);
        let mut f = std::fs::File::create(&full).unwrap();
        writeln!(
            f,
            r#"{{"step_index":196,"source":"MODEL","type":"PLANNER_RESPONSE","content":"hydrated step 196"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"step_index":198,"source":"MODEL","type":"PLANNER_RESPONSE","content":"oversize-198 {five_mib}"}}"#
        )
        .unwrap();
        f.flush().unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        let (_lines_inspected, records_parsed, _bytes_read) =
            FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(
            !text.contains("oversize-198"),
            "needed 5 MiB step 198 must not be hydrated"
        );
        assert!(
            text.contains("preview 198") && text.lines().any(|l| l == "[truncated]"),
            "needed 5 MiB step 198 must render its compact preview and [truncated], got:\n{text}"
        );
        assert!(
            text.contains("hydrated step 196"),
            "scan must continue past oversize needed step 198 and hydrate step 196, got:\n{text}"
        );
        assert_eq!(
            records_parsed, 1,
            "needed 5 MiB step 198 must never be parsed in full (got {records_parsed})"
        );
    }

    #[test]
    fn load_full_records_caps_hydrated_bytes_and_marks_unhydrated_truncation() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        let compact_rows = [
            r#"{"step_index":18,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview eighteen\n```rust\nlet unclosed = 1;","truncated_fields":["content"]}"#,
            r#"{"step_index":19,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview nineteen","truncated_fields":["content"]}"#,
            r#"{"step_index":20,"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview twenty","truncated_fields":["content"]}"#,
        ];
        std::fs::write(&compact, compact_rows.join("\n") + "\n").unwrap();

        let chunk = "x".repeat(2 * 1024 * 1024 - 256);
        let mut f = std::fs::File::create(&full).unwrap();
        for step in 1..=17 {
            writeln!(
                f,
                r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"earlier {step}"}}"#
            )
            .unwrap();
        }
        for (step, tag) in [
            (18, "over-cap-eighteen"),
            (19, "hydrated-nineteen"),
            (20, "hydrated-twenty"),
        ] {
            writeln!(
                f,
                r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"{tag} {chunk}"}}"#
            )
            .unwrap();
        }
        f.flush().unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        let (lines_inspected, records_parsed, _bytes_read) =
            FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(
            text.contains("hydrated-twenty"),
            "newest record is hydrated first"
        );
        assert!(
            text.contains("hydrated-nineteen"),
            "second newest record fits in the 4 MiB budget"
        );
        assert!(
            !text.contains("over-cap-eighteen"),
            "oldest record exceeds the 4 MiB hydration cap and must not be hydrated"
        );
        assert!(
            text.contains("preview eighteen") && text.lines().any(|l| l == "[truncated]"),
            "unhydrated truncated step must show its compact preview and a standalone [truncated] line outside any code fence, got:\n{text}"
        );
        assert_eq!(
            records_parsed, 2,
            "must not parse the record that pushes hydration past the byte cap"
        );
        assert_eq!(
            lines_inspected, 3,
            "must stop scanning backward once the hydration byte cap is reached"
        );
    }

    #[test]
    fn index_images_enforces_scan_budget_and_keeps_newest_images() {
        use std::io::{Seek, SeekFrom, Write};
        let dir = tempfile::tempdir().unwrap();
        let old_img = dir.path().join("old_before_window.png");
        let mid_img = dir.path().join("mid_in_tail.png");
        let new1_img = dir.path().join("new1_in_tail.png");
        let new2_img = dir.path().join("new2_in_tail.png");
        for p in [&old_img, &mid_img, &new1_img, &new2_img] {
            std::fs::write(p, b"\x89PNG\r\n\x1a\nimg").unwrap();
        }

        let transcript = dir.path().join("transcript.jsonl");
        let mut f = std::fs::File::create(&transcript).unwrap();
        let old_rec = serde_json::json!({
            "step_index": 1,
            "source": "USER_EXPLICIT",
            "type": "USER_INPUT",
            "content": "<USER_REQUEST>old before window</USER_REQUEST>",
            "media": [{"mime_type": "image/png", "uri": old_img.display().to_string()}],
        })
        .to_string();
        writeln!(f, "{old_rec}").unwrap();

        let budget = 4 * 1024u64;
        let tail_end = (old_rec.len() as u64) + 1 + budget;
        f.set_len(tail_end).unwrap();
        f.seek(SeekFrom::Start(tail_end - 1024)).unwrap();
        f.write_all(b"\n").unwrap();
        let mid_rec = serde_json::json!({
            "step_index": 98,
            "source": "USER_EXPLICIT",
            "type": "USER_INPUT",
            "content": "<USER_REQUEST>mid in tail</USER_REQUEST>",
            "media": [{"mime_type": "image/png", "uri": mid_img.display().to_string()}],
        })
        .to_string();
        let new_rec = serde_json::json!({
            "step_index": 99,
            "source": "USER_EXPLICIT",
            "type": "USER_INPUT",
            "content": "<USER_REQUEST>newest two [Image #1] [Image #2]</USER_REQUEST>",
            "media": [
                {"mime_type": "image/png", "uri": new1_img.display().to_string()},
                {"mime_type": "image/png", "uri": new2_img.display().to_string()},
            ],
        })
        .to_string();
        writeln!(f, "{mid_rec}").unwrap();
        writeln!(f, "{new_rec}").unwrap();
        f.flush().unwrap();

        let (candidates, _, _) =
            scan_image_candidates(std::fs::File::open(&transcript).unwrap(), budget);
        assert_eq!(
            candidates.len(),
            2,
            "only records in the trailing {budget} bytes should be scanned, got: {}",
            candidates.len()
        );

        let images = index_images_with_limits(&transcript, budget, 2);
        assert_eq!(
            images.len(),
            2,
            "image cap should return 2 images, got: {images:?}"
        );
        assert_eq!(images[0].seq, 1);
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(images[1].seq, 2);
        assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
    }

    #[test]
    fn index_images_resolves_relative_uris_percent_decoding_and_file_localhost() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("session/.system_generated/logs");
        let uploaded_dir = dir.path().join("session/.user_uploaded");
        std::fs::create_dir_all(logs_dir.join("sub")).unwrap();
        std::fs::create_dir_all(&uploaded_dir).unwrap();

        let rel_logs_img = logs_dir.join("sub/from_logs.png");
        let rel_session_img = uploaded_dir.join("from_session.png");
        let pct_img = logs_dir.join("space & hash #1.png");
        let localhost_img = logs_dir.join("localhost.png");
        std::fs::write(&rel_logs_img, b"logs-rel").unwrap();
        std::fs::write(&rel_session_img, b"session-rel").unwrap();
        std::fs::write(&pct_img, b"pct-decoded").unwrap();
        std::fs::write(&localhost_img, b"localhost-bytes").unwrap();

        let pct_uri = format!(
            "file://{}/space%20%26%20hash%20%231.png",
            logs_dir.display()
        );
        let localhost_uri = format!("file://localhost{}", localhost_img.display());
        let remote_host_uri = format!("file://otherhost{}", localhost_img.display());

        let mut media = vec![
            serde_json::json!({"mime_type": "image/png", "uri": "sub/from_logs.png"}),
            serde_json::json!({"mime_type": "image/png", "uri": ".user_uploaded/from_session.png"}),
            serde_json::json!({"mime_type": "image/png", "uri": pct_uri}),
            serde_json::json!({"mime_type": "image/png", "uri": localhost_uri}),
            serde_json::json!({"mime_type": "image/png", "uri": remote_host_uri}),
        ];

        // Indexing records a path without opening it, so a non-UTF-8 name is
        // checked on the decoded path: APFS refuses to create one.
        #[cfg(unix)]
        media.push(serde_json::json!({
            "mime_type": "image/png",
            "uri": "file:///nonexistent-spyc/raw_%80_byte.png",
        }));

        let transcript = logs_dir.join("transcript.jsonl");
        let rec = serde_json::json!({
            "step_index": 1,
            "source": "USER_EXPLICIT",
            "type": "USER_INPUT",
            "content": "<USER_REQUEST>valid uris and one remote host</USER_REQUEST>",
            "media": media,
        })
        .to_string();
        std::fs::write(&transcript, format!("{rec}\n")).unwrap();

        let images = index_images(&transcript);
        let expected_len = if cfg!(unix) { 5 } else { 4 };
        assert_eq!(
            images.len(),
            expected_len,
            "non-localhost file:// host must be rejected"
        );
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[0]).unwrap(),
            b"logs-rel"
        );
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[1]).unwrap(),
            b"session-rel"
        );
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[2]).unwrap(),
            b"pct-decoded"
        );
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[3]).unwrap(),
            b"localhost-bytes"
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let crate::state::transcript_images::ImageSource::Agy { path } = &images[4].source
            else {
                panic!("expected an agy image source");
            };
            assert_eq!(
                path.as_os_str().as_bytes(),
                b"/nonexistent-spyc/raw_\x80_byte.png"
            );
        }
    }

    /// Writes `rows` as `transcript_full.jsonl` beside `compact` and returns
    /// each row's byte offset.
    fn write_full(compact: &Path, rows: &[String]) -> Vec<u64> {
        let mut offsets = Vec::with_capacity(rows.len());
        let mut body = String::new();
        for row in rows {
            offsets.push(body.len() as u64);
            body.push_str(row);
            body.push('\n');
        }
        std::fs::write(compact.with_file_name("transcript_full.jsonl"), body).unwrap();
        offsets
    }

    fn planner_row(step: u64, content: &str) -> String {
        format!(
            r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"{content}"}}"#
        )
    }

    fn truncated_row(step: u64) -> String {
        format!(
            r#"{{"step_index":{step},"source":"MODEL","type":"PLANNER_RESPONSE","content":"preview {step}","truncated_fields":["content"]}}"#
        )
    }

    /// The backward scan reads in 64 KiB chunks, and a chunk boundary can fall
    /// inside an oversize line's leading `{"step_index":197`. The step it reads
    /// must still be 197: read as 1, it ends the scan above the older needed 196.
    /// That holds whether the line was already oversize before its first bytes
    /// arrived (5 MiB) or tips over on them (just past the 4 MiB cap).
    #[test]
    fn an_oversize_line_split_inside_its_step_index_keeps_its_step() {
        const CHUNK: usize = 64 * 1024;
        let cap = crate::state::MAX_TRANSCRIPT_TAIL_BYTES as usize;
        for line_len in [5 * 1024 * 1024, cap + 10] {
            for split_at in [1, 15] {
                let dir = tempfile::tempdir().unwrap();
                let compact = dir.path().join("transcript.jsonl");
                std::fs::write(
                    &compact,
                    [truncated_row(196), truncated_row(198)].join("\n") + "\n",
                )
                .unwrap();
                let filler = line_len - planner_row(197, "").len();
                let unneeded = planner_row(197, &"u".repeat(filler));
                assert_eq!(unneeded.len(), line_len);
                let newest =
                    |pad: usize| planner_row(198, &format!("hydrated 198{}", " ".repeat(pad)));
                let unpadded = unneeded.len() + 1 + newest(0).len() + 1 - split_at;
                let pad = (CHUNK - unpadded % CHUNK) % CHUNK;
                write_full(
                    &compact,
                    &[planner_row(196, "hydrated 196"), unneeded, newest(pad)],
                );

                let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
                assert!(
                    text.contains("hydrated 198") && text.contains("hydrated 196"),
                    "line of {line_len} bytes split {split_at} bytes in: {}",
                    text.lines().take(6).collect::<Vec<_>>().join(" | ")
                );
            }
        }
    }

    /// The scan reads no more than its window, and a line that starts exactly
    /// at the window's floor is whole, so it is hydrated. One byte less and the
    /// line is cut, so it is not.
    #[test]
    #[ignore = "red: load_full_records reads the whole file and ignores its scan window"]
    fn the_full_scan_stays_inside_its_window_and_keeps_a_line_at_its_floor() {
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let mut rows = vec![planner_row(5, "hydrated 5"), planner_row(10, "hydrated 10")];
        rows.extend((11..=60).map(|step| planner_row(step, &"p".repeat(80))));
        rows.push(planner_row(70, "hydrated 70"));
        let offsets = write_full(&compact, &rows);
        let file_len = std::fs::metadata(compact.with_file_name("transcript_full.jsonl"))
            .unwrap()
            .len();
        let needed: HashSet<u64> = [5, 10, 70].into();

        let window = file_len - offsets[1];
        let found = load_full_records(&compact, &needed, true, window);
        let (_, _, bytes_read) = FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(found.contains_key(&70) && found.contains_key(&10));
        assert!(!found.contains_key(&5), "step 5 lies below the window");
        assert!(
            (bytes_read as u64) <= window,
            "read {bytes_read} of a {window}-byte window"
        );

        let found = load_full_records(&compact, &needed, true, window - 1);
        let (_, _, bytes_read) = FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(found.contains_key(&70));
        assert!(
            !found.contains_key(&10),
            "step 10 starts one byte below the window"
        );
        assert!((bytes_read as u64) < window);
    }

    /// Agy can write a step after a higher one; this module's own fixture runs
    /// 1, 11, 10, 12. The scan must not stop at the first lower step it meets.
    #[test]
    fn a_step_written_after_a_higher_one_is_still_hydrated() {
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        std::fs::write(&compact, truncated_row(11) + "\n").unwrap();
        write_full(
            &compact,
            &[
                planner_row(1, "one"),
                planner_row(11, "hydrated 11"),
                planner_row(10, "ten"),
                planner_row(12, "twelve"),
            ],
        );
        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(text.contains("hydrated 11"), "got: {text}");
    }

    /// Once the hydration budget is spent no later needed step can be
    /// hydrated, so the scan stops reading instead of walking the window.
    #[test]
    #[ignore = "red: load_full_records reads the whole file however much it has hydrated"]
    fn the_full_scan_stops_reading_once_the_hydration_budget_is_spent() {
        let cap = crate::state::MAX_TRANSCRIPT_TAIL_BYTES as usize;
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        std::fs::write(
            &compact,
            [truncated_row(1), truncated_row(200)].join("\n") + "\n",
        )
        .unwrap();
        let mut rows = vec![planner_row(1, "hydrated 1")];
        rows.extend((2..=150).map(|step| planner_row(step, &"p".repeat(16 * 1024))));
        let filler = cap - planner_row(200, "budget 200 ").len();
        rows.push(planner_row(
            200,
            &format!("budget 200 {}", "x".repeat(filler)),
        ));
        assert_eq!(rows.last().unwrap().len(), cap);
        write_full(&compact, &rows);

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        let (_, _, bytes_read) = FULL_LOAD_COUNTS.with(std::cell::Cell::get);
        assert!(text.contains("budget 200"));
        assert!(
            !text.contains("hydrated 1"),
            "step 1 is past the spent budget"
        );
        assert!(
            bytes_read <= cap + 2 * 64 * 1024,
            "read {bytes_read} bytes after a {cap}-byte budget was spent"
        );
    }

    #[cfg(unix)]
    #[test]
    fn named_pipe_at_transcript_path_returns_immediately_without_blocking() {
        use std::os::unix::fs::OpenOptionsExt;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("transcript.jsonl");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );

        let full_dir = tempfile::tempdir().unwrap();
        let compact_with_trunc = full_dir.path().join("transcript.jsonl");
        let full_fifo = full_dir.path().join("transcript_full.jsonl");
        std::fs::write(
            &compact_with_trunc,
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>short</USER_REQUEST>","media":[{"mime_type":"image/png","uri":"/tmp/x.png"}],"truncated_fields":["content","media"]}"#.to_owned()
                + "\n",
        )
        .unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&full_fifo)
                .status()
                .unwrap()
                .success()
        );

        let (tx, rx) = std::sync::mpsc::channel();
        let fifo_clone = fifo.clone();
        std::thread::spawn(move || {
            let imgs1 = index_images(&fifo_clone);
            let lines1 = render_transcript(&fifo_clone, &Theme::default(), None, true);
            let imgs2 = index_images(&compact_with_trunc);
            let lines2 = render_transcript(&compact_with_trunc, &Theme::default(), None, true);
            let _ = tx.send((imgs1.len(), lines1.len(), imgs2.len(), !lines2.is_empty()));
        });

        let result = rx.recv_timeout(Duration::from_secs(5));
        if result.is_err() {
            for pipe in [&fifo, &full_fifo] {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(pipe);
            }
        }
        assert_eq!(
            result,
            Ok((0, 0, 1, true)),
            "FIFO at transcript.jsonl or transcript_full.jsonl blocked the worker instead of returning immediately"
        );
    }
}
