#!/usr/bin/env python3
"""B1 one-shot field probe; no watcher, credential lookup or RPC connection."""
import argparse
import collections
import datetime
import json
import os
import re
import stat

MAX_BYTES = 16 * 1024 * 1024
MAX_LINE = 256 * 1024
TOKEN_FIELDS = (
    "input_tokens", "cached_input_tokens", "cache_write_input_tokens",
    "output_tokens", "reasoning_output_tokens", "total_tokens",
)


def token_fields(value):
    if not isinstance(value, dict):
        return None
    out = {
        key: value[key] for key in TOKEN_FIELDS
        if type(value.get(key)) is int and 0 <= value[key] <= 2**63 - 1
    }
    return out or None


def label(value):
    if isinstance(value, str) and re.fullmatch(r"[a-zA-Z0-9_.:/ -]{1,80}", value):
        return value
    return None


def timestamp(value):
    if not isinstance(value, str) or len(value) > 40:
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed.isoformat() if parsed.tzinfo else None


def metric_record(record, thread):
    payload = record.get("payload")
    if not isinstance(payload, dict):
        return None
    kind = record.get("type")
    sample = {"timestamp": timestamp(record.get("timestamp"))}
    if kind == "token_usage_record":
        if payload.get("thread_id") != thread or payload.get("session_id") != thread:
            return None
        sample.update({"kind": kind, "request": token_fields(payload.get("usage")),
                       "turn": token_fields(payload.get("turn_token_usage")),
                       "thread": token_fields(payload.get("thread_token_usage"))})
    elif kind == "turn_context":
        policy = payload.get("sandbox_policy")
        sample.update({"kind": kind, "model": label(payload.get("model")),
                       "effort": label(payload.get("effort")),
                       "approval": label(payload.get("approval_policy")),
                       "sandbox": label(policy.get("type")) if isinstance(policy, dict) else None})
    elif kind == "event_msg" and payload.get("type") == "token_count":
        info = payload.get("info")
        info = info if isinstance(info, dict) else {}
        window = info.get("model_context_window")
        rate = payload.get("rate_limits")
        rate = rate if isinstance(rate, dict) else {}
        sample.update({"kind": "token_count", "thread": token_fields(info.get("total_token_usage")),
                       "last": token_fields(info.get("last_token_usage")),
                       "model_context_window": window if type(window) is int and 0 < window < 2**63 else None,
                       "limit_id": label(rate.get("limit_id")),
                       "primary_reported": isinstance(rate.get("primary"), dict),
                       "secondary_reported": isinstance(rate.get("secondary"), dict)})
    elif kind == "compacted":
        number = payload.get("window_number")
        sample.update({"kind": "compacted", "window_number": number if type(number) is int and number >= 0 else None})
    else:
        return None
    return sample


def inspect(path, thread):
    counts = collections.Counter()
    latest = {}
    recent = collections.deque(maxlen=32)
    skipped = collections.Counter()
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError("expected a regular rollout file")
        size = metadata.st_size
        first = source.readline(MAX_LINE + 1)
        if len(first) > MAX_LINE or not first.endswith(b"\n"):
            raise ValueError("missing or oversized session header")
        header = json.loads(first)
        if header.get("type") != "session_meta" or header.get("payload", {}).get("id") != thread:
            raise ValueError("rollout does not match the explicitly selected thread")
        cli_version = label(header["payload"].get("cli_version"))
        offset = max(0, size - MAX_BYTES)
        source.seek(offset)
        data = source.read(min(size, MAX_BYTES))
    lines = data.split(b"\n")
    if offset:
        lines = lines[1:]
    lines = lines[:-1]  # never parse an unfinished last line
    for raw in lines:
        if len(raw) > MAX_LINE:
            skipped["oversized"] += 1
            continue
        try:
            record = json.loads(raw)
        except (ValueError, UnicodeError):
            skipped["malformed"] += 1
            continue
        if not isinstance(record, dict):
            skipped["unknown"] += 1
            continue
        payload = record.get("payload")
        if isinstance(payload, dict) and record.get("type") == "event_msg":
            subtype = payload.get("type")
            if subtype in {"item_started", "item_completed", "task_started", "task_complete"}:
                counts[subtype] += 1
        sample = metric_record(record, thread)
        if sample is not None:
            counts[sample["kind"]] += 1
            latest[sample["kind"]] = sample
            recent.append(sample)
    return {"probe_version": 1, "cli_version": cli_version,
            "coverage": {"file_bytes": size, "bytes_read": len(data), "tail_only": bool(offset),
                         "line_cap_bytes": MAX_LINE, "skipped": dict(skipped)},
            "counts_in_read_window": dict(counts), "latest": latest, "recent_metric_records": list(recent)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rollout", help="one exact rollout; never searches a home or sessions tree")
    parser.add_argument("--thread", required=True, help="expected session_meta id; omitted from output")
    args = parser.parse_args()
    try:
        print(json.dumps(inspect(args.rollout, args.thread), indent=2))
    except (OSError, ValueError, AttributeError, TypeError) as error:
        parser.exit(1, f"probe refused the rollout: {type(error).__name__}\n")
