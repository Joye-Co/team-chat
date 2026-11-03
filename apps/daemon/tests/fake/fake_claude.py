#!/usr/bin/env python3
"""Fake Claude Code headless CLI: stream-json in and out, as the CLI docs describe.

Prompt words: "slow" streams 5 deltas 0.3 s apart; "hang" writes fake.pid and sleeps; "tick"
writes fake.pid and appends to ticks.txt 30 times, 0.1 s apart (a stopped process stops ticking).
The arguments are written to argv.json in the working directory.
The session file goes to $CLAUDE_CONFIG_DIR/projects/<cwd with non-alphanumerics as ->/<id>.jsonl.
"""
import json, os, re, sys, time

args = sys.argv[1:]
sid = args[args.index("--session-id") + 1] if "--session-id" in args else "fake-session"
assert "--input-format" in args and "stream-json" in args
open("argv.json", "w").write(json.dumps(args))
cfg = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(os.environ["HOME"], ".claude")
proj = os.path.join(cfg, "projects", re.sub(r"[^A-Za-z0-9]", "-", os.path.realpath(os.getcwd())))
os.makedirs(proj, exist_ok=True)
session = open(os.path.join(proj, sid + ".jsonl"), "a")

def out(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()

n = 0
for line in sys.stdin:
    msg = json.loads(line)
    text = "".join(c.get("text", "") for c in msg["message"]["content"])
    session.write(json.dumps({"type": "user", "text": text}) + "\n"); session.flush()
    if n == 0:
        out({"type": "system", "subtype": "init", "cwd": os.getcwd(), "session_id": sid, "model": "fake-model"})
    n += 1
    mid = "msg_%d" % n
    if "hang" in text:
        open("fake.pid", "w").write(str(os.getpid()))
        time.sleep(600)
    if "tick" in text:
        open("fake.pid", "w").write(str(os.getpid()))
        for i in range(30):
            with open("ticks.txt", "a") as t:
                t.write("%d\n" % i)
            time.sleep(0.1)
    out({"type": "stream_event", "event": {"type": "message_start", "message": {"id": mid}}, "session_id": sid})
    reply = "reply %d: %s" % (n, text)
    k = 5 if "slow" in text else 2
    step = max(1, len(reply) // k + 1)
    chunks = [reply[i:i + step] for i in range(0, len(reply), step)]
    for c in chunks:
        out({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": c}}, "session_id": sid})
        if "slow" in text:
            time.sleep(0.3)
    out({"type": "assistant", "message": {"id": mid, "content": [{"type": "text", "text": reply}]}, "session_id": sid})
    if n == 1:
        out({"type": "assistant", "message": {"id": mid, "content": [{"type": "tool_use", "id": "toolu_%d" % n, "name": "Bash", "input": {"command": "echo hi"}}]}, "session_id": sid})
        out({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_%d" % n, "content": "hi"}]}, "session_id": sid})
    session.write(json.dumps({"type": "assistant", "text": reply}) + "\n"); session.flush()
    out({"type": "result", "subtype": "success", "is_error": False, "result": reply, "session_id": sid,
         "usage": {"input_tokens": 3, "output_tokens": 4}, "total_cost_usd": 0.001})
