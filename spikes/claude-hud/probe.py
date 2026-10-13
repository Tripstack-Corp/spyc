#!/usr/bin/env python3
"""One-shot field probe of one Claude Code transcript; no watcher, discovery or hooks.

Reads at most a bounded tail of the one file it is given and prints only counts,
token numbers and short labels. Prompts, responses, reasoning, tool arguments and
results, paths and ids never leave it.
"""
import argparse
import collections
import datetime
import json
import os
import re
import stat

MAX_BYTES = 16 * 1024 * 1024
MAX_LINE = 256 * 1024
USAGE_FIELDS = (
    "input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens",
)
TODO_STATES = ("pending", "in_progress", "completed")


def label(value):
    if isinstance(value, str) and re.fullmatch(r"[a-zA-Z0-9_.:/ -]{1,64}", value):
        return value
    return None


def count(value):
    return value if type(value) is int and 0 <= value <= 2**63 - 1 else None


def timestamp(value):
    if not isinstance(value, str) or len(value) > 40:
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed.isoformat() if parsed.tzinfo else None


def usage_fields(value):
    if not isinstance(value, dict):
        return None
    out = {key: count(value.get(key)) for key in USAGE_FIELDS if count(value.get(key)) is not None}
    return out or None


def tool_label(name):
    """MCP tools collapse to `mcp`, or `mcp:spyc` for spyc's own server, so the
    output names no other server or tool."""
    name = label(name)
    if name and name.startswith("mcp__"):
        return "mcp:spyc" if name.startswith("mcp__spyc__") else "mcp"
    return name or "unlabelled"


class Observation:
    def __init__(self):
        self.records = collections.Counter()
        self.attachments = collections.Counter()
        self.system = collections.Counter()
        self.skipped = collections.Counter()
        self.latest = {}
        self.models = collections.Counter()
        self.tools = collections.Counter()
        self.background = collections.Counter()
        self.subagent_types = collections.Counter()
        self.stop_reasons = collections.Counter()
        self.result_keys = collections.Counter()
        self.messages = {}
        self.blocks = 0
        self.sidechain = 0
        self.compact_summaries = 0
        self.context_management = 0
        self.first = None
        self.last = None

    def assistant(self, record):
        message = record.get("message")
        if not isinstance(message, dict):
            return
        self.models[label(message.get("model")) or "unlabelled"] += 1
        self.stop_reasons[label(message.get("stop_reason")) or "none"] += 1
        if message.get("context_management") is not None:
            self.context_management += 1
        usage = usage_fields(message.get("usage"))
        identity = message.get("id")
        if isinstance(identity, str) and usage:
            self.blocks += 1
            seen = self.messages.setdefault(identity, [])
            seen.append(usage)
            self.latest["usage"] = usage
        for key, field in (("model", message.get("model")), ("requested_model", record.get("requestedModel")),
                           ("effort", record.get("effort"))):
            if label(field):
                self.latest[key] = label(field)
        for block in message.get("content") or []:
            if not isinstance(block, dict) or block.get("type") != "tool_use":
                continue
            name = tool_label(block.get("name"))
            self.tools[name] += 1
            tool_input = block.get("input") if isinstance(block.get("input"), dict) else {}
            if tool_input.get("run_in_background") is True:
                self.background[name] += 1
            if name in ("Agent", "Task"):
                self.subagent_types[label(tool_input.get("subagent_type")) or "unlabelled"] += 1
            if name == "TodoWrite" and isinstance(tool_input.get("todos"), list):
                states = collections.Counter(
                    todo.get("status") for todo in tool_input["todos"] if isinstance(todo, dict))
                self.latest["todos"] = {state: states[state] for state in TODO_STATES}

    def observe(self, record):
        kind = label(record.get("type")) or "unlabelled"
        self.records[kind] += 1
        when = timestamp(record.get("timestamp"))
        if when:
            self.first = self.first or when
            self.last = when
        if label(record.get("version")):
            self.latest["claude_code_version"] = label(record.get("version"))
        if record.get("isSidechain") is True:
            self.sidechain += 1
        if kind == "assistant":
            self.assistant(record)
        elif kind == "user":
            if record.get("isCompactSummary") is True:
                self.compact_summaries += 1
            result = record.get("toolUseResult")
            if isinstance(result, dict):
                for key in result:
                    if label(key) and ("ackground" in key or "task" in key.lower()):
                        self.result_keys[key] += 1
        elif kind == "permission-mode" and label(record.get("permissionMode")):
            self.latest["permission_mode"] = label(record["permissionMode"])
        elif kind == "mode" and label(record.get("mode")):
            self.latest["mode"] = label(record["mode"])
        elif kind == "attachment" and isinstance(record.get("attachment"), dict):
            self.attachments[label(record["attachment"].get("type")) or "unlabelled"] += 1
        elif kind == "system":
            subtype = label(record.get("subtype")) or "unlabelled"
            self.system[subtype] += 1
            metadata = record.get("compactMetadata")
            if subtype == "compact_boundary" and isinstance(metadata, dict):
                self.latest["compaction"] = {"trigger": label(metadata.get("trigger")),
                                             "pre_tokens": count(metadata.get("preTokens")),
                                             "timestamp": when}

    def result(self):
        repeated = [usages for usages in self.messages.values() if len(usages) > 1]
        totals = collections.Counter()
        for usages in self.messages.values():
            totals.update(usages[-1])
        latest = self.latest.get("usage")
        context = None
        if latest:
            context = sum(latest.get(key, 0) for key in USAGE_FIELDS[:3])
        return {
            "records": dict(self.records), "attachments": dict(self.attachments),
            "system_subtypes": dict(self.system), "sidechain_records": self.sidechain,
            "window": {"first": self.first, "last": self.last},
            "identity": {key: self.latest.get(key) for key in
                         ("claude_code_version", "model", "requested_model", "effort", "permission_mode", "mode")},
            "models": dict(self.models),
            "usage": {"assistant_records": self.blocks, "distinct_messages": len(self.messages),
                      "messages_written_more_than_once": len(repeated),
                      "repeats_with_differing_usage": sum(
                          1 for usages in repeated if any(u != usages[0] for u in usages)),
                      "latest_message": latest,
                      "latest_context_tokens": context,
                      "distinct_message_totals": dict(totals) or None},
            "stop_reasons": dict(self.stop_reasons),
            "context_management_records": self.context_management,
            "tools": dict(self.tools), "background_tool_calls": dict(self.background),
            "tool_result_task_keys": dict(self.result_keys),
            "subagent_types": dict(self.subagent_types),
            "latest_todos": self.latest.get("todos"),
            "compaction": {"summaries": self.compact_summaries, "latest_boundary": self.latest.get("compaction")},
        }


def inspect(path, session):
    observation = Observation()
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError("expected a regular transcript file")
        size = metadata.st_size
        offset = max(0, size - MAX_BYTES)
        source.seek(offset)
        data = source.read(min(size, MAX_BYTES))
    lines = data.split(b"\n")
    if offset:
        lines = lines[1:]
    lines = lines[:-1]  # never parse an unfinished last line
    matched = 0
    for raw in lines:
        if len(raw) > MAX_LINE:
            observation.skipped["oversized"] += 1
            continue
        try:
            record = json.loads(raw)
        except (ValueError, UnicodeError):
            observation.skipped["malformed"] += 1
            continue
        if not isinstance(record, dict):
            observation.skipped["unknown"] += 1
            continue
        owner = record.get("sessionId")
        if owner is not None and owner != session:
            observation.skipped["other_session"] += 1
            continue
        matched += owner == session
        observation.observe(record)
    if not matched:
        raise ValueError("transcript does not belong to the explicitly selected session")
    return {"probe_version": 1,
            "coverage": {"file_bytes": size, "bytes_read": len(data), "tail_only": bool(offset),
                         "line_cap_bytes": MAX_LINE, "skipped": dict(observation.skipped)},
            **observation.result()}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("transcript", help="one exact transcript; never searches ~/.claude")
    parser.add_argument("--session", required=True, help="expected sessionId; omitted from output")
    args = parser.parse_args()
    try:
        print(json.dumps(inspect(args.transcript, args.session), indent=2))
    except (OSError, ValueError, AttributeError, TypeError) as error:
        parser.exit(1, f"probe refused the transcript: {type(error).__name__}\n")
