//! Index the images an agent actually received, out of its on-disk transcript.
//!
//! This is the record that answers "what *was* `[Image #3]`?" after the prompt
//! is long gone: the JSONL keeps the base64 next to the prompt text that
//! referenced it, so the pairing survives spyc restarts and `claude --resume`.
//!
//! Two passes by design. [`index`] streams the file once and keeps only
//! *metadata* per image — a multi-MB transcript is mostly base64, and holding
//! every image would cost more memory than the images are worth. Each entry
//! records where to find its bytes again; [`load`] re-reads that one record when
//! the user actually opens it.
//!
//! ## What the format does NOT give you
//!
//! `[Image #N]` reads like a stable id and is not one, so spyc numbers the
//! images itself and keeps the agent's label only for cross-reference:
//!
//! - The counter **restarts** mid-file when the conversation is cleared or
//!   resumed — one observed transcript runs to `#12` and then starts again at
//!   `#1`.
//! - A single record can carry **several** images (`[Image #6] [Image #7]` with
//!   two image blocks), paired positionally with the blocks.
//! - Some images carry **no label at all**.
//! - `type:"attachment"` records duplicate an image — sometimes *before* the
//!   `user` record, sometimes as the only copy of it, sometimes twice over. So
//!   neither "skip attachments" nor "dedupe by label" is correct; dedupe is on
//!   content identity.

use std::io::BufReader;
use std::path::Path;

/// How much of a base64 payload to read from each end when fingerprinting it
/// for dedupe. Two different images sharing an exact length *and* both ends is
/// not a case worth defending against; reading the whole payload to compare is.
const FINGERPRINT_EDGE: usize = 64;

/// One image found in a transcript, without its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptImage {
    /// spyc's own 1-based position in the transcript — the stable handle the
    /// gallery shows and the user types. Not the agent's `[Image #N]`.
    pub seq: usize,
    /// The agent's own label (`"#3"`) when the prompt carried one, for
    /// cross-referencing against what it printed. Not unique; see the module
    /// docs.
    pub agent_label: Option<String>,
    /// e.g. `image/png`.
    pub media_type: String,
    /// Decoded size, derived from the base64 length without decoding.
    pub bytes_len: usize,
    /// ISO-8601 stamp from the record, when present.
    pub timestamp: Option<String>,
    /// The prompt text this image arrived with, trimmed for a list row.
    pub prompt_excerpt: String,
    /// Where to read it back: 0-based line, then which image block in that line.
    pub line: usize,
    pub block: usize,
}

/// Longest prompt excerpt kept per row — enough to recognize the message,
/// short enough not to dominate the list.
const EXCERPT_MAX: usize = 90;

/// Stream `path` and index every image in it, oldest first.
///
/// Returns an empty vec for a missing/unreadable file or a transcript with no
/// images — "nothing to show" is a normal state here, not an error worth
/// surfacing.
pub fn index(path: &Path) -> Vec<TranscriptImage> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut seen: Vec<u64> = Vec::new();
    let mut out: Vec<TranscriptImage> = Vec::new();
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut oversized = 0usize;
    // Bounded per line, NOT `lines()`: a file with no newline in it would be
    // read whole. See `read_line_capped`.
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
        // Cheap reject first: only a handful of lines in a transcript hold an
        // image, and parsing a multi-hundred-KB JSON line to discover otherwise
        // is the whole cost of this pass.
        if !line.contains("\"type\":\"image\"") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        index_record(&record, line_no, &mut seen, &mut out);
    }
    if oversized > 0 {
        // Not silent: a skipped record could have held an image, and a gallery
        // that quietly omits one is worse than one that says it did.
        crate::debug_log::log(&format!(
            "transcript_images: skipped {oversized} record(s) over {} bytes in {}",
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
            path.display()
        ));
    }
    out
}

/// Pull the content blocks out of whichever wrapper this record uses. A `user`
/// message carries them at `message.content`; an `attachment` re-states the
/// same prompt at `attachment.prompt`.
fn content_blocks(record: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    match record.get("type").and_then(serde_json::Value::as_str) {
        Some("user") => record.pointer("/message/content"),
        Some("attachment") => record.pointer("/attachment/prompt"),
        _ => None,
    }
    .and_then(serde_json::Value::as_array)
}

fn index_record(
    record: &serde_json::Value,
    line_no: usize,
    seen: &mut Vec<u64>,
    out: &mut Vec<TranscriptImage>,
) {
    let Some(blocks) = content_blocks(record) else {
        return;
    };
    let text: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let labels = image_labels(&text);
    let timestamp = record
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let excerpt = excerpt_of(&text);

    for (nth, block) in blocks
        .iter()
        .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("image"))
        .enumerate()
    {
        let Some(source) = block.get("source") else {
            continue;
        };
        let Some(data) = source.get("data").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let fp = fingerprint(data);
        if seen.contains(&fp) {
            continue;
        }
        seen.push(fp);
        out.push(TranscriptImage {
            seq: out.len() + 1,
            // Labels pair with image blocks positionally: "[Image #6] [Image #7]"
            // alongside two blocks means the first block is #6.
            agent_label: labels.get(nth).cloned(),
            media_type: source
                .get("media_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("image")
                .to_string(),
            bytes_len: decoded_len(data),
            timestamp: timestamp.clone(),
            prompt_excerpt: excerpt.clone(),
            line: line_no,
            block: nth,
        });
    }
}

/// Read one indexed image's bytes back out of the transcript.
///
/// Re-reads the file to `entry.line` rather than holding bytes from the index
/// pass — the point of the split. `Err` carries a reason for the status line.
pub fn load(path: &Path, entry: &TranscriptImage) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    // Same bound as the index pass — this walks the same records.
    for _ in 0..=entry.line {
        match crate::state::read_line_capped(
            &mut reader,
            &mut buf,
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
        ) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err("transcript changed under us (line is gone)".to_string());
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    let line = std::str::from_utf8(&buf).map_err(|e| format!("read: {e}"))?;
    let record: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("parse: {e}"))?;
    let blocks = content_blocks(&record).ok_or_else(|| "no content blocks".to_string())?;
    let data = blocks
        .iter()
        .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("image"))
        .nth(entry.block)
        .and_then(|b| b.pointer("/source/data"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "image block is gone".to_string())?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("base64: {e}"))
}

/// The `[Image #N]` labels in a prompt, in the order they appear.
fn image_labels(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[Image #") {
        let after = &rest[start + "[Image #".len()..];
        let Some(end) = after.find(']') else { break };
        let digits = &after[..end];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            out.push(format!("#{digits}"));
        }
        rest = &after[end + 1..];
    }
    out
}

/// The prompt text without its `[Image #N]` markers (the gallery shows those in
/// their own column), collapsed to one line and clipped.
fn excerpt_of(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[Image #") {
        cleaned.push_str(&rest[..start]);
        let after = &rest[start..];
        // An unterminated marker means the rest of the text is inside it; drop
        // the remainder rather than looping forever on the same `start`.
        let Some(end) = after.find(']') else {
            rest = "";
            break;
        };
        rest = &after[end + 1..];
    }
    cleaned.push_str(rest);
    let flat = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= EXCERPT_MAX {
        return flat;
    }
    let clipped: String = flat.chars().take(EXCERPT_MAX.saturating_sub(1)).collect();
    format!("{clipped}\u{2026}")
}

/// Decoded byte count from a base64 length — no decode, so the index pass stays
/// cheap. Standard base64 with `=` padding.
fn decoded_len(b64: &str) -> usize {
    let padding = b64.bytes().rev().take_while(|&c| c == b'=').count();
    b64.len() / 4 * 3 - padding.min(2)
}

/// Content identity for dedupe: length plus both ends of the payload.
fn fingerprint(data: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.len().hash(&mut h);
    let edge = FINGERPRINT_EDGE.min(data.len());
    data[..edge].hash(&mut h);
    data[data.len() - edge..].hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1x1 PNG as base64 — small enough to inline, real enough to decode.
    const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    fn user_line(text: &str, images: &[&str], ts: &str) -> String {
        let mut content = vec![serde_json::json!({"type": "text", "text": text})];
        for b64 in images {
            content.push(serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": b64},
            }));
        }
        serde_json::json!({"type": "user", "timestamp": ts, "message": {"content": content}})
            .to_string()
    }

    fn attachment_line(text: &str, b64: &str) -> String {
        serde_json::json!({
            "type": "attachment",
            "attachment": {"prompt": [
                {"type": "text", "text": text},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64}},
            ]},
        })
        .to_string()
    }

    fn write(lines: &[String]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, lines.join("\n")).expect("write transcript");
        (dir, path)
    }

    /// An `attachment` twin is the SAME image, so it must not double the
    /// gallery — and it can arrive either side of the `user` record.
    #[test]
    fn an_attachment_twin_is_not_a_second_image() {
        let (_d, path) = write(&[
            attachment_line("look: [Image #3]", PNG_B64),
            user_line("look: [Image #3]", &[PNG_B64], "2026-08-04T15:48:10Z"),
        ]);
        let images = index(&path);
        assert_eq!(images.len(), 1, "one image, indexed once");
        assert_eq!(images[0].seq, 1);
    }

    /// The observed failure mode this guards: the agent's counter restarts, so
    /// two different images can both be `#1`. Both must appear, numbered by
    /// spyc's own sequence.
    #[test]
    fn a_restarted_agent_counter_does_not_collapse_two_images() {
        let other = PNG_B64.replace("iVBORw0", "iVBORw1");
        let (_d, path) = write(&[
            user_line("first [Image #1]", &[PNG_B64], "2026-08-04T10:00:00Z"),
            user_line(
                "after a clear [Image #1]",
                &[&other],
                "2026-08-04T20:00:00Z",
            ),
        ]);
        let images = index(&path);
        assert_eq!(images.len(), 2, "same label, different images");
        assert_eq!((images[0].seq, images[1].seq), (1, 2));
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(images[1].agent_label.as_deref(), Some("#1"));
    }

    /// Two images in one record pair with their labels positionally.
    #[test]
    fn two_images_in_one_record_take_their_labels_in_order() {
        let second = PNG_B64.replace("iVBORw0", "iVBORw1");
        let (_d, path) = write(&[user_line(
            "compare [Image #6] with [Image #7]",
            &[PNG_B64, &second],
            "2026-08-04T11:00:00Z",
        )]);
        let images = index(&path);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].agent_label.as_deref(), Some("#6"));
        assert_eq!(images[1].agent_label.as_deref(), Some("#7"));
        assert_eq!((images[0].block, images[1].block), (0, 1));
    }

    /// An unlabelled image is still an image — it must not be dropped just
    /// because the prompt never named it.
    #[test]
    fn an_unlabelled_image_is_still_indexed() {
        let (_d, path) = write(&[user_line("here", &[PNG_B64], "2026-08-04T12:00:00Z")]);
        let images = index(&path);
        assert_eq!(images.len(), 1);
        assert!(images[0].agent_label.is_none());
        assert_eq!(images[0].prompt_excerpt, "here");
    }

    /// The excerpt drops the markers (they get their own column) and collapses
    /// whitespace, so a row stays one readable line.
    #[test]
    fn the_excerpt_strips_markers_and_flattens_whitespace() {
        let (_d, path) = write(&[user_line(
            "align   the\ncolumns [Image #3] please",
            &[PNG_B64],
            "2026-08-04T13:00:00Z",
        )]);
        let images = index(&path);
        assert_eq!(images[0].prompt_excerpt, "align the columns please");
    }

    /// Round-trip: what the index points at must decode back to real bytes.
    #[test]
    fn an_indexed_image_loads_back_as_decodable_bytes() {
        let second = PNG_B64.replace("iVBORw0", "iVBORw1");
        let (_d, path) = write(&[user_line(
            "two [Image #1] [Image #2]",
            &[PNG_B64, &second],
            "2026-08-04T14:00:00Z",
        )]);
        let images = index(&path);
        let bytes = load(&path, &images[0]).expect("load the first image");
        assert_eq!(&bytes[1..4], b"PNG", "decodes to a real PNG header");
        // The second block must load the SECOND image, not the first again.
        let other = load(&path, &images[1]).expect("load the second image");
        assert_ne!(bytes, other);
    }

    /// Reported size comes from the base64 length, so the index never decodes.
    #[test]
    fn decoded_length_is_derived_without_decoding() {
        use base64::Engine;
        let actual = base64::engine::general_purpose::STANDARD
            .decode(PNG_B64)
            .expect("fixture decodes");
        assert_eq!(decoded_len(PNG_B64), actual.len());
    }

    /// A monstrous record must not cost the images after it: the index has to
    /// skip it and carry on, not stop or mis-split the rest of the file.
    #[test]
    fn an_oversized_record_does_not_hide_the_images_after_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let mut data = user_line("first", &[PNG_B64], "2026-08-04T10:00:00Z");
        data.push('\n');
        // A single line far past the cap, with no newline inside it.
        data.push_str(&"x".repeat(crate::state::MAX_TRANSCRIPT_LINE_BYTES + 1024));
        data.push('\n');
        let other = PNG_B64.replace("iVBORw0", "iVBORw1");
        data.push_str(&user_line("second", &[&other], "2026-08-04T11:00:00Z"));
        std::fs::write(&path, data).expect("write transcript");

        let images = index(&path);
        assert_eq!(
            images.len(),
            2,
            "the record after the huge line is still indexed"
        );
        assert_eq!(images[1].prompt_excerpt, "second");
        // And its bytes still load — the line index survived the skip.
        let bytes = load(&path, &images[1]).expect("load the second image");
        assert_eq!(&bytes[1..4], b"PNG");
    }

    /// A transcript with no images, a missing file, and a corrupt line are all
    /// "nothing to show", not errors.
    #[test]
    fn absent_or_broken_input_yields_no_images() {
        assert!(index(Path::new("/nonexistent/session.jsonl")).is_empty());
        let (_d, path) = write(&[
            serde_json::json!({"type": "user", "message": {"content": "plain text"}}).to_string(),
            "{ not json at all \"type\":\"image\"".to_string(),
        ]);
        assert!(index(&path).is_empty());
    }

    /// Loading against an index whose line no longer holds that image reports
    /// the staleness instead of panicking or returning wrong bytes.
    #[test]
    fn a_stale_index_entry_reports_rather_than_lies() {
        let (_d, path) = write(&[user_line("x", &[PNG_B64], "2026-08-04T15:00:00Z")]);
        let images = index(&path);
        let mut stale = images[0].clone();
        stale.line = 99;
        let err = load(&path, &stale).expect_err("line 99 does not exist");
        assert!(err.contains("transcript changed"), "got: {err}");

        let mut wrong_block = images[0].clone();
        wrong_block.block = 7;
        let err = load(&path, &wrong_block).expect_err("block 7 does not exist");
        assert!(err.contains("image block is gone"), "got: {err}");
    }
}
