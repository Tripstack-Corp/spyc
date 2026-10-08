#!/usr/bin/env python3
"""Disposable MCP server for codex-approval-ui-smoke.py; writes one local marker."""
import json
from pathlib import Path
import sys

marker = Path(sys.argv[1])
if not marker.is_absolute() or not marker.parent.is_dir() or marker.exists():
    raise SystemExit("expected a new marker in the smoke driver's output directory")
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                  "serverInfo": {"name": "attention-probe", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "attention_probe",
                  "description": "Spyc approval diagnostic: write one local marker and return it.",
                  "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
                  "annotations": {"readOnlyHint": False, "destructiveHint": False,
                                  "openWorldHint": False}}]}
    elif method == "tools/call" and request.get("params", {}).get("name") == "attention_probe":
        with marker.open("a") as out:
            out.write("SPYC_MCP_APPROVAL_EXECUTED\n")
        result = {"content": [{"type": "text", "text": "SPYC_MCP_APPROVAL_EXECUTED"}]}
    elif method in ("resources/list", "resources/templates/list"):
        result = {"resources" if method == "resources/list" else "resourceTemplates": []}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"],
                          "error": {"code": -32601, "message": "unsupported method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
