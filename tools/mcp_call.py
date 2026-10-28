#!/usr/bin/env python3
"""Minimal modern-era stdio client for driving torori (or any
fleet server) from a shell: sends server/discover, then one
tools/call from a JSON file, printing both responses.

Usage: mcp_call.py <binary> <args-json-file> [binary args...]
Environment for the server is inherited.
"""

import json
import subprocess
import sys

META = {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientInfo": {"name": "torori-mcp-call", "version": "0"},
    "io.modelcontextprotocol/clientCapabilities": {},
}


def frame(id_, method, params):
    params = dict(params)
    params["_meta"] = META
    return json.dumps({"id": id_, "jsonrpc": "2.0", "method": method, "params": params})


def main() -> int:
    binary, args_file, *rest = sys.argv[1:]
    with open(args_file, encoding="utf-8") as fh:
        call = json.load(fh)
    proc = subprocess.Popen(
        [binary, *rest],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    proc.stdin.write(frame(1, "server/discover", {}) + "\n")
    proc.stdin.write(frame(2, "tools/call", call) + "\n")
    proc.stdin.flush()
    for _ in range(2):
        line = proc.stdout.readline()
        if not line:
            break
        print(line.strip())
    proc.stdin.close()
    proc.wait(timeout=30)
    return 0


if __name__ == "__main__":
    sys.exit(main())
