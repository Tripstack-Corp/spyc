//! Folds an agy transcript's events, in file order, into the HUD's [`Snapshot`]:
//! usage, mode, compactions and tool calls, each result paired with its call and
//! a result that arrives first held until its call does.

use std::collections::{BTreeMap, VecDeque};

use jiff::Timestamp;

use super::records::{Event, ToolCall};
use super::{Compaction, Mode, Sample, Snapshot, Tool, ToolStatus, Usage};
use crate::agent::metrics::{MAX_TOOLS, count_tool, replace};

/// How many planner responses are remembered to deduplicate their records.
const RECENT_MESSAGES: usize = 64;

#[derive(Clone, Debug)]
struct PendingTool {
    id: String,
    planner_step: u64,
    name: Option<String>,
}

/// agy sometimes writes a result before the planner response that made its
/// call. Such results wait for it; past this many, the oldest is dropped.
const MAX_EARLY_RESULTS: usize = 8;

#[derive(Clone, Debug)]
struct EarlyResult {
    result_step: u64,
    failed: bool,
    at: Timestamp,
    record_type: Option<String>,
}

/// Folds transcript events in file order.
#[derive(Default)]
pub struct Reducer {
    snapshot: Snapshot,
    recent: VecDeque<(u64, Usage)>,
    tools: BTreeMap<String, Tool>,
    pending_tools: VecDeque<PendingTool>,
    early_results: VecDeque<EarlyResult>,
    tools_incomplete: bool,
    mode: Mode,
    mode_at: Option<Timestamp>,
    clock: Option<Timestamp>,
    compacted_at: Option<Timestamp>,
    compacted_step: Option<u64>,
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
            Event::PlannerResponse {
                at,
                step_index,
                usage,
                calls,
            } => {
                self.tick(at);
                let Some(step) = step_index else {
                    self.tools_incomplete = true;
                    return;
                };
                self.close_older_pending_tools(step, at);
                if let Some(usage) = usage {
                    self.fold_usage(step, usage, at);
                }
                for (i, call) in calls.into_iter().enumerate() {
                    let id = format!("{step}:{i}");
                    self.fold_call(id, step, call, at);
                }
                self.apply_early_results(step);
            }
            Event::ToolResult {
                at,
                step_index,
                failed,
                record_type,
            } => {
                self.tick(at);
                let Some(step) = step_index else {
                    self.tools_incomplete = true;
                    return;
                };
                self.fold_result(step, failed, at, record_type.as_deref());
            }
            Event::Checkpoint { at, step_index } => self.fold_compaction(at, step_index),
            Event::Model { at, model } => {
                if let Some(at) = at {
                    self.tick(at);
                    self.mode_at = Some(at);
                } else {
                    self.mode_at = self.clock;
                }
                self.mode.model = Some(model);
            }
        }
    }

    fn tick(&mut self, at: Timestamp) {
        self.clock = Some(self.clock.map_or(at, |clock| clock.max(at)));
    }

    fn fold_usage(&mut self, step: u64, usage: Usage, at: Timestamp) {
        let restore =
            Self::should_restore_context(self.compacted_at, self.compacted_step, at, step);
        if let Some((_, seen)) = self.recent.iter_mut().find(|(s, _)| *s == step) {
            self.snapshot.totals = self
                .snapshot
                .totals
                .saturating_sub(*seen)
                .saturating_add(usage);
            *seen = usage;
        } else {
            self.snapshot.totals = self.snapshot.totals.saturating_add(usage);
            self.snapshot.messages = self.snapshot.messages.saturating_add(1);
            self.recent.push_back((step, usage));
            if self.recent.len() > RECENT_MESSAGES {
                self.recent.pop_front();
            }
        }
        replace(&mut self.snapshot.request, at, usage);
        if restore {
            replace(&mut self.snapshot.context_tokens, at, usage.context());
            self.compacted_at = None;
            self.compacted_step = None;
        }
    }

    fn should_restore_context(
        compacted_at: Option<Timestamp>,
        compacted_step: Option<u64>,
        at: Timestamp,
        step: u64,
    ) -> bool {
        let Some(cut_at) = compacted_at else {
            return true;
        };
        if at > cut_at {
            return true;
        }
        if at == cut_at {
            if let Some(cut_step) = compacted_step {
                return step > cut_step;
            }
            return true;
        }
        false
    }

    fn fold_call(&mut self, id: String, planner_step: u64, call: ToolCall, at: Timestamp) {
        if self.tools.contains_key(&id) {
            return;
        }
        if matches!(call.name.as_deref(), Some("invoke_subagent")) {
            self.snapshot.subagent_calls = self.snapshot.subagent_calls.saturating_add(1);
        }
        let count = count_tool(&mut self.snapshot.tool_counts, call.name.as_deref());
        count.calls = count.calls.saturating_add(1);
        self.tools.insert(
            id.clone(),
            Tool {
                name: call.name.clone(),
                status: ToolStatus::Started,
                at,
            },
        );
        self.pending_tools.push_back(PendingTool {
            id,
            planner_step,
            name: call.name,
        });
        self.bound_tools();
    }

    fn bound_tools(&mut self) {
        while self.tools.len() > MAX_TOOLS {
            if let Some(oldest) = self
                .tools
                .iter()
                .min_by_key(|(_, tool)| tool.at)
                .map(|(id, _)| id.clone())
            {
                self.tools.remove(&oldest);
                if let Some(pos) = self.pending_tools.iter().position(|p| p.id == oldest) {
                    self.pending_tools.remove(pos);
                }
            }
            self.tools_incomplete = true;
        }
        while self.pending_tools.len() > MAX_TOOLS {
            self.pending_tools.pop_front();
            self.tools_incomplete = true;
        }
    }

    /// agy plans only after its calls return, so a newer planner response
    /// settles every earlier call, including those that never get a result.
    fn close_older_pending_tools(&mut self, current_step: u64, at: Timestamp) {
        let mut remaining = VecDeque::new();
        while let Some(pending) = self.pending_tools.pop_front() {
            if pending.planner_step < current_step {
                if let Some(tool) = self.tools.get_mut(&pending.id)
                    && tool.status == ToolStatus::Started
                {
                    tool.status = ToolStatus::Completed;
                    tool.at = tool.at.max(at);
                }
            } else {
                remaining.push_back(pending);
            }
        }
        self.pending_tools = remaining;
    }

    fn fold_result(
        &mut self,
        result_step: u64,
        failed: bool,
        at: Timestamp,
        record_type: Option<&str>,
    ) {
        let index = find_pending_tool(&self.pending_tools, result_step, record_type);
        let Some(idx) = index else {
            if self.early_results.len() >= MAX_EARLY_RESULTS {
                self.early_results.pop_front();
                self.tools_incomplete = true;
            }
            self.early_results.push_back(EarlyResult {
                result_step,
                failed,
                at,
                record_type: record_type.map(str::to_owned),
            });
            return;
        };
        let pending = self.pending_tools.remove(idx).expect("index within bounds");
        self.apply_result(&pending.id, failed, at);
    }

    fn apply_result(&mut self, tool_id: &str, failed: bool, at: Timestamp) {
        let Some(tool) = self.tools.get_mut(tool_id) else {
            self.tools_incomplete = true;
            return;
        };
        tool.at = tool.at.max(at);
        tool.status = if failed {
            ToolStatus::Failed
        } else {
            ToolStatus::Completed
        };
        if failed {
            let name = tool.name.clone();
            let count = count_tool(&mut self.snapshot.tool_counts, name.as_deref());
            count.failed = count.failed.saturating_add(1);
        }
    }

    /// Pairs held results with the calls `planner_step` just made. One older
    /// than that step has no call left to answer.
    fn apply_early_results(&mut self, planner_step: u64) {
        let mut unconsumed = VecDeque::new();
        while let Some(early) = self.early_results.pop_front() {
            if early.result_step < planner_step {
                self.tools_incomplete = true;
                continue;
            }
            if let Some(idx) = find_pending_tool(
                &self.pending_tools,
                early.result_step,
                early.record_type.as_deref(),
            ) {
                let pending = self.pending_tools.remove(idx).expect("index within bounds");
                self.apply_result(&pending.id, early.failed, early.at);
            } else {
                unconsumed.push_back(early);
            }
        }
        self.early_results = unconsumed;
    }

    fn fold_compaction(&mut self, at: Timestamp, step_index: Option<u64>) {
        self.tick(at);
        self.snapshot.compactions = self.snapshot.compactions.saturating_add(1);
        self.compacted_at = Some(at);
        self.compacted_step = step_index;
        if self
            .snapshot
            .context_tokens
            .as_ref()
            .is_some_and(|sample| sample.at <= at)
        {
            self.snapshot.context_tokens = None;
        }
        replace(&mut self.snapshot.compaction, at, Compaction);
    }
}

/// The waiting call a result at `result_step` answers: one from the latest
/// planner response at or before it, preferring a call of the result's kind.
fn find_pending_tool(
    pending: &VecDeque<PendingTool>,
    result_step: u64,
    record_type: Option<&str>,
) -> Option<usize> {
    if let Some(rt) = record_type {
        let max_step = pending
            .iter()
            .filter(|p| {
                p.planner_step <= result_step
                    && p.name.as_deref().is_some_and(|n| type_matches_tool(rt, n))
            })
            .map(|p| p.planner_step)
            .max();
        if let Some(ms) = max_step {
            return pending.iter().position(|p| {
                p.planner_step == ms && p.name.as_deref().is_some_and(|n| type_matches_tool(rt, n))
            });
        }
    }
    let max_planner = pending
        .iter()
        .filter(|p| p.planner_step <= result_step)
        .map(|p| p.planner_step)
        .max();
    max_planner.and_then(|max_step| pending.iter().position(|p| p.planner_step == max_step))
}

/// Which tool calls produce each of agy's typed result records. An untyped
/// `GENERIC` result is matched by step alone.
fn type_matches_tool(typ: &str, tool: &str) -> bool {
    match typ {
        "VIEW_FILE" => tool == "view_file" || tool == "read_file",
        "RUN_COMMAND" => tool == "run_command" || tool == "manage_task",
        "GREP_SEARCH" => tool == "grep_search",
        "SEARCH_WEB" => tool == "search_web",
        "READ_URL_CONTENT" => tool == "read_url_content",
        "LIST_DIRECTORY" => tool == "list_dir" || tool == "list_directory",
        "READ_RESOURCE" => tool == "read_resource",
        "LIST_RESOURCES" => tool == "list_resources",
        "ASK_QUESTION" => tool == "ask_question",
        "INVOKE_SUBAGENT" => {
            tool == "invoke_subagent"
                || tool == "define_subagent"
                || tool == "send_message"
                || tool == "manage_subagents"
        }
        "CODE_ACTION" => {
            tool == "replace_file_content"
                || tool == "write_to_file"
                || tool == "multi_replace_file_content"
                || tool == "edit_file"
                || tool == "write_file"
        }
        _ => false,
    }
}
