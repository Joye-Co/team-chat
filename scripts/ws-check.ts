#!/usr/bin/env bun
// Usage: bun scripts/ws-check.ts <base url> <token file>. Prints counts only, never the token.
// Read-only WebSocket check: hello -> welcome, fetch each group -> page, ping -> pong.
import { readFileSync } from "node:fs";
const [base, tokenFile] = process.argv.slice(2);
if (!base || !tokenFile) { console.error("usage: bun scripts/ws-check.ts <base url> <token file>"); process.exit(2); }
const token = readFileSync(tokenFile, "utf8").trim();
const ws = new WebSocket(base.replace(/^http/, "ws") + "/v1/ws", { headers: { authorization: `Bearer ${token}` } } as any);
const t0 = Date.now();
const out: Record<string, unknown> = {};
const pages: { events: number; lastSeq: number }[] = [];
let pending = 0;
const timer = setTimeout(() => { console.log(JSON.stringify({ ok: false, reason: "timeout", out })); process.exit(1); }, 15000);
ws.onopen = () => ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "ws-probe", version: "0" } }));
ws.onmessage = (e) => {
  const f = JSON.parse(String(e.data));
  if (f.t === "welcome") {
    out.welcomeMs = Date.now() - t0;
    out.me = { name: f.me.name, level: f.me.level };
    out.groups = f.groups.map((g: any) => ({ name: g.group.name, lastSeq: g.lastSeq }));
    out.members = f.members.length;
    for (const g of f.groups) { pending++; ws.send(JSON.stringify({ t: "fetch", rid: g.group.id, groupId: g.group.id, afterSeq: 0, limit: 50 })); }
    ws.send(JSON.stringify({ t: "ping" }));
  } else if (f.t === "page") { pending--; pages.push({ events: f.events.length, lastSeq: f.lastSeq }); }
  else if (f.t === "pong") out.pong = true;
  else if (f.t === "error" || (f.t === "ack" && !f.ok)) { console.log(JSON.stringify({ ok: false, frame: f.t, error: f.error })); process.exit(1); }
  if (out.pong && out.welcomeMs !== undefined && pending === 0) { clearTimeout(timer); console.log(JSON.stringify({ ok: true, ...out, pages })); ws.close(); process.exit(0); }
};
ws.onerror = () => { console.log(JSON.stringify({ ok: false, reason: "socket error" })); process.exit(1); };
