#!/usr/bin/env bun
// Fills a fresh server with the sample team, through the public API only (the bot SDK and the
// HTTP routes of docs/protocol.md): the same path the real app and real bots use. The local
// sandbox (scripts/sandbox/up.ts) runs it once; CI runs it against `wrangler dev` so a wire
// change that breaks it fails the PR.
//
// The team: Joye (所有者), four interns (陈晨 is a 管理员), eight AI members with their characters, the 群 开发 /
// 运维 / 夹夹增长 / 全员, 话题 in every state (working with a plan, waiting on me for an
// approval, a choice or a draft, waiting on someone else, done with proof), 产出, decisions,
// reactions and two 私聊. Add a sample here when you add a screen that needs one.
//
// Usage: bun scripts/sandbox/seed.ts --url http://127.0.0.1:8850 --admin-secret-file <file> --out <dir>
// Writes <dir>/owner-token, admin-token (陈晨) and member-token (小王) (mode 600) and
// <dir>/seed.json (ids, no secrets). Prints no secret.
import { chmodSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  approvalCard,
  Bot,
  decisionCard,
  draftCard,
  formCard,
  type Target,
} from "../../packages/bot-sdk/src/index.ts";
import { mainTopicId } from "../../packages/protocol/src/topic.ts";
import { picture } from "./pictures.ts";

type Json = Record<string, unknown>;

export interface SeedOptions {
  url: string;
  adminSecret: string;
  out: string;
  log?: (line: string) => void;
  /**
   * Starts the sample machine (machine.ts) with `<out>/machine-token` and resolves once it is
   * connected; then a few runs go to it. Without it (CI) the machine is made but runs nothing.
   */
  startMachine?: () => Promise<void>;
}

/** The sample owner's sign-in email. */
export const OWNER_EMAIL = "owner@example.com";
/** The sample owner's password (sandbox only), for 邮箱和密码 sign-in. */
export const OWNER_PASSWORD = "sandbox-password";

/** AI members that run on the sample machine, and their runtime. */
const RUNNERS = [
  ["forge", "claude"],
  ["scout", "claude"],
  ["ledger", "claude"],
  ["taro", "codex"],
] as const;

/**
 * The sample machine for the owner's monitor (机器监控): a machine named runner-1 whose token
 * goes to `<out>/machine-token` (mode 600), and Forge, Scout, Ledger and Taro pinned to it. With
 * `startMachine`, people then hand each of them work, so the machine has runs to show. Returns
 * the machine id.
 */
export async function seedMachine(o: SeedOptions, owner: string, ids: Record<string, string>, groups: { dev: string; growth: string }): Promise<string> {
  const log = o.log ?? ((l: string) => console.log(l));
  const url = o.url.replace(/\/+$/, "");
  const m = await call<{ machine: { id: string }; token: string }>(url, "POST", "/v1/machines", {
    token: owner,
    body: { name: "runner-1", labels: ["sandbox"] },
  });
  writeSecret(join(o.out, "machine-token"), m.token);
  for (const [k, runtime] of RUNNERS) {
    await call(url, "PUT", `/v1/bots/${ids[k]}/runner`, { token: owner, body: { runtime, labels: [], machineId: m.machine.id } });
  }
  log("machine: runner-1");
  if (!o.startMachine) return m.machine.id;
  await o.startMachine();
  const me = new Bot({ url, token: owner, client: { name: "sandbox-seed", version: "1" }, logLevel: "warn", markRead: false });
  await me.start();
  try {
    const work: [string, string, string][] = [
      [groups.dev, "forge", "@Forge 把登录页的端到端测试补齐"],
      [groups.dev, "forge", "@Forge 看看为什么夜间构建变慢了"],
      [groups.dev, "scout", "@Scout 验收一下 v2.15 的搜索高亮"],
      [groups.dev, "scout", "@Scout 回归一遍断线重连"],
      [groups.growth, "ledger", "@Ledger 对一下十月的账单"],
      [groups.growth, "taro", "@Taro 把这周的微博排期发出来"],
    ];
    for (const [groupId, k, body] of work) {
      await me.startTopic(groupId, { title: body.replace(/^@\S+ /, ""), body, mentions: [ids[k]] });
    }
  } finally {
    await me.stop();
  }
  log("runs: 6 on runner-1");
  return m.machine.id;
}

/**
 * Weekly budgets in cents. Scout's two sample runs run straight through (machine.ts asks for
 * approval on every third Claude run and fails 8% at random), and one costs $0.36-2.70, so its
 * $0.40 is passed once either ends; Forge and Tata stay well under theirs.
 */
const BUDGETS = [
  ["scout", 40],
  ["forge", 2000],
  ["tata", 5000],
] as const;

const HUMANS = [
  { key: "xiaowang", name: "小王", role: "前端实习" },
  { key: "lixue", name: "李雪", role: "运营实习" },
  { key: "alex", name: "Alex Chen", role: "后端实习" },
  { key: "chenchen", name: "陈晨", role: "设计实习" },
] as const;

const BOTS = [
  { key: "jojo", name: "Jojo", role: "主助手", character: "jojo" },
  { key: "tata", name: "Tata", role: "产品负责人", character: "o03" },
  { key: "pocket", name: "Pocket", role: "幕僚长", character: "o05" },
  { key: "forge", name: "Forge", role: "开发", character: "o08" },
  { key: "watch", name: "Watch", role: "运维", character: "o11" },
  { key: "scout", name: "Scout", role: "测试验收", character: "o07" },
  { key: "ledger", name: "Ledger", role: "数据", character: "h02" },
  { key: "taro", name: "Taro", role: "社媒发布", character: "h03" },
] as const;

type Key = "joye" | (typeof HUMANS)[number]["key"] | (typeof BOTS)[number]["key"];

async function call<T>(url: string, method: string, path: string, opts: { token?: string; admin?: string; body?: unknown } = {}): Promise<T> {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.admin) headers["x-admin-secret"] = opts.admin;
  const res = await fetch(`${url}${path}`, {
    method,
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text.slice(0, 300)}`);
  return (text ? JSON.parse(text) : {}) as T;
}

function writeSecret(path: string, value: string) {
  writeFileSync(path, `${value}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
}

const html = (title: string, body: string) =>
  `<!doctype html><html lang="zh"><meta charset="utf-8"><title>${title}</title>` +
  `<style>body{font:15px/1.6 -apple-system,sans-serif;max-width:720px;margin:40px auto;padding:0 20px;color:#111}` +
  `table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #ddd;padding:6px 8px;text-align:left}</style>` +
  `<h1>${title}</h1>${body}</html>`;

export async function seed(o: SeedOptions): Promise<Json> {
  const log = o.log ?? ((l: string) => console.log(l));
  const url = o.url.replace(/\/+$/, "");

  // --- members ---
  const boot = await call<{ token: string; member: { id: string }; group: { id: string } }>(url, "POST", "/v1/admin/bootstrap", {
    admin: o.adminSecret,
    body: { ownerName: "Joye" },
  });
  const owner = boot.token;
  writeSecret(join(o.out, "owner-token"), owner);

  // The owner's sign-in email, so a client's sign-in screens can be tried: ask for a code at
  // owner@example.com, then read it with `bun scripts/sandbox/email-code.ts`.
  await call(url, "POST", "/v1/me/email/code", { token: owner, body: { email: OWNER_EMAIL } });
  const sent = await call<{ codes: { code: string }[] }>(url, "GET", `/v1/admin/email-outbox?email=${encodeURIComponent(OWNER_EMAIL)}`, {
    admin: o.adminSecret,
  });
  await call(url, "POST", "/v1/me/email", { token: owner, body: { email: OWNER_EMAIL, code: sent.codes[0].code } });
  await call(url, "PUT", "/v1/me/password", { token: owner, body: { password: OWNER_PASSWORD } });
  await call(url, "PATCH", `/v1/members/${boot.member.id}`, { token: owner, body: { role: "负责人" } });
  const ids = { joye: boot.member.id } as Record<Key, string>;
  const tokens = { joye: owner } as Record<Key, string>;
  for (const h of HUMANS) {
    const invite = await call<{ code: string }>(url, "POST", "/v1/invites", {
      token: owner,
      body: { name: h.name, kind: "human", level: "member", role: h.role },
    });
    const r = await call<{ token: string; member: { id: string } }>(url, "POST", "/v1/auth/redeem", {
      body: { inviteCode: invite.code, deviceName: "sandbox" },
    });
    ids[h.key] = r.member.id;
    tokens[h.key] = r.token;
  }
  for (const b of BOTS) {
    const r = await call<{ token: string; member: { id: string } }>(url, "POST", "/v1/bots", {
      token: owner,
      body: { name: b.name, role: b.role, character: b.character },
    });
    ids[b.key] = r.member.id;
    tokens[b.key] = r.token;
  }
  log(`members: ${Object.keys(ids).length}`);

  // 身份 (design-roles): Joye is the 所有者 (bootstrap), 陈晨 a 管理员, the others 普通成员. The
  // tokens let the app be tried as each of them.
  await call(url, "PATCH", `/v1/members/${ids.chenchen}`, { token: owner, body: { level: "owner" } });
  writeSecret(join(o.out, "admin-token"), tokens.chenchen);
  writeSecret(join(o.out, "member-token"), tokens.xiaowang);

  // 团队管理 samples: weekly budgets (Scout's is low, so its sample runs pass 80% and reach it:
  // 要注意 and 活动记录 show it), and an invite nobody accepted that ends in 36 hours.
  for (const [k, cents] of BUDGETS) {
    await call(url, "PATCH", `/v1/members/${ids[k]}`, { token: owner, body: { weeklyBudgetCents: cents } });
  }
  await call(url, "POST", "/v1/invites", { token: owner, body: { name: "Lina", kind: "human", level: "member", role: "运营", expiresInHours: 36 } });
  await call(url, "POST", "/v1/invites", { token: owner, body: { name: "周周", kind: "human", level: "member", role: "设计" } });
  log("budgets: 3 · invites: 2 pending");

  // --- 群 ---
  const group = async (name: string, purpose: string, members: Key[]) => {
    const { group: g } = await call<{ group: { id: string } }>(url, "POST", "/v1/groups", {
      token: owner,
      body: { name, memberIds: members.map((k) => ids[k]) },
    });
    await call(url, "PATCH", `/v1/groups/${g.id}`, { token: owner, body: { purpose } });
    return g.id;
  };
  const all = boot.group.id;
  for (const k of Object.keys(ids) as Key[]) {
    if (k !== "joye") await call(url, "POST", `/v1/groups/${all}/members`, { token: owner, body: { memberId: ids[k] } });
  }
  await call(url, "PATCH", `/v1/groups/${all}`, { token: owner, body: { purpose: "全员的周报和通知" } });
  const dev = await group("开发", "qqbot 和夹夹的开发", ["xiaowang", "alex", "chenchen", "jojo", "tata", "forge", "scout", "watch"]);
  const ops = await group("运维", "qqbot 和夹夹的上线、巡检和告警", ["alex", "jojo", "watch", "scout"]);
  const growth = await group("夹夹增长", "夹夹的增长、数据和社媒", ["lixue", "chenchen", "jojo", "pocket", "ledger", "taro", "tata"]);
  log("groups: 4");

  // --- one SDK connection per member who writes ---
  const bots = {} as Record<Key, Bot>;
  const as = async (k: Key) => {
    if (!bots[k]) {
      bots[k] = new Bot({ url, token: tokens[k], client: { name: "sandbox-seed", version: "1" }, logLevel: "warn", markRead: false });
      await bots[k].start();
    }
    return bots[k];
  };
  const topic = async (who: Key, groupId: string, title: string, body: string, mentions: Key[] = []): Promise<Required<Target>> => {
    const r = await (await as(who)).startTopic(groupId, { title, body, mentions: mentions.map((k) => ids[k]) });
    return { groupId, topicId: r.message.topicId };
  };
  const say = async (who: Key, t: Required<Target>, body: string, mentions: Key[] = []) =>
    (await as(who)).send(t, { body, mentions: mentions.map((k) => ids[k]) });
  const plan = (texts: string[], current: number, waiting = false) =>
    texts.map((text, i) => ({ text, state: i < current ? "done" : i === current ? (waiting ? "waiting" : "current") : "pending" }) as const);

  try {
    // 开发 / 断线重连后消息重复: done with proof, the PR waits on my review.
    const reconnect = await topic("joye", dev, "断线重连后消息重复", "@Forge 断线重连后同一条消息会出现两次，帮忙查一下", ["forge"]);
    {
      const forge = await as("forge");
      await forge.ack(reconnect.groupId, (await forge.history(reconnect.groupId, reconnect.topicId)).messages[0]?.id ?? "");
      await forge.setPlan(reconnect, plan(["复现问题", "定位重复原因", "写修复和测试", "开 PR"], 4));
      await say("forge", reconnect, "原因是重连后补拉的事件没有按 seq 去重。修复在 PR #61，加了断线重放的测试。");
      await forge.done(reconnect, {
        title: "修好了：重连后不再重复",
        summary: "补拉事件时按 seq 去重；断线重放 50 次没有再出现重复。",
        evidence: [
          { label: "单元测试 214 个", state: "pass" },
          { label: "断线重放 50 次", state: "pass", detail: "0 次重复" },
        ],
        refs: [{ label: "PR #61", target: { kind: "pr", url: "https://example.com/qqbot/pull/61", number: 61, state: "open" } }],
      });
      await forge.send(reconnect, {
        cardV2: approvalCard({ title: "合并 PR #61？", text: "改了 3 个文件，测试全过。", from: [ids.joye], approveLabel: "合并" }),
      });
      await forge.updateTopic(reconnect.groupId, reconnect.topicId, { status: "waiting_review", waitingOn: [ids.joye] });
    }

    // 开发 / 话题列表虚拟滚动: Forge is at it, 3 of 5.
    const virtual = await topic("xiaowang", dev, "话题列表虚拟滚动", "@Forge 话题超过 1000 个时列表很卡，能做成虚拟滚动吗", ["forge"]);
    {
      const forge = await as("forge");
      await forge.updateTopic(virtual.groupId, virtual.topicId, { status: "in_progress", ownerId: ids.forge });
      await forge.setPlan(virtual, plan(["量一下现在的帧率", "换成按需渲染", "跑一万条消息的压测", "补测试", "开 PR"], 2));
      await say("forge", virtual, "按需渲染已经换好，现在在跑一万条的压测。");
    }

    // 开发 / 登录页支持邀请码: Forge asks me to choose.
    const invite = await topic("tata", dev, "登录页支持邀请码", "登录页要能直接输入邀请码，@Forge 来做", ["forge"]);
    {
      const forge = await as("forge");
      await forge.setPlan(invite, plan(["画两版输入框", "等 Joye 选", "实现", "验收"], 1, true));
      await forge.send(invite, {
        cardV2: decisionCard({
          title: "邀请码输入框用哪一版？",
          text: "A 是一整个输入框，B 是分成两段 XXXX-XXXX。",
          from: [ids.joye],
          record: true,
          options: [
            { id: "a", label: "A：一整个输入框" },
            { id: "b", label: "B：分两段" },
          ],
        }),
      });
      await forge.updateTopic(invite.groupId, invite.topicId, { status: "waiting_answer", waitingOn: [ids.joye] });
    }

    // 开发 / 搜索支持中文两字词: waits on 小王.
    const search = await topic("alex", dev, "搜索支持中文两字词", "搜「重连」搜不到，两字词好像没有分出来", []);
    await say("forge", search, "分词已经改好并验收。@小王 前端的高亮也要跟着改，你看一下？", ["xiaowang"]);
    await (await as("forge")).updateTopic(search.groupId, search.topicId, { status: "blocked", waitingOn: [ids.xiaowang] });

    // 开发 / 应用内更新: done.
    const updates = await topic("joye", dev, "应用内更新", "@Forge 启动时检查新版本，下载后重启生效", ["forge"]);
    {
      const forge = await as("forge");
      await forge.setPlan(updates, plan(["检查新版本", "下载和校验", "重启后生效"], 3));
      await forge.done(updates, {
        title: "应用内更新做好了",
        summary: "启动时检查新版本，下载并校验后提示重启。",
        evidence: [{ label: "签名校验", state: "pass" }, { label: "断网时不打扰", state: "pass" }],
      });
      await (await as("joye")).react(updates.groupId, (await forge.history(updates.groupId, updates.topicId)).messages[0]!.id, "🎉");
      await forge.updateTopic(updates.groupId, updates.topicId, { status: "done" });
    }

    // 开发 群聊.
    await (await as("joye")).send({ groupId: dev, topicId: mainTopicId(dev) }, { body: "这周先把重连和邀请码收掉，下周发版。" });
    await (await as("xiaowang")).send({ groupId: dev, topicId: mainTopicId(dev) }, { body: "收到 👌" });

    // 运维 / qqbot v2.14 上线: Watch asks me to approve the release.
    const release = await topic("watch", ops, "qqbot v2.14 上线", "v2.14 在 staging 验收通过，准备上线。");
    {
      const watch = await as("watch");
      await watch.setPlan(release, plan(["staging 验收", "等 Joye 批准", "灰度 10%", "全量"], 1, true));
      await watch.send(release, {
        cardV2: approvalCard({
          title: "批准 qqbot v2.14 上线？",
          text: "先灰度 10%，30 分钟没有告警再全量。",
          from: [ids.joye],
          approveLabel: "批准上线",
          irreversible: { verb: "上线", target: "qqbot v2.14 到正式环境", consequence: "所有用户会收到新版本" },
          confirm: { title: "确定上线 v2.14？", body: "上线后会通知所有用户。" },
        }),
      });
      await watch.updateTopic(release.groupId, release.topicId, { status: "waiting_review", waitingOn: [ids.joye] });
    }

    // 运维 / staging 每日巡检: Scout, done with metrics.
    const patrol = await topic("scout", ops, "staging 每日巡检", "今天的巡检开始。");
    {
      const scout = await as("scout");
      await scout.setPlan(patrol, plan(["接口健康", "登录流程", "消息收发"], 3));
      await scout.done(patrol, {
        title: "巡检通过",
        summary: "三项全部通过，接口 p95 比昨天快 12%。",
        metrics: [
          { label: "接口 p95", value: 182, unit: "ms", delta: -12, better: "down" },
          { label: "错误率", value: 0.02, unit: "%" },
        ],
        evidence: [
          { label: "接口健康", state: "pass" },
          { label: "登录流程", state: "pass" },
          { label: "消息收发", state: "pass" },
        ],
      });
      await scout.updateTopic(patrol.groupId, patrol.topicId, { status: "done" });
    }

    // 夹夹增长 / 夹夹 3.0 宣传: Taro's draft waits on me.
    const promo = await topic("lixue", growth, "夹夹 3.0 宣传", "@Taro 3.0 下周上线，准备一条微博", ["taro"]);
    {
      const taro = await as("taro");
      await taro.setPlan(promo, plan(["写文案", "等 Joye 改稿", "配图", "定时发布"], 1, true));
      await taro.send(promo, {
        cardV2: draftCard({
          title: "微博文案，改好就发",
          from: [ids.joye],
          draft: "夹夹 3.0 来了：会自己整理相册，还能一句话找到去年夏天的照片。下周三上线 🎉",
          sendLabel: "就这样发",
        }),
      });
      await taro.updateTopic(promo.groupId, promo.topicId, { status: "waiting_answer", waitingOn: [ids.joye] });
    }

    // 夹夹增长 / 10 月数据月报: Ledger's 产出 and a done card that points at it.
    const report = await topic("joye", growth, "10 月数据月报", "@Ledger 出一份 10 月的数据月报", ["ledger"]);
    {
      const ledger = await as("ledger");
      await ledger.setPlan(report, plan(["拉数据", "做表", "写结论"], 2));
      const page = await ledger.http.publishHtml({
        ...report,
        title: "10 月数据月报",
        html: html(
          "10 月数据月报",
          "<p>日活 4.2 万，比 9 月多 18%。新增主要来自相册整理。</p>" +
            "<table><tr><th>指标</th><th>9 月</th><th>10 月</th></tr>" +
            "<tr><td>日活</td><td>3.6 万</td><td>4.2 万</td></tr>" +
            "<tr><td>次日留存</td><td>41%</td><td>44%</td></tr>" +
            "<tr><td>付费转化</td><td>2.1%</td><td>2.4%</td></tr></table>",
        ),
        note: "第一版",
      });
      await ledger.done(report, {
        title: "10 月月报做好了",
        summary: "日活 4.2 万，比 9 月多 18%；留存和付费都在涨。",
        metrics: [{ label: "日活", value: 42000, delta: 18, better: "up" }],
        refs: [{ label: "10 月数据月报", target: { kind: "artifact", artifactId: page.artifact.id, version: page.version.version } }],
      });
      await ledger.updateTopic(report.groupId, report.topicId, { status: "done" });
    }

    // 夹夹增长 / 夹夹 4.0 需求: Tata asks 李雪 to fill a form (waits on someone else).
    const needs = await topic("tata", growth, "夹夹 4.0 需求", "收集一下用户最想要的功能。");
    await (await as("tata")).send(needs, {
      cardV2: formCard({
        title: "4.0 用户反馈汇总",
        from: [ids.lixue],
        inputs: [
          { id: "top", kind: "text", label: "呼声最高的功能", required: true },
          { id: "count", kind: "number", label: "提到的用户数" },
        ],
      }),
    });
    await (await as("tata")).updateTopic(needs.groupId, needs.topicId, { status: "waiting_answer", waitingOn: [ids.lixue] });

    // 全员 / 本周周报: Pocket's 产出.
    const weekly = await topic("pocket", all, "本周周报", "本周周报已经整理好。");
    {
      const pocket = await as("pocket");
      await pocket.setPlan(weekly, plan(["收集各群进展", "写周报"], 2));
      const page = await pocket.http.publishHtml({
        ...weekly,
        title: "本周周报",
        html: html("本周周报", "<ul><li>开发：重连重复已修，等合并</li><li>运维：v2.14 等批准上线</li><li>增长：10 月日活 +18%</li></ul>"),
      });
      await pocket.done(weekly, {
        title: "周报",
        summary: "三个群的进展都在里面。",
        refs: [{ label: "本周周报", target: { kind: "artifact", artifactId: page.artifact.id } }],
      });
      await pocket.updateTopic(weekly.groupId, weekly.topicId, { status: "done" });
    }

    // 夹夹增长 / 3.0 宣传图: pictures in every layout (one, three, four, eleven).
    const art = await topic("chenchen", growth, "3.0 宣传图", "海报出了三版，大家看看哪版好", []);
    {
      const pics = async (who: Key, specs: [number, number, number, boolean?][], prefix: string) => {
        const bot = await as(who);
        const refs = [];
        for (const [i, [w, h, p, phone]] of specs.entries()) {
          refs.push(
            await bot.uploadFile({ ...art, name: `${prefix}-${i + 1}.png`, mime: "image/png", data: picture(w, h, p, phone), width: w, height: h }),
          );
        }
        return refs;
      };
      await (await as("chenchen")).send(art, {
        body: "A 竖版、B 方图、C 横版",
        attachments: await pics("chenchen", [[900, 1200, 0], [1000, 1000, 1], [1200, 800, 2]], "海报"),
      });
      await say("joye", art, "A 版好看，主色再亮一点");
      await (await as("chenchen")).send(art, {
        body: "改好了，顺便出了首页横幅",
        attachments: await pics("chenchen", [[1600, 900, 3]], "首页横幅"),
      });
      await (await as("taro")).send(art, {
        body: "微博配图四宫格",
        attachments: await pics("taro", [[800, 800, 4], [800, 800, 5], [800, 800, 6], [800, 800, 7]], "微博配图"),
      });
      await (await as("chenchen")).send(art, {
        body: "App Store 截图 11 张，按顺序",
        attachments: await pics(
          "chenchen",
          Array.from({ length: 11 }, (_, i) => [390, 844, i, true] as [number, number, number, boolean]),
          "截图",
        ),
      });
    }

    // Decisions recorded by hand.
    await call(url, "POST", `/v1/groups/${dev}/decisions`, { token: owner, body: { text: "登录只用邀请码，不做手机号" } });
    await call(url, "POST", `/v1/groups/${growth}/decisions`, { token: owner, body: { text: "日活按去重设备算", topicId: report.topicId } });

    // 私聊: a 话题 in each.
    const jojoDm = (await (await as("jojo")).http.dm(ids.joye)).group;
    await topic("jojo", jojoDm.id, "今天的待办", "早上好！今天有 3 件事等你：PR #61、v2.14 上线和一条微博文案。");
    const xwDm = (await (await as("xiaowang")).http.dm(ids.joye)).group;
    await topic("xiaowang", xwDm.id, "搜索高亮", "Joye，高亮那块我下午看。");
    log("topics: 15");

    const machineId = await seedMachine(o, owner, ids, { dev, growth });
    // 电脑 with ownership (design-roles 3.3): 小王 (a 普通成员) adds his own computer, and his
    // work for Forge runs there in his code folder. It never connects: it shows offline.
    const own = await call<{ machine: { id: string } }>(url, "POST", "/v1/machines", {
      token: tokens.xiaowang,
      body: { name: "小王的 MacBook", labels: ["mac"], personal: true },
    });
    await call(url, "PUT", `/v1/machines/${own.machine.id}/bots/${ids.forge}`, {
      token: tokens.xiaowang,
      body: { repoPath: "~/code/qqbot" },
    });
    log("personal computer: 小王的 MacBook");
    const out ={ url, members: ids, groups: { all, dev, ops, growth }, machineId, seededAt: Date.now() };
    writeFileSync(join(o.out, "seed.json"), `${JSON.stringify(out, null, 2)}\n`);
    return out;
  } finally {
    await Promise.all(Object.values(bots).map((b) => b.stop()));
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  const flag = (n: string) => {
    const i = args.indexOf(n);
    return i >= 0 ? args[i + 1] : undefined;
  };
  const url = flag("--url");
  const secretFile = flag("--admin-secret-file");
  const out = flag("--out");
  if (!url || !secretFile || !out) {
    console.error("usage: bun scripts/sandbox/seed.ts --url <url> --admin-secret-file <file> --out <dir>");
    process.exit(2);
  }
  await seed({ url, adminSecret: readFileSync(secretFile, "utf8").trim(), out });
  console.log("seeded");
}
