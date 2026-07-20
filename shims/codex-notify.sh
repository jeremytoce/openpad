#!/usr/bin/env bash
# Codex notify shim: argv[1] is a JSON payload with a "type" field.
curl -s -m 1 -X POST "http://127.0.0.1:7676/event?agent=codex" \
     -H 'Content-Type: application/json' --data-binary "${1:-{}}" >/dev/null 2>&1
exit 0
