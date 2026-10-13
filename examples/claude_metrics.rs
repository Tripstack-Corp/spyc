//! Read-only acceptance driver for the Claude metrics reducer. It folds one
//! explicitly selected transcript once, at most its last 16 MiB, and prints
//! metric summaries; it never launches Claude Code or searches `~/.claude`.

use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, bail};
use spyc::claude_metrics::{Reducer, Sample, records};

const MAX_TAIL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 256 * 1024;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || uuid::Uuid::parse_str(&args[0]).is_err() {
        bail!("usage: claude_metrics SESSION_ID EXACT_TRANSCRIPT");
    }
    let mut file = std::fs::File::open(&args[1]).context("opening the transcript")?;
    let size = file.metadata()?.len();
    let offset = size.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    file.take(MAX_TAIL_BYTES).read_to_end(&mut data)?;

    let mut lines: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
    lines.pop(); // never fold an unfinished last line
    let skip_partial_first = usize::from(offset > 0);
    let mut reducer = Reducer::default();
    let mut skipped = 0_u32;
    for line in lines.into_iter().skip(skip_partial_first) {
        if line.len() > MAX_LINE_BYTES {
            skipped += 1;
            continue;
        }
        let Ok(value) = serde_json::from_slice(line) else {
            skipped += 1;
            continue;
        };
        if let Some(event) = records::decode(&value, &args[0]) {
            reducer.fold(event);
        }
    }

    let snapshot = reducer.snapshot();
    let value =
        |sample: Option<&Sample<u64>>| sample.map_or("--".to_owned(), |s| s.value.to_string());
    let mode = snapshot.mode.as_ref().map(|m| &m.value);
    println!("tail_only={} skipped_lines={skipped}", offset > 0);
    println!(
        "model={} effort={} permission_mode={}",
        mode.and_then(|m| m.model.as_deref()).unwrap_or("--"),
        mode.and_then(|m| m.effort.as_deref()).unwrap_or("--"),
        mode.and_then(|m| m.permission_mode.as_deref())
            .unwrap_or("--"),
    );
    println!(
        "messages={} totals={:?} context_tokens={} context=--%",
        snapshot.messages,
        snapshot.totals,
        value(snapshot.context_tokens.as_ref()),
    );
    println!(
        "compactions={} latest={:?}",
        snapshot.compactions,
        snapshot.compaction.as_ref().map(|c| &c.value),
    );
    println!(
        "recent_tools={} exhaustive={} subagent_calls={} background_calls={} plan={:?}",
        snapshot.tools.len(),
        snapshot.tools_complete,
        snapshot.subagent_calls,
        snapshot.background_calls,
        snapshot.plan.as_ref().map(|p| p.value),
    );
    Ok(())
}
