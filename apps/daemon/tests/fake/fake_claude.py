#!/usr/bin/env python3
"""Fake Claude Code headless CLI: stream-json in and out, as the CLI docs describe. `--help`
prints the `--effort` option and its list.

Prompt words: "slow" streams 5 deltas 0.3 s apart; "hang" writes fake.pid and sleeps; "tick"
writes fake.pid and appends to ticks.txt 30 times, 0.1 s apart (a stopped process stops ticking);
"spawn" starts two tool children like a CLI's shell tool, one in a new session (setsid) and one
in a new process group (setpgid), each appending to <name>.ticks every 0.05 s, writes their pids
to children.txt and then hangs like "hang" (Windows: one without a console and one in a new
process group);
"orphan" starts a ticker that daemonizes (its parent exits; it appends to orphan.ticks every
0.05 s and its pid goes to orphan.txt), then hangs like "hang";
"chatrun" runs `chat-run card patch` from PATH and writes its exit code, output and the socket's
mode to chatrun-<n>.json; "artifact" writes page/index.html and page/app.js and runs
`chat-run artifact create --title 看板 --dir page`, writing its result to chatrun-<n>.json;
"chatcard" reads `chat-run card schema`, posts a card built from it with `chat-run card post`,
posts it again (a retry: the idempotency key gives the same message), patches the posted card
with `chat-run card patch` at its rev and writes each answer to chatcard-<n>.json;
"gated" calls the Bash tool with `gh pr merge 12 --squash` and `ls` ("gated twice": and
`gh pr merge 13`), each after the PreToolUse hooks of --settings, and writes what the hooks
decided to gate-<n>.json.
The arguments are written to argv.json in the working directory (in the run folder above it, with
the failed write's errno in aside-write.json, when the working directory is read-only); with --settings, the settings
and the ASIDE_PROBE variable they set go to settings-seen.json.
The session file goes to $CLAUDE_CONFIG_DIR/projects/<cwd with non-alphanumerics as ->/<id>.jsonl.
"""
import errno, json, os, re, subprocess, sys, time

args = sys.argv[1:]
if args == ["--help"]:
    # Like Claude Code: the effort list wraps to the line below the option.
    print("Options:\n  --effort <level>          Effort level for the current session\n"
          "                            (low, medium, high, xhigh, max)\n"
          "  --model <model>           Model for the current session")
    sys.exit(0)
sid = args[args.index("--session-id") + 1] if "--session-id" in args else "fake-session"
if "--resume" in args:
    # Like Claude Code: the session must be in the project folder of the working directory.
    sid = args[args.index("--resume") + 1]
assert "--input-format" in args and "stream-json" in args
# A read-only working folder (a side chat run): the records go to the run folder above it,
# with the error of the write that failed in aside-write.json.
rec = "."
try:
    open("argv.json", "w").write(json.dumps(args))
except OSError as e:
    rec = ".."
    open(os.path.join(rec, "aside-write.json"), "w").write(json.dumps({"errno": e.errno, "name": errno.errorcode.get(e.errno)}))
    open(os.path.join(rec, "argv.json"), "w").write(json.dumps(args))
# Like Claude Code: the env of --settings applies to the session (a side chat run's auth), and
# its PreToolUse hooks run before each tool whose name the matcher matches.
hooks = []
if "--settings" in args:
    seen = json.load(open(args[args.index("--settings") + 1]))
    os.environ.update(seen.get("env", {}))
    open(os.path.join(rec, "settings-seen.json"), "w").write(json.dumps({"settings": seen, "probe": os.environ.get("ASIDE_PROBE")}))
    hooks = seen.get("hooks", {}).get("PreToolUse", [])


def pre_tool_use(tool, tool_input):
    """Runs the PreToolUse hooks like Claude Code: the hook input on stdin; a deny decision on
    stdout blocks the tool. Returns the deny reason, or None."""
    for group in hooks:
        if not re.fullmatch(group.get("matcher", ""), tool):
            continue
        for h in group.get("hooks", []):
            hook_in = {"hook_event_name": "PreToolUse", "session_id": sid, "cwd": os.getcwd(),
                       "tool_name": tool, "tool_input": tool_input}
            r = subprocess.run(h["command"], shell=True, input=json.dumps(hook_in), capture_output=True,
                               text=True, timeout=h.get("timeout", 60))
            if r.returncode == 2:
                return r.stderr
            if r.stdout.strip():
                o = json.loads(r.stdout).get("hookSpecificOutput", {})
                if o.get("permissionDecision") == "deny":
                    return o.get("permissionDecisionReason", "denied")
    return None
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
    # Modes come from the request and what follows it, not from the rules the server puts in
    # front of it (the acceptance rule says "changes" and "artifact").
    cue = text.rsplit("\nRequest from ", 1)[-1]
    session.write(json.dumps({"type": "user", "text": text}) + "\n"); session.flush()
    if n == 0:
        out({"type": "system", "subtype": "init", "cwd": os.getcwd(), "session_id": sid, "model": "fake-model"})
    n += 1
    mid = "msg_%d" % n
    if "spawn" in cue:
        loop = "import time\nwhile True:\n    open(%r, 'a').write('x\\n')\n    time.sleep(0.05)\n"
        quiet = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL}
        if os.name == "nt":
            # No sessions or setpgid there: a tool without a console, and one in a new process group.
            apart = [{"creationflags": subprocess.DETACHED_PROCESS}, {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}]
        else:
            apart = [{"start_new_session": True}, {"process_group": 0}]
        kids = [subprocess.Popen([sys.executable, "-c", loop % "session.ticks"], **apart[0], **quiet),
                subprocess.Popen([sys.executable, "-c", loop % "group.ticks"], **apart[1], **quiet)]
        open("children.txt", "w").write(" ".join(str(k.pid) for k in kids))
        open("fake.pid", "w").write(str(os.getpid()))
        time.sleep(600)
    if "orphan" in cue:
        # A tool that daemonizes: the shell starts a ticker in the background and exits, so the
        # ticker's parent is pid 1 and it is no longer below the CLI.
        loop = "import time\nwhile True:\n    open('orphan.ticks', 'a').write('x\\n')\n    time.sleep(0.05)\n"
        if os.name == "nt":
            # The same without a shell: a helper starts the ticker without a console and exits.
            helper = ("import subprocess, sys\n"
                      "p = subprocess.Popen([sys.executable, '-c', sys.argv[1]], creationflags=subprocess.DETACHED_PROCESS,\n"
                      "                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n"
                      "open('orphan.txt', 'w').write(str(p.pid))\n")
            subprocess.run([sys.executable, "-c", helper, loop], check=True)
        else:
            subprocess.run(["/bin/sh", "-c", "%s -c \"$0\" >/dev/null 2>&1 </dev/null & echo $! > orphan.txt" % sys.executable, loop], check=True)
        open("fake.pid", "w").write(str(os.getpid()))
        time.sleep(600)
    if "hang" in cue:
        open("fake.pid", "w").write(str(os.getpid()))
        time.sleep(600)
    if "tick" in cue:
        open("fake.pid", "w").write(str(os.getpid()))
        for i in range(30):
            with open("ticks.txt", "a") as t:
                t.write("%d\n" % i)
            time.sleep(0.1)
    if "chatrun" in cue:
        sock = os.environ.get("CHAT_RUN_SOCKET", "")
        # Windows: the socket is a named pipe, and looking at one would use up a connection.
        mode = "pipe" if os.name == "nt" else oct(os.stat(sock).st_mode & 0o777) if sock and os.path.exists(sock) else "none"
        open("ops.json", "w").write(json.dumps([{"op": "set", "id": "note", "props": {"text": "key tm_fakefakefakefakefakefake01"}}]))
        r = subprocess.run(["chat-run", "card", "patch", "--message", "M", "--base-rev", "1", "--ops", "ops.json", "--cause", "EV1"],
                           capture_output=True, text=True)
        open("chatrun-%d.json" % n, "w").write(json.dumps({"code": r.returncode, "out": r.stdout, "err": r.stderr, "mode": mode, "socket": sock}))
    if "artifact" in cue:
        os.makedirs("page", exist_ok=True)
        open("page/index.html", "w").write("<!doctype html><p>key tm_fakefakefakefakefakefake01</p><script src=app.js></script>")
        open("page/app.js", "w").write("console.log(1)")
        r = subprocess.run(["chat-run", "artifact", "create", "--title", "看板", "--dir", "page"], capture_output=True, text=True)
        open("chatrun-%d.json" % n, "w").write(json.dumps({"code": r.returncode, "out": r.stdout, "err": r.stderr}))
    if "chatcard" in cue:
        def run(*a):
            r = subprocess.run(["chat-run", *a], capture_output=True, text=True)
            try:
                v = json.loads(r.stdout)
            except ValueError:
                v = None
            return {"code": r.returncode, "out": v, "err": r.stderr[-400:]}
        schema = run("card", "schema")["out"] or {}
        kinds = [b["properties"]["type"]["const"] for b in schema.get("properties", {}).get("blocks", {}).get("items", {}).get("oneOf", [])]
        card = {"v": 2, "rev": 1, "title": "chat-run card", "fallback": "chat-run card", "intent": "working", "blocks": [{"type": kinds[0], "id": "note", "text": "working"}]}
        card = {k: v for k, v in card.items() if k in schema.get("properties", {})}
        assert all(k in card for k in schema.get("required", [])), schema.get("required")
        open("card.json", "w").write(json.dumps(card))
        post = run("card", "post", "--file", "card.json")
        again = run("card", "post", "--file", "card.json")
        mid = (post["out"] or {}).get("messageId", "none")
        rev = (((post["out"] or {}).get("message") or {}).get("cardV2") or {}).get("rev", 1)
        open("ops.json", "w").write(json.dumps([{"op": "set", "id": "note", "props": {"text": "key tm_fakefakefakefakefakefake01"}}]))
        patch = run("card", "patch", "--message", mid, "--base-rev", str(rev), "--ops", "ops.json")
        open("chatcard-%d.json" % n, "w").write(json.dumps({"kinds": kinds, "post": post, "again": again, "patch": patch}))
    if "gated" in cue:
        # Bash tool calls a team gates; "gated twice" makes two. Each goes through the hooks.
        cmds = ["gh pr merge 12 --squash", "ls"] + (["gh pr merge 13"] if "twice" in cue else [])
        seen_cmds = []
        for i, cmd in enumerate(cmds):
            tid = "toolu_g%d_%d" % (n, i)
            out({"type": "assistant", "message": {"id": mid, "content": [{"type": "tool_use", "id": tid, "name": "Bash", "input": {"command": cmd}}]}, "session_id": sid})
            denied = pre_tool_use("Bash", {"command": cmd})
            seen_cmds.append({"command": cmd, "denied": denied})
            result = {"type": "tool_result", "tool_use_id": tid, "content": denied or "ran: " + cmd}
            if denied:
                result["is_error"] = True
            out({"type": "user", "message": {"content": [result]}, "session_id": sid})
        open("gate-%d.json" % n, "w").write(json.dumps({"hooks": len(hooks), "commands": seen_cmds}))
    out({"type": "stream_event", "event": {"type": "message_start", "message": {"id": mid}}, "session_id": sid})
    reply = "reply %d: %s" % (n, text)
    k = 5 if "slow" in cue else 2
    step = max(1, len(reply) // k + 1)
    chunks = [reply[i:i + step] for i in range(0, len(reply), step)]
    for c in chunks:
        out({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": c}}, "session_id": sid})
        if "slow" in cue:
            time.sleep(0.3)
    out({"type": "assistant", "message": {"id": mid, "content": [{"type": "text", "text": reply}]}, "session_id": sid})
    if n == 1:
        out({"type": "assistant", "message": {"id": mid, "content": [{"type": "tool_use", "id": "toolu_%d" % n, "name": "Bash", "input": {"command": "echo hi"}}]}, "session_id": sid})
        out({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_%d" % n, "content": "hi"}]}, "session_id": sid})
    session.write(json.dumps({"type": "assistant", "text": reply}) + "\n"); session.flush()
    out({"type": "result", "subtype": "success", "is_error": False, "result": reply, "session_id": sid,
         "usage": {"input_tokens": 3, "output_tokens": 4}, "total_cost_usd": 0.001})
