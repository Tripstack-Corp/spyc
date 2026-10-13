use std::collections::BTreeMap;

use jiff::Timestamp;

use super::{Sample, Snapshot, Source, Tool, ToolStatus};
use crate::agent::codex_records::metrics::Event;

pub const MAX_TOOLS: usize = 64;

/// Cumulative token snapshots replace older samples; they are never summed.
#[derive(Default)]
pub struct Reducer {
    snapshot: Snapshot,
    turn: Option<(String, Timestamp)>,
    compaction_at: Option<Timestamp>,
    tools: BTreeMap<String, Tool>,
}

impl Reducer {
    pub fn snapshot(&self) -> Snapshot {
        let mut result = self.snapshot.clone();
        result.tools = self.tools.values().cloned().collect();
        result.tools.sort_by_key(|tool| tool.at);
        result
    }

    pub fn fold(&mut self, event: Event) {
        match event {
            Event::Usage {
                at,
                turn_id,
                request,
                turn,
                thread,
            } => {
                if self.accept_turn(Some(turn_id), at) {
                    replace(&mut self.snapshot.request, at, Source::UsageRecord, request);
                    replace(&mut self.snapshot.turn, at, Source::UsageRecord, turn);
                }
                replace(&mut self.snapshot.thread, at, Source::UsageRecord, thread);
            }
            Event::Legacy {
                at,
                last,
                thread,
                window,
                bucket,
                limits,
            } => {
                if self.compaction_at.is_none_or(|cutoff| at > cutoff)
                    && self.turn.as_ref().is_none_or(|(_, cutoff)| at >= *cutoff)
                {
                    replace(
                        &mut self.snapshot.legacy_last,
                        at,
                        Source::LegacyTokenCount,
                        last,
                    );
                }
                replace(
                    &mut self.snapshot.legacy_thread,
                    at,
                    Source::LegacyTokenCount,
                    thread,
                );
                replace(
                    &mut self.snapshot.model_context_window,
                    at,
                    Source::LegacyTokenCount,
                    window,
                );
                replace(
                    &mut self.snapshot.limit_bucket,
                    at,
                    Source::LegacyTokenCount,
                    bucket,
                );
                replace(
                    &mut self.snapshot.limits,
                    at,
                    Source::LegacyTokenCount,
                    limits,
                );
            }
            Event::Context { at, turn_id, mode } => {
                if self.accept_turn(turn_id, at) {
                    replace(&mut self.snapshot.mode, at, Source::TurnContext, Some(mode));
                }
            }
            Event::Compacted { at, number } => {
                if self.compaction_at.is_none_or(|old| at >= old) {
                    self.compaction_at = Some(at);
                    if self
                        .snapshot
                        .legacy_last
                        .as_ref()
                        .is_some_and(|s| s.at <= at)
                    {
                        self.snapshot.legacy_last = None;
                    }
                    replace(&mut self.snapshot.compaction, at, Source::Lifecycle, number);
                }
            }
            Event::Tool { id, tool } => self.fold_tool(id, tool),
        }
    }

    fn accept_turn(&mut self, id: Option<String>, at: Timestamp) -> bool {
        if self.turn.as_ref().is_some_and(|(_, clock)| at < *clock) {
            return false;
        }
        if let Some(id) = id {
            if self.turn.as_ref().is_some_and(|(old, _)| old != &id) {
                self.snapshot.request = None;
                self.snapshot.turn = None;
                self.snapshot.legacy_last = None;
            }
            self.turn = Some((id, at));
        }
        true
    }

    fn fold_tool(&mut self, id: String, mut tool: Tool) {
        if let Some(old) = self.tools.get_mut(&id) {
            if tool.at < old.at
                || (old.status != ToolStatus::Started && tool.status == ToolStatus::Started)
            {
                old.name = old.name.take().or(tool.name);
                return;
            }
            if old.status == ToolStatus::Failed {
                tool.status = ToolStatus::Failed;
            }
            tool.elapsed_ms = tool.elapsed_ms.or(old.elapsed_ms);
            tool.name = tool.name.or_else(|| old.name.clone());
        }
        self.tools.insert(id, tool);
        if self.tools.len() > MAX_TOOLS {
            // The oldest observation is evicted, not an arbitrary hash-order entry.
            if let Some(id) = self
                .tools
                .iter()
                .min_by_key(|(_, tool)| tool.at)
                .map(|(id, _)| id.clone())
            {
                self.tools.remove(&id);
            }
        }
    }
}

fn replace<T>(slot: &mut Option<Sample<T>>, at: Timestamp, source: Source, value: Option<T>) {
    if let Some(value) = value
        && slot.as_ref().is_none_or(|old| at >= old.at)
    {
        *slot = Some(Sample { at, source, value });
    }
}
