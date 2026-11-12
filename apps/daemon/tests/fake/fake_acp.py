#!/usr/bin/env python3
"""Fake ACP agent (JSON-RPC 2.0 on stdio, Agent Client Protocol v1): streams message chunks,
one tool call, and asks permission when the prompt says "approve": for a file write, or with
"merge" in the prompt for the shell command `gh pr merge 5`. The words count in the request, not
in the rules the server puts in front of it."""
import json, sys

def out(o):
    o["jsonrpc"] = "2.0"; sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()

def upd(u):
    out({"method": "session/update", "params": {"sessionId": "s1", "update": u}})

prompt_id = None
for line in sys.stdin:
    m = json.loads(line)
    meth, mid = m.get("method"), m.get("id")
    if meth == "initialize":
        out({"id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif meth == "session/new":
        out({"id": mid, "result": {"sessionId": "s1"}})
    elif meth == "session/prompt":
        text = m["params"]["prompt"][0]["text"]
        prompt_id = mid
        cue = text.rsplit("\nRequest from ", 1)[-1]
        upd({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "acp: "}})
        if "approve" in cue:
            out({"id": 500, "method": "session/request_permission", "params": {"sessionId": "s1",
                "toolCall": {"toolCallId": "call_1", "title": "Run shell", "kind": "execute", "rawInput": {"command": "gh pr merge 5"}}
                    if "merge" in cue else {"toolCallId": "call_1", "title": "Write file"},
                "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, {"optionId": "no", "name": "Reject", "kind": "reject_once"}]}})
            continue
        upd({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
        out({"id": mid, "result": {"stopReason": "end_turn"}})
    elif meth is None and mid == 500:
        picked = m["result"]["outcome"].get("optionId")
        upd({"sessionUpdate": "tool_call", "toolCallId": "call_1", "title": "Write file", "kind": "edit", "status": "completed" if picked == "yes" else "failed"})
        upd({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "permission " + str(picked)}})
        out({"id": prompt_id, "result": {"stopReason": "end_turn"}})
    elif meth == "session/cancel":
        pass
    elif mid is not None:
        out({"id": mid, "error": {"code": -32601, "message": "unknown"}})
