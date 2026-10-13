"""Privacy, identity and bounded-read checks for the one-shot Claude probe."""
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import probe

SESSION = "session-a"


def assistant(message_id="msg-1", block=0, usage=None, content=None, session=SESSION):
    return {"type": "assistant", "sessionId": session, "version": "9.9.9", "apiBlockIndex": block,
            "requestedModel": "claude-test", "effort": "high",
            "message": {"id": message_id, "model": "claude-test", "stop_reason": "tool_use",
                        "usage": usage or {"input_tokens": 2, "cache_creation_input_tokens": 30,
                                           "cache_read_input_tokens": 400, "output_tokens": 5},
                        "content": content or []}}


class ProbeTests(unittest.TestCase):
    def capture(self, records, session=SESSION, suffix=b""):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "transcript.jsonl"
            data = b"\n".join(json.dumps(row).encode() for row in records) + b"\n" + suffix
            path.write_bytes(data)
            return probe.inspect(path, session)

    def test_private_content_never_leaves_the_probe(self):
        records = [
            {"type": "user", "sessionId": SESSION, "cwd": "/PRIVATE_PATH_SENTINEL",
             "message": {"role": "user", "content": "PRIVATE_PROMPT_SENTINEL"},
             "toolUseResult": {"stdout": "PRIVATE_OUTPUT_SENTINEL", "backgroundTaskId": "PRIVATE_ID_SENTINEL"}},
            assistant(content=[
                {"type": "thinking", "thinking": "PRIVATE_REASONING_SENTINEL"},
                {"type": "text", "text": "PRIVATE_RESPONSE_SENTINEL"},
                {"type": "tool_use", "name": "Bash", "input": {"command": "PRIVATE_ARGUMENT_SENTINEL",
                                                                "run_in_background": True}},
                {"type": "tool_use", "name": "mcp__PRIVATE_SERVER_SENTINEL__tool", "input": {}},
                {"type": "tool_use", "name": "TodoWrite", "input": {"todos": [
                    {"content": "PRIVATE_TODO_SENTINEL", "status": "completed"},
                    {"content": "PRIVATE_TODO_SENTINEL", "status": "in_progress"}]}},
                {"type": "tool_use", "name": "Agent", "input": {"prompt": "PRIVATE_TASK_SENTINEL",
                                                                 "subagent_type": "general-purpose"}},
            ]),
            {"type": "system", "subtype": "compact_boundary", "sessionId": SESSION,
             "content": "PRIVATE_SUMMARY_SENTINEL", "compactMetadata": {"trigger": "auto", "preTokens": 190000}},
            {"type": "attachment", "sessionId": SESSION,
             "attachment": {"type": "edited_text_file", "filename": "/PRIVATE_PATH_SENTINEL"}},
        ]
        output = self.capture(records)
        text = json.dumps(output)
        self.assertNotIn("SENTINEL", text)
        self.assertNotIn(SESSION, text)
        self.assertNotIn("msg-1", text)
        self.assertEqual(output["background_tool_calls"], {"Bash": 1})
        self.assertEqual(output["latest_todos"], {"pending": 0, "in_progress": 1, "completed": 1})
        self.assertEqual(output["subagent_types"], {"general-purpose": 1})
        self.assertEqual(output["tool_result_task_keys"], {"backgroundTaskId": 1})
        self.assertEqual(output["compaction"]["latest_boundary"]["pre_tokens"], 190000)

    def test_a_message_written_once_per_block_counts_once(self):
        output = self.capture([assistant(block=0), assistant(block=1), assistant(block=2),
                               assistant(message_id="msg-2")])
        usage = output["usage"]
        self.assertEqual((usage["assistant_records"], usage["distinct_messages"]), (4, 2))
        self.assertEqual(usage["messages_written_more_than_once"], 1)
        self.assertEqual(usage["repeats_with_differing_usage"], 0)
        self.assertEqual(usage["distinct_message_totals"]["output_tokens"], 10)
        self.assertEqual(usage["latest_context_tokens"], 432)

    def test_another_session_is_refused_and_cannot_contribute(self):
        with self.assertRaises(ValueError):
            self.capture([assistant()], session="session-b")
        output = self.capture([assistant(), assistant(message_id="foreign", session="session-b")])
        self.assertEqual(output["coverage"]["skipped"], {"other_session": 1})
        self.assertEqual(output["usage"]["distinct_messages"], 1)

    def test_an_unfinished_last_line_is_not_a_measurement(self):
        late = assistant(message_id="msg-2", usage={"output_tokens": 999})
        output = self.capture([assistant()], suffix=json.dumps(late).encode())
        self.assertEqual(output["usage"]["latest_message"]["output_tokens"], 5)

    def test_oversized_and_malformed_records_do_not_hide_later_metrics(self):
        oversized = {"type": "user", "sessionId": SESSION, "message": {"content": "x" * (probe.MAX_LINE + 1)}}
        output = self.capture([oversized, assistant()], suffix=b"not json\n")
        self.assertEqual(output["coverage"]["skipped"], {"oversized": 1, "malformed": 1})
        self.assertEqual(output["usage"]["distinct_messages"], 1)

    def test_a_large_file_reads_only_the_declared_tail_budget(self):
        ignored = {"type": "user", "sessionId": SESSION, "message": {"content": "x" * 2000}}
        with mock.patch.object(probe, "MAX_BYTES", 1024):
            output = self.capture([ignored, ignored, assistant()])
        self.assertTrue(output["coverage"]["tail_only"])
        self.assertLessEqual(output["coverage"]["bytes_read"], 1024)
        self.assertEqual(output["usage"]["distinct_messages"], 1)

    def test_absence_and_invalid_values_do_not_become_zero(self):
        output = self.capture([assistant(usage={"input_tokens": -1, "output_tokens": True,
                                                "cache_read_input_tokens": 0})])
        self.assertEqual(output["usage"]["latest_message"], {"cache_read_input_tokens": 0})
        self.assertIsNone(output["latest_todos"])
        self.assertEqual(probe.usage_fields(None), None)


if __name__ == "__main__":
    unittest.main()
