#!/usr/bin/env bun
// Live check of the 1.1 interaction loop and 引用提问 with the desktop app (INTERACT §7.1,
// REFS §A3): a fresh local `wrangler dev`, the SDK member Ledger and a test human.
//
// 1. Ledger (the SDK) posts 渠道明细 (a plain table) and the 7.1 report 10 月数据月报, whose
//    ask is from the test human, in one 话题.
// 2. The desktop app, signed in as the test human with the chat-core provider, sends
//    choice.change (有问题) on the report (`--selftest-live … --interact <dir>`).
// 3. Ledger patches in the rev 2 drill-down of §7.1; the app shows rev 2 with 按你的选择
//    (screenshot interact-rev2.png). The ops are §7.1's in the shipped protocol's words:
//    string table columns, no summaryRow, input kind textarea (§7.1 writes longtext).
// 4. The app adds the cell 渠道明细 · B 站 · 新增群 to the 话题 composer, types a question and
//    sends it. This script checks the stored message as Ledger gets it: `blockRefs` with the
//    cell, and the body led by the quote line the server wrote.
//
// Usage (repo root; the desktop binary must be built):
//   bun scripts/live-interact-check.ts [--port <n>] [--desktop <path>] [--out <dir>]
// Defaults: a free port, $CARGO_TARGET_DIR/debug/desktop, .tmp/live-interact/<stamp>.
// Writes <out>/report.json and exits 0 when every check passes. State, secrets and logs stay in
// <out> (mode 600 for secret files); nothing outside the repository's .tmp is written.

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import type { CardV2 } from "@chat/protocol";
import { blocks, Bot, type CardEventContext, composeCard, type MessageContext } from "../packages/bot-sdk/src/index.ts";

const ROOT = resolve(import.meta.dir, "..");
const SERVER = join(ROOT, "server");
const args = process.argv.slice(2);
const flag = (name: string) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

const QUESTION = "为什么比上月少了 1 个？";
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const OUT = resolve(flag("--out") ?? join(ROOT, ".tmp", "live-interact", stamp));
const DESKTOP = resolve(
  flag("--desktop") ?? join(process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"), "debug", "desktop"),
);

async function freePort(): Promise<number> {
  for (;;) {
    const port = await new Promise<number>((res, rej) => {
      const s = createServer();
      s.once("error", rej);
      s.listen(0, "127.0.0.1", () => {
        const p = (s.address() as { port: number }).port;
        s.close(() => res(p));
      });
    });
    // wrangler also takes port + 1000 for the inspector.
    if (port + 1000 < 65535) return port;
  }
}

const report: Record<string, unknown> = { out: OUT, steps: [] as string[] };
const step = (s: string) => {
  (report.steps as string[]).push(s);
  console.log(`- ${s}`);
};
const checks: Record<string, boolean> = {};
const check = (name: string, ok: boolean) => {
  checks[name] = ok;
  if (!ok) console.error(`FAIL ${name}`);
};

async function waitFor<T>(what: string, fn: () => T | undefined | null | false, ms = 20_000): Promise<T> {
  const until = Date.now() + ms;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > until) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 20));
  }
}

mkdirSync(OUT, { recursive: true });
const port = Number(flag("--port") ?? 0) || (await freePort());
const url = `http://127.0.0.1:${port}`;
const persist = join(OUT, "state");
const vars = join(OUT, "dev.vars");
const adminSecret = Buffer.from(crypto.getRandomValues(new Uint8Array(24))).toString("base64url");
writeFileSync(vars, `ADMIN_SECRET=${adminSecret}\n`, { mode: 0o600 });
chmodSync(vars, 0o600);
const wrangler = join(SERVER, "node_modules", ".bin", "wrangler");
const env = { ...process.env, CI: "1", WRANGLER_SEND_METRICS: "false" };

let server: ChildProcess | null = null;
let ledger: Bot | null = null;
let ok = false;
try {
  const mig = spawnSync(wrangler, ["d1", "migrations", "apply", "DB", "--local", "--persist-to", persist], {
    cwd: SERVER,
    env,
    encoding: "utf8",
  });
  if (mig.status !== 0) throw new Error(`d1 migrations failed: ${mig.stderr || mig.stdout}`);
  step("local D1 migrated");
  const log = openSync(join(OUT, "wrangler.log"), "a");
  server = spawn(
    wrangler,
    ["dev", "--port", String(port), "--inspector-port", String(port + 1000), "--ip", "127.0.0.1",
      "--persist-to", persist, "--env-file", vars, "--show-interactive-dev-session=false"],
    { cwd: SERVER, env, stdio: ["ignore", log, log], detached: true },
  );
  writeFileSync(join(OUT, "wrangler.pid"), `${server.pid}\n`);
  const until = Date.now() + 60_000;
  for (;;) {
    try {
      if ((await fetch(`${url}/v1/health`)).ok) break;
    } catch {}
    if (Date.now() > until) throw new Error("wrangler dev did not start");
    await new Promise((r) => setTimeout(r, 250));
  }
  step(`wrangler dev on ${url} (pid ${server.pid})`);

  async function post(path: string, body: unknown, headers: Record<string, string>): Promise<any> {
    const r = await fetch(`${url}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", ...headers },
      body: JSON.stringify(body),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(`${path} -> ${r.status} ${JSON.stringify(data)}`);
    return data;
  }
  const boot = await post("/v1/admin/bootstrap", { ownerName: "Joye" }, { "x-admin-secret": adminSecret });
  const auth = { authorization: `Bearer ${boot.token}` };
  const groupId: string = boot.group.id;
  const made = await post("/v1/bots", { name: "Ledger", role: "test" }, auth);
  const ledgerId: string = made.member.id;
  const invite = await post("/v1/invites", { name: "测试员", kind: "human", level: "member" }, auth);
  const redeemed = await post("/v1/auth/redeem", { inviteCode: invite.code, deviceName: "live-check" }, {});
  const humanId: string = redeemed.member.id;
  const tokenFile = join(OUT, "human.token");
  writeFileSync(tokenFile, `${redeemed.token}\n`, { mode: 0o600 });
  chmodSync(tokenFile, 0o600);
  step(`owner, the SDK member Ledger (${ledgerId}) and the test human 测试员 (${humanId}) made`);

  // Ledger: the §7.1 rev 2 patch on choice.change 有问题; records the question it is asked.
  const events: CardEventContext[] = [];
  const asked: MessageContext[] = [];
  ledger = new Bot({ url, token: made.token, stateFile: join(OUT, "ledger.state.json"), logLevel: "warn", markRead: false });
  ledger.onCardEvent(async (ctx) => {
    events.push(ctx);
    if (ctx.blockId !== "check" || ctx.name !== "choice.change") return;
    if ((ctx.value as { value?: string }).value !== "wrong") return;
    await ctx.patch([
      { op: "insert", after: "main", block: { id: "drill", type: "group", title: "按渠道拆开 · 新增群", collapsible: false, children: [] } },
      {
        op: "insert",
        parent: "drill",
        block: {
          id: "by_channel",
          type: "table",
          columns: ["渠道", "新增群", "占比", "比 9 月"],
          rows: [["小红书", "14", "37%", "+6"], ["群内邀请", "10", "26%", "+1"], ["B 站", "9", "24%", "−1"], ["抖音", "5", "13%", "−2"]],
          rowKeys: ["xhs", "invite", "bili", "douyin"],
          select: "multiple",
          events: ["table.rowSelect"],
        },
      },
      { op: "insert", parent: "drill", block: { id: "what", type: "input", kind: "textarea", label: "哪一项不对？后台看到的是多少？", required: false, maxLength: 300 } },
      { op: "remove", id: "note" },
      {
        op: "card",
        props: {
          ask: { id: "verify", from: [humanId], blocking: false },
          actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "by_channel", "what"] }],
        },
      },
    ] as never);
  });
  ledger.onMention(async (ctx) => {
    asked.push(ctx);
  });
  await ledger.start();

  const table = composeCard({
    intent: "info",
    title: "渠道明细",
    blocks: [
      blocks.table({
        id: "channels",
        columns: ["渠道", "新增群", "占比", "比 9 月"],
        rows: [["小红书", "14", "37%", "+6"], ["群内邀请", "10", "26%", "+1"], ["B 站", "9", "24%", "−1"], ["抖音", "5", "13%", "−2"]],
        rowKeys: ["xhs", "invite", "bili", "douyin"],
      }),
    ],
  });
  const first = await ledger.postCardV2({ groupId }, table);
  const topicId = first.message.topicId;
  const fixture = JSON.parse(readFileSync(join(ROOT, "protocol/fixtures/entities/card-v2-report-compose.json"), "utf8")).value as CardV2;
  const card = { ...fixture, ask: { ...(fixture.ask as NonNullable<CardV2["ask"]>), from: [humanId] } };
  const posted = await ledger.postCardV2({ groupId, topicId }, card);
  const messageId = posted.message.id;
  report.server = { url, groupId, topicId, reportMessageId: messageId, tableMessageId: first.message.id, ledgerId, humanId };
  step(`Ledger posted 渠道明细 and the 7.1 report (${messageId}) in 话题 ${topicId}`);

  // The desktop app, signed in as the test human.
  const shots = join(OUT, "shots");
  const desktopReport = join(OUT, "desktop-report.json");
  const desktopLog = openSync(join(OUT, "desktop.log"), "a");
  const app = spawn(DESKTOP, ["--selftest-live", url, "--token-file", tokenFile, "--report", desktopReport, "--interact", shots], {
    cwd: ROOT,
    stdio: ["ignore", desktopLog, desktopLog],
  });
  step(`desktop app started (pid ${app.pid})`);
  const code = await new Promise<number | null>((res) => app.once("exit", (c) => res(c)));
  const dr = JSON.parse(readFileSync(desktopReport, "utf8"));
  report.desktop = { exitCode: code, report: dr };
  step(`desktop app ended with ${code}: ${dr.ok ? "ok" : dr.error}`);
  check("desktop report ok", dr.ok === true && code === 0);

  // The live event reached Ledger from the human; the stored question carries its reference.
  const ev = events.find((e) => e.messageId === messageId && e.name === "choice.change");
  check("Ledger got choice.change from the test human", ev?.actorId === humanId && (ev?.value as { value?: string })?.value === "wrong");
  check("the app saw rev 2 caused by its event", dr.report_rev_after === 2 && dr.cause_actor_is_me === true);
  const q = await waitFor("the question at Ledger", () => asked.find((c) => c.message.body.endsWith(QUESTION)), 10_000).catch(() => undefined);
  const refs = q?.message.blockRefs ?? [];
  const r0 = refs[0];
  report.question = q ? { id: q.message.id, body: q.message.body, blockRefs: refs, refs: q.refs, question: q.question } : null;
  check("the stored question has one blockRef", refs.length === 1);
  check(
    "the blockRef is the B 站 · 新增群 cell of 渠道明细",
    r0?.messageId === first.message.id && r0?.blockId === "channels" && r0?.part?.kind === "cell" &&
      (r0?.part?.keys as Record<string, unknown> | undefined)?.row === "bili" &&
      (r0?.part?.keys as Record<string, unknown> | undefined)?.col === 1,
  );
  check("the body starts with the quote line prefix", !!r0 && (q?.message.body.startsWith(`> ↳ ${r0.label}\n\n`) ?? false));
  check("the question without the prefix", q?.question?.endsWith(QUESTION) ?? false);
  ok = Object.values(checks).every(Boolean) && Object.keys(checks).length > 0;
} catch (err) {
  report.error = String(err);
  console.error(err);
} finally {
  report.checks = checks;
  report.ok = ok;
  await ledger?.stop().catch(() => {});
  if (server?.pid) {
    try {
      process.kill(-server.pid, "SIGTERM"); // the group we started: wrangler and workerd
    } catch {}
  }
  writeFileSync(join(OUT, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`report: ${join(OUT, "report.json")}`);
}
process.exit(ok ? 0 : 1);
