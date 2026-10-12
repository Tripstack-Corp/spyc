//! Codex gallery metadata, with bounded records and parent-rollout provenance.
mod records;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::transcript_images::{ImageSource, MAX_IMAGE_BYTES, TranscriptImage};
use records::{Input, Record};

const SCAN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_IMAGES: usize = 4096;

/// A record location survives a fork without confusing its parent's bytes with
/// the child's file. The hash rejects a rewritten record before opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub rollout: PathBuf,
    pub offset: u64,
    pub block: usize,
    pub record_hash: u64,
    pub cwd: Option<PathBuf>,
}

struct Group {
    kinds: u8,
    prompt_hash: u64,
    rows: Vec<usize>,
    identities: Vec<Option<u64>>,
}

/// Index ancestors only through their branch offsets, then the current rollout.
/// Each file has its own duplicate identities; two separate submissions of the
/// same image remain two rows.
pub fn index(path: &Path) -> Vec<TranscriptImage> {
    let mut chain = vec![(path.to_path_buf(), u64::MAX)];
    let mut seen = HashSet::from([path.to_path_buf()]);
    let mut child = path.to_path_buf();
    for _ in 0..32 {
        let Some((parent, end)) = super::codex_history::fork_base(&child) else {
            break;
        };
        if !seen.insert(parent.clone()) {
            break;
        }
        chain.push((parent.clone(), end));
        child = parent;
    }
    let mut out = Vec::new();
    let mut budget = SCAN_BYTES;
    for (file, end) in chain.into_iter().rev() {
        index_file(&file, end, &mut budget, &mut out);
    }
    if budget == 0 || out.len() == MAX_IMAGES {
        crate::debug_log::log("codex_images: gallery scan reached its byte or image limit");
    }
    out
}

fn index_file(path: &Path, end: u64, budget: &mut u64, out: &mut Vec<TranscriptImage>) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let Ok(meta) = file.metadata() else { return };
    if !meta.is_file() {
        return;
    }
    let length = meta.len().min(end).min(*budget);
    *budget -= length;
    let mut reader = BufReader::new(file.take(length));
    let mut buf = Vec::new();
    let mut cwd = None;
    let mut seen = HashSet::new();
    let mut group = None;
    for line in 0.. {
        if out.len() >= MAX_IMAGES {
            break;
        }
        let offset = length - reader.get_ref().limit() - reader.buffer().len() as u64;
        match super::read_line_capped(&mut reader, &mut buf, super::MAX_TRANSCRIPT_LINE_BYTES) {
            Ok(None) | Err(_) => break,
            Ok(Some(false)) => {
                crate::debug_log::log("codex_images: skipped oversized rollout record");
                group = None;
                continue;
            }
            Ok(Some(true)) => {}
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            continue;
        };
        if value["type"] == "session_meta" {
            cwd = value["payload"]["cwd"]
                .as_str()
                .map(PathBuf::from)
                .filter(|p| p.is_absolute());
        }
        if matches!(
            value["payload"]["type"].as_str(),
            Some("turn_complete" | "turn_aborted")
        ) {
            group = None;
        }
        let Some(record) = records::decode(&value) else {
            continue;
        };
        if let Some(id) = record.id.filter(|id| !id.is_empty())
            && !seen.insert((record.kind, id.to_owned()))
        {
            continue;
        }
        let location = Location {
            rollout: path.to_path_buf(),
            offset,
            block: 0,
            record_hash: hash(&buf),
            cwd: cwd.clone(),
        };
        let timestamp = value["timestamp"].as_str().map(str::to_owned);
        index_record(record, location, line, timestamp, &mut group, out);
    }
}

fn index_record(
    record: Record<'_>,
    location: Location,
    line: usize,
    timestamp: Option<String>,
    group: &mut Option<Group>,
    out: &mut Vec<TranscriptImage>,
) {
    let labels = super::transcript_images::image_labels(&record.text);
    let mut normalized = record.text.clone();
    for label in &labels {
        normalized = normalized.replace(&format!("[Image {label}]"), "");
    }
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let prompt_hash = hash(normalized.as_bytes());
    let identities = record
        .images
        .iter()
        .map(|input| identity(*input, location.cwd.as_deref()))
        .collect::<Vec<_>>();
    if let Some(previous) = group.as_mut()
        && previous.kinds & record.kind == 0
        && previous.prompt_hash == prompt_hash
        && previous.rows.len() == record.images.len()
        && previous
            .identities
            .iter()
            .zip(&identities)
            .all(|(old, new)| old.is_some() && old == new)
    {
        for (block, row) in previous.rows.iter().enumerate() {
            let ImageSource::Codex { primary, fallback } = &mut out[*row].source else {
                continue;
            };
            let mut twin = location.clone();
            twin.block = block;
            // Keep durable inline bytes as a fallback when Codex's temporary
            // clipboard file disappears after a restart.
            if record.kind == records::API {
                *fallback = Some(Box::new(twin));
            } else if previous.kinds == records::API {
                *fallback = Some(primary.clone());
                **primary = twin;
            }
            if record.kind != records::API {
                out[*row].agent_label = labels.get(block).cloned();
                out[*row].prompt_excerpt = super::transcript_images::excerpt_of(&record.text);
                if timestamp.is_some() {
                    out[*row].timestamp.clone_from(&timestamp);
                }
            }
        }
        previous.kinds |= record.kind;
        return;
    }
    let mut rows = Vec::new();
    for (block, input) in record
        .images
        .iter()
        .enumerate()
        .take(MAX_IMAGES.saturating_sub(out.len()))
    {
        let (media_type, bytes_len) = metadata(*input, location.cwd.as_deref());
        rows.push(out.len());
        let mut primary = location.clone();
        primary.block = block;
        out.push(TranscriptImage {
            seq: out.len() + 1,
            agent_label: labels.get(block).cloned(),
            media_type,
            bytes_len,
            timestamp: timestamp.clone(),
            prompt_excerpt: super::transcript_images::excerpt_of(&record.text),
            line,
            block,
            source: ImageSource::Codex {
                primary: Box::new(primary),
                fallback: None,
            },
        });
    }
    *group = Some(Group {
        kinds: record.kind,
        prompt_hash,
        rows,
        identities,
    });
}

fn identity(input: Input<'_>, cwd: Option<&Path>) -> Option<u64> {
    match input {
        Input::Local(path)
        | Input::Inline {
            path: Some(path), ..
        } => {
            use std::os::unix::ffi::OsStrExt;
            local_path(path, cwd).map(|path| hash(path.as_os_str().as_bytes()))
        }
        Input::Inline { url, path: None } => Some(hash(url.as_bytes())),
        Input::Unavailable => None,
    }
}

fn metadata(input: Input<'_>, cwd: Option<&Path>) -> (String, usize) {
    match input {
        Input::Inline { url, .. } => records::data_url(url)
            .map_or(("unavailable".into(), 0), |(media, data)| {
                (media.into(), super::transcript_images::decoded_len(data))
            }),
        Input::Local(path) => {
            let length = local_path(path, cwd)
                .and_then(|p| std::fs::metadata(p).ok())
                .map_or(0, |m| m.len().min(MAX_IMAGE_BYTES) as usize);
            ("image".into(), length)
        }
        Input::Unavailable => ("unavailable".into(), 0),
    }
}

/// Read only this image's source record. A fallback API representation can keep
/// an attachment accessible after its original local file has been removed.
pub fn load(primary: &Location, fallback: Option<&Location>) -> Result<Vec<u8>, String> {
    load_location(primary).or_else(|error| fallback.map_or(Err(error), load_location))
}

fn load_location(location: &Location) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(&location.rollout)
        .map_err(|e| format!("{}: {e}", location.rollout.display()))?;
    file.seek(SeekFrom::Start(location.offset))
        .map_err(|e| format!("seek: {e}"))?;
    let mut buf = Vec::new();
    if super::read_line_capped(
        &mut BufReader::new(file),
        &mut buf,
        super::MAX_TRANSCRIPT_LINE_BYTES,
    )
    .map_err(|e| format!("read: {e}"))?
        != Some(true)
        || hash(&buf) != location.record_hash
    {
        return Err("transcript changed under us (image record is gone)".into());
    }
    let value = serde_json::from_slice(&buf).map_err(|e| format!("parse: {e}"))?;
    let record = records::decode(&value).ok_or("image record is gone")?;
    match record
        .images
        .get(location.block)
        .ok_or("image block is gone")?
    {
        Input::Inline { url, .. } => {
            let (_, data) =
                records::data_url(url).ok_or("image is not a supported inline data URL")?;
            super::transcript_images::decode_base64(data)
        }
        Input::Local(path) => read_local(
            &local_path(path, location.cwd.as_deref())
                .ok_or("relative image path has no recorded cwd")?,
        ),
        Input::Unavailable => Err("image bytes are unavailable in this rollout".into()),
    }
}

fn local_path(path: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        cwd.map(|cwd| cwd.join(path))
    }
}

fn read_local(path: &Path) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{}: image is not a regular file within the 64 MiB limit",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("image exceeds the 64 MiB limit".into());
    }
    Ok(bytes)
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}
