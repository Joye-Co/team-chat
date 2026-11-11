#!/usr/bin/env python3
"""Fake ACP agent (JSON-RPC 2.0 on stdio, Agent Client Protocol v1): streams message chunks,
one tool call, and asks permission when the prompt says "approve": for a file write, or with
"merge" in the prompt for the shell command `gh pr merge 5`.

Sessions are kept in $HOME/acp-sessions/<id>.json (the prompts and the cwd), so a later
process can `session/load` one: the agent then replays the history as `session/update`s, as
real agents do. A prompt with "recall" answers with every earlier prompt of the session; one
with "slow" takes two seconds before it answers. FAKE_ACP_STORE: another folder for the
sessions. FAKE_ACP_NO_LOAD=1: the agent does not advertise `loadSession`."""
import json, os, sys, time

STORE = os.environ.get("FAKE_ACP_STORE") or os.path.join(os.environ.get("HOME", "."), "acp-sessions")

def out(o):
    o["jsonrpc"] = "2.0"; sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()

def upd(u):
    out({"method": "session/update", "params": {"sessionId": sid, "update": u}})

def path(s):
    return os.path.join(STORE, s + ".json")

def save():
    os.makedirs(STORE, exist_ok=True)
    with open(path(sid), "w") as f:
        json.dump(sess, f)

sid, sess = "s1", {"prompts": [], "cwd": None}
prompt_id = None
for line in sys.stdin:
    m = json.loads(line)
    meth, mid = m.get("method"), m.get("id")
    if meth == "initialize":
        caps = {} if os.environ.get("FAKE_ACP_NO_LOAD") else {"loadSession": True}
        out({"id": mid, "result": {"protocolVersion": 1, "agentCapabilities": caps}})
    elif meth == "session/new":
        sid = "s%d" % (len(os.listdir(STORE)) + 1 if os.path.isdir(STORE) else 1)
        sess = {"prompts": [], "cwd": m["params"]["cwd"]}
        save()
        out({"id": mid, "result": {"sessionId": sid}})
    elif meth == "session/load":
        want = m["params"]["sessionId"]
        if os.environ.get("FAKE_ACP_NO_LOAD") or not os.path.exists(path(want)):
            out({"id": mid, "error": {"code": -32002, "message": "Resource not found: " + want}})
            continue
        sid = want
        with open(path(sid)) as f:
            sess = json.load(f)
        for p in sess["prompts"]:
            upd({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": p}})
            upd({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "old answer"}})
        sess["cwd"] = m["params"]["cwd"]
        save()
        out({"id": mid, "result": {}})
    elif meth == "session/prompt":
        text = m["params"]["prompt"][0]["text"]
        prompt_id = mid
        earlier = list(sess["prompts"])
        sess["prompts"].append(text)
        save()
        upd({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "acp: "}})
        if "approve" in text:
            out({"id": 500, "method": "session/request_permission", "params": {"sessionId": sid,
                "toolCall": {"toolCallId": "call_1", "title": "Run shell", "kind": "execute", "rawInput": {"command": "gh pr merge 5"}}
                    if "merge" in text else {"toolCallId": "call_1", "title": "Write file"},
                "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, {"optionId": "no", "name": "Reject", "kind": "reject_once"}]}})
            continue
        if "slow" in text:
            time.sleep(2)
        if "recall" in text:
            text = "remembered [" + " | ".join(earlier) + "] in " + str(sess["cwd"])
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
