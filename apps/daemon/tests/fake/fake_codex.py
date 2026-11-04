#!/usr/bin/env python3
"""Fake `codex app-server`: JSON-RPC lines on stdio following the v2 schema
(`codex app-server generate-ts`). A turn streams 4 deltas 0.2 s apart; turn/steer input is
appended to the reply; a command approval is asked when the prompt says "approve". When the
prompt says "late", the turn waits for a steer and then completes before it answers the steer
with an error (a steer that arrives as the turn ends). When the prompt says "chatrun", the turn
runs `chat-run resolve` from PATH and writes its exit code, output, `CHAT_RUN_DIR`,
`CHAT_RUN_SOCKET` and the folder's mode and files to chatrun.json."""
import json, os, subprocess, sys, threading, time

assert sys.argv[1:2] == ["app-server"]
lock = threading.Lock()
def out(o):
    with lock:
        sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()

thread = "th-1"
path = os.path.join(os.getcwd(), "..", "codex-rollout.jsonl")
state = {"turn": None, "steer": [], "n": 0, "approval": None}
pending = {}

def run_turn(tid, text):
    out({"method": "turn/started", "params": {"threadId": thread, "turn": {"id": tid, "items": [], "status": "inProgress"}}})
    if "approve" in text:
        ev = threading.Event(); pending[99] = ev
        out({"id": 99, "method": "item/commandExecution/requestApproval", "params": {"threadId": thread, "turnId": tid, "itemId": "c1", "command": "rm -rf build", "reason": "clean"}})
        ev.wait(10)
        dec = state["approval"]
        out({"method": "item/completed", "params": {"threadId": thread, "turnId": tid, "item": {"type": "commandExecution", "id": "c1", "command": "rm -rf build", "status": "completed" if dec == "accept" else "declined", "aggregatedOutput": "", "exitCode": 0 if dec == "accept" else None}}})
    if "chatrun" in text:
        d = os.environ.get("CHAT_RUN_DIR", "")
        mode = oct(os.stat(d).st_mode & 0o777) if d and os.path.isdir(d) else "none"
        r = subprocess.run(["chat-run", "resolve", "--event", "EV1", "--result", "ignored"], capture_output=True, text=True)
        files = sorted(os.listdir(d)) if d and os.path.isdir(d) else []
        open("chatrun.json", "w").write(json.dumps({"code": r.returncode, "out": r.stdout, "err": r.stderr, "dir": d,
            "socket": os.environ.get("CHAT_RUN_SOCKET"), "mode": mode, "files": files}))
    mid = "m-" + tid
    reply = "codex: " + text
    late = "late" in text
    if late:
        state["late_go"] = threading.Event(); state["late_done"] = threading.Event()
        with lock:
            state["late_turn"] = tid
    for i in range(4):
        out({"method": "item/agentMessage/delta", "params": {"threadId": thread, "turnId": tid, "itemId": mid, "delta": "."}})
        time.sleep(0.2)
    if late:
        state["late_go"].wait(10)
    with lock:
        steer = state["steer"]; state["steer"] = []; state["turn"] = None
    if steer:
        reply += " + " + " + ".join(steer)
    open(path, "a").write(json.dumps({"turn": tid, "reply": reply}) + "\n")
    out({"method": "item/completed", "params": {"threadId": thread, "turnId": tid, "item": {"type": "agentMessage", "id": mid, "text": reply}}})
    out({"method": "turn/completed", "params": {"threadId": thread, "turn": {"id": tid, "items": [], "status": "completed", "error": None}}})
    if late:
        state["late_done"].set()

for line in sys.stdin:
    m = json.loads(line)
    meth, mid = m.get("method"), m.get("id")
    if meth is None and mid in pending:
        state["approval"] = m["result"]["decision"]; pending.pop(mid).set(); continue
    if meth == "initialize":
        out({"id": mid, "result": {"userAgent": "fake", "codexHome": "/tmp", "platformFamily": "unix", "platformOs": "macos"}})
    elif meth == "initialized":
        pass
    elif meth == "thread/start":
        assert m["params"]["cwd"] == os.getcwd() or os.path.realpath(m["params"]["cwd"]) == os.path.realpath(os.getcwd())
        out({"id": mid, "result": {"thread": {"id": thread, "path": os.path.realpath(path)}, "model": "fake-codex"}})
    elif meth == "turn/start":
        state["n"] += 1
        tid = "tu-%d" % state["n"]
        with lock:
            state["turn"] = tid
        out({"id": mid, "result": {"turn": {"id": tid, "items": [], "status": "inProgress"}}})
        text = "".join(i["text"] for i in m["params"]["input"])
        threading.Thread(target=run_turn, args=(tid, text), daemon=True).start()
    elif meth == "turn/steer" and state.get("late_turn") == m["params"]["expectedTurnId"]:
        # The turn completes first; the steer then fails because no turn is active.
        state["late_turn"] = None
        state["late_go"].set(); state["late_done"].wait(10)
        out({"id": mid, "error": {"code": -32600, "message": "no active turn"}})
    elif meth == "turn/steer":
        with lock:
            ok = state["turn"] == m["params"]["expectedTurnId"]
            if ok:
                state["steer"].append("".join(i["text"] for i in m["params"]["input"]))
        if ok:
            out({"id": mid, "result": {"turnId": m["params"]["expectedTurnId"]}})
        else:
            out({"id": mid, "error": {"code": -32600, "message": "no active turn"}})
    elif meth == "turn/interrupt":
        out({"id": mid, "result": {}})
    elif mid is not None:
        out({"id": mid, "error": {"code": -32601, "message": "unknown"}})
