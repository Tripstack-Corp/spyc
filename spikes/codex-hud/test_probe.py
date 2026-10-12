"""Privacy, identity and bounded-read checks for the one-shot B1 probe."""
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import probe


class ProbeTests(unittest.TestCase):
    def capture(self, records, thread="thread-a", suffix=b""):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "rollout.jsonl"
            header = {"type": "session_meta", "payload": {"id": "thread-a", "cli_version": "captured-cli"}}
            data = b"\n".join(json.dumps(row).encode() for row in [header, *records]) + b"\n" + suffix
            path.write_bytes(data)
            return probe.inspect(path, thread)

    def usage(self, total=11, thread="thread-a"):
        return {"type": "token_usage_record", "payload": {
            "thread_id": thread, "session_id": thread,
            "usage": {"input_tokens": 10, "cached_input_tokens": 8, "output_tokens": 1, "reasoning_output_tokens": 1, "total_tokens": total},
            "turn_token_usage": {"total_tokens": 100}, "thread_token_usage": {"total_tokens": 1000},
            "arguments": "PRIVATE_ARGUMENT_SENTINEL", "output": "PRIVATE_OUTPUT_SENTINEL"}}

    def test_private_content_and_unknown_fields_never_leave_the_probe(self):
        record = self.usage()
        record["payload"]["usage"]["unknown_private"] = "PRIVATE_NESTED_SENTINEL"
        records = [record,
                   {"type": "event_msg", "payload": {"type": "user_message", "message": "PRIVATE_PROMPT_SENTINEL"}},
                   {"type": "response_item", "payload": {"type": "reasoning", "encrypted_content": "PRIVATE_REASONING_SENTINEL"}},
                   {"type": "compacted", "payload": {"window_number": 1, "replacement_history": "PRIVATE_HISTORY_SENTINEL"}}]
        output = json.dumps(self.capture(records))
        self.assertNotIn("SENTINEL", output)
        self.assertNotIn("thread-a", output)
        self.assertEqual(self.capture(records)["latest"]["token_usage_record"]["request"]["total_tokens"], 11)

    def test_wrong_header_refuses_the_selected_file(self):
        with self.assertRaises(ValueError):
            self.capture([self.usage()], thread="thread-b")

    def test_foreign_metric_cannot_populate_the_selected_thread(self):
        output = self.capture([self.usage(thread="thread-b")])
        self.assertNotIn("token_usage_record", output["latest"])

    def test_an_unfinished_last_line_is_not_a_measurement(self):
        output = self.capture([self.usage()], suffix=json.dumps(self.usage(total=999)).encode())
        self.assertEqual(output["latest"]["token_usage_record"]["request"]["total_tokens"], 11)

    def test_oversized_and_malformed_records_do_not_hide_later_metrics(self):
        oversized = {"type": "event_msg", "payload": {"type": "user_message", "message": "x" * (probe.MAX_LINE + 1)}}
        output = self.capture([oversized, self.usage()], suffix=b"not json\n")
        self.assertEqual(output["coverage"]["skipped"], {"oversized": 1, "malformed": 1})
        self.assertEqual(output["latest"]["token_usage_record"]["thread"]["total_tokens"], 1000)

    def test_a_large_file_reads_only_the_declared_tail_budget(self):
        ignored = {"type": "response_item", "payload": {"type": "message", "content": "x" * 2000}}
        with mock.patch.object(probe, "MAX_BYTES", 1024):
            output = self.capture([ignored, ignored, self.usage()])
        self.assertTrue(output["coverage"]["tail_only"])
        self.assertLessEqual(output["coverage"]["bytes_read"], 1024)
        self.assertEqual(output["latest"]["token_usage_record"]["request"]["total_tokens"], 11)

    def test_absence_and_invalid_values_do_not_become_zero(self):
        record = self.usage()
        record["payload"]["usage"] = {"input_tokens": -1, "output_tokens": True, "total_tokens": 0}
        output = self.capture([record])
        self.assertEqual(output["latest"]["token_usage_record"]["request"], {"total_tokens": 0})
        self.assertEqual(probe.token_fields(None), None)


if __name__ == "__main__":
    unittest.main()
