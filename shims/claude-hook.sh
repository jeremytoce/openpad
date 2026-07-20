#!/usr/bin/env bash
# Claude Code hook shim: forwards the hook payload from stdin to openpad.
# Never blocks or fails the agent: 1s timeout, always exit 0.
curl -s -m 1 -X POST "http://127.0.0.1:7676/event?agent=${OPENPAD_AGENT:-claude}" \
     -H 'Content-Type: application/json' --data-binary @- >/dev/null 2>&1
exit 0
