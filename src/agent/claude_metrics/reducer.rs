use std::collections::{BTreeMap, VecDeque};

use jiff::Timestamp;

use super::records::{Event, TaskOutcome, ToolCall};
use super::{Compaction, Mode, Plan, Sample, Snapshot, Tool, ToolCount, ToolStatus, Usage};

pub const MAX_TOOLS: usize = 64;
/// Tools counted by name; later names share [`OTHER_TOOLS`].
pub const MAX_TOOL_NAMES: usize = 24;
pub const OTHER_TOOLS: &str = "other";
/// How many message ids are remembered to deduplicate their records. A
/// message's records are written together, so a short window is enough.
const RECENT_MESSAGES: usize = 64;

/// Folds transcript events in file order. A message's usage counts once
/// however many records repeat it; a compaction ends what is known about the
/// context but not the totals.
#[derive(Default)]
pub struct Reducer {
    snapshot: Snapshot,
    recent: VecDeque<(String, Usage)>,
    tools: BTreeMap<String, Tool>,
    tools_incomplete: bool,
    mode: Mode,
    mode_at: Option<Timestamp>,
    /// The latest timestamp folded, which dates the untimed records.
    clock: Option<Timestamp>,
    compacted_at: Option<Timestamp>,
    /// Accumulated from `TaskCreate`/`TaskUpdate`, independently of whatever
    /// `TodoWrite`'s own full-list reports hold — published to
    /// `snapshot.plan` either way, latest timestamp winning.
    task_plan: Plan,
}

impl Reducer {
    pub fn snapshot(&self) -> Snapshot {
        let mut result = self.snapshot.clone();
        result.tools = self.tools.values().cloned().collect();
        result.tools.sort_by_key(|tool| tool.at);
        result.tools_complete = !self.tools_incomplete;
        result
            .tool_counts
            .sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name)));
        result.mode = (self.mode != Mode::default())
            .then(|| self.mode_at.or(self.clock))
            .flatten()
            .map(|at| Sample {
                at,
                value: self.mode.clone(),
            });
        result
    }

    pub fn fold(&mut self, event: Event) {
        match event {
            Event::Assistant {
                at,
                message_id,
                model,
                effort,
                usage,
                calls,
            } => {
                self.tick(at);
                if model.is_some() || effort.is_some() {
                    if model.is_some() {
                        self.mode.model = model;
                    }
                    if effort.is_some() {
                        self.mode.effort = effort;
                    }
                    self.mode_at = Some(at);
                }
                if let Some(usage) = usage {
                    self.fold_usage(message_id, usage, at);
                }
                for call in calls {
                    self.fold_call(call, at);
                }
            }
            Event::Results { at, results, task } => {
                self.tick(at);
                for (id, failed) in results {
                    self.fold_result(&id, failed, at);
                }
                if let Some(task) = task {
                    self.fold_task(task, at);
                }
            }
            Event::PermissionMode(mode) => {
                self.mode.permission_mode = Some(mode);
                self.mode_at = self.clock;
            }
            Event::Compaction { at, compaction } => self.fold_compaction(at, compaction),
        }
    }

    fn tick(&mut self, at: Timestamp) {
        self.clock = Some(self.clock.map_or(at, |clock| clock.max(at)));
    }

    fn fold_usage(&mut self, id: String, usage: Usage, at: Timestamp) {
        let snapshot = &mut self.snapshot;
        if let Some((_, seen)) = self.recent.iter_mut().find(|(seen, _)| *seen == id) {
            // A repeat replaces what its message counted; it never adds to it.
            snapshot.totals = snapshot.totals.saturating_sub(*seen).saturating_add(usage);
            *seen = usage;
        } else {
            snapshot.totals = snapshot.totals.saturating_add(usage);
            snapshot.messages = snapshot.messages.saturating_add(1);
            self.recent.push_back((id, usage));
            if self.recent.len() > RECENT_MESSAGES {
                self.recent.pop_front();
            }
        }
        replace(&mut snapshot.request, at, usage);
        if self.compacted_at.is_none_or(|cut| at > cut) {
            replace(&mut snapshot.context_tokens, at, usage.context());
        }
    }

    fn fold_call(&mut self, call: ToolCall, at: Timestamp) {
        if let Some(plan) = call.plan {
            replace(&mut self.snapshot.plan, at, plan);
        }
        if self.tools.contains_key(&call.id) {
            return;
        }
        if call.background {
            self.snapshot.background_calls = self.snapshot.background_calls.saturating_add(1);
        }
        if matches!(call.name.as_deref(), Some("Agent" | "Task")) {
            self.snapshot.subagent_calls = self.snapshot.subagent_calls.saturating_add(1);
        }
        let count = self.count(call.name.as_deref());
        count.calls = count.calls.saturating_add(1);
        self.tools.insert(
            call.id,
            Tool {
                name: call.name,
                status: ToolStatus::Started,
                at,
                background: call.background,
            },
        );
        if self.tools.len() > MAX_TOOLS {
            // The oldest observation is evicted, not an arbitrary map entry.
            if let Some(id) = self
                .tools
                .iter()
                .min_by_key(|(_, tool)| tool.at)
                .map(|(id, _)| id.clone())
            {
                self.tools.remove(&id);
            }
            self.tools_incomplete = true;
        }
    }

    fn fold_result(&mut self, id: &str, failed: bool, at: Timestamp) {
        let Some(tool) = self.tools.get_mut(id) else {
            // Started before the read, or already evicted: it can't be named.
            self.tools_incomplete = true;
            return;
        };
        tool.at = tool.at.max(at);
        if tool.status == ToolStatus::Failed {
            return;
        }
        tool.status = if failed {
            ToolStatus::Failed
        } else {
            ToolStatus::Completed
        };
        if failed {
            let name = tool.name.clone();
            let count = self.count(name.as_deref());
            count.failed = count.failed.saturating_add(1);
        }
    }

    /// The tally for `name`. Past [`MAX_TOOL_NAMES`] distinct names, new ones
    /// share [`OTHER_TOOLS`], so the list stays bounded.
    fn count(&mut self, name: Option<&str>) -> &mut ToolCount {
        let counts = &mut self.snapshot.tool_counts;
        let name = name.unwrap_or("tool");
        let index = counts.iter().position(|c| c.name == name).or_else(|| {
            (counts.len() >= MAX_TOOL_NAMES)
                .then(|| counts.iter().position(|c| c.name == OTHER_TOOLS))
                .flatten()
        });
        let index = index.unwrap_or_else(|| {
            let name = if counts.len() >= MAX_TOOL_NAMES {
                OTHER_TOOLS
            } else {
                name
            };
            counts.push(ToolCount {
                name: name.to_owned(),
                calls: 0,
                failed: 0,
            });
            counts.len() - 1
        });
        &mut counts[index]
    }

    /// `TaskCreate` starts a task `pending`; `TaskUpdate`'s own `from`/`to`
    /// moves it, so no per-task state is kept here to derive the delta.
    fn fold_task(&mut self, task: TaskOutcome, at: Timestamp) {
        match task {
            TaskOutcome::Created => {
                self.task_plan.pending = self.task_plan.pending.saturating_add(1);
            }
            TaskOutcome::StatusChanged { from, to } => {
                if let Some(slot) = Self::plan_slot(&mut self.task_plan, &from) {
                    *slot = slot.saturating_sub(1);
                }
                if let Some(slot) = Self::plan_slot(&mut self.task_plan, &to) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
        replace(&mut self.snapshot.plan, at, self.task_plan);
    }

    /// Which count `status` names, for an enum closed by `TaskList`'s own
    /// description; an unrecognized value is dropped rather than guessed.
    fn plan_slot<'a>(plan: &'a mut Plan, status: &str) -> Option<&'a mut u32> {
        match status {
            "pending" => Some(&mut plan.pending),
            "in_progress" => Some(&mut plan.in_progress),
            "completed" => Some(&mut plan.completed),
            _ => None,
        }
    }

    fn fold_compaction(&mut self, at: Timestamp, compaction: Compaction) {
        self.tick(at);
        self.snapshot.compactions = self.snapshot.compactions.saturating_add(1);
        if self.compacted_at.is_none_or(|old| at >= old) {
            self.compacted_at = Some(at);
            if self
                .snapshot
                .context_tokens
                .as_ref()
                .is_some_and(|sample| sample.at <= at)
            {
                self.snapshot.context_tokens = None;
            }
            replace(&mut self.snapshot.compaction, at, compaction);
        }
    }
}

fn replace<T>(slot: &mut Option<Sample<T>>, at: Timestamp, value: T) {
    if slot.as_ref().is_none_or(|old| at >= old.at) {
        *slot = Some(Sample { at, value });
    }
}
