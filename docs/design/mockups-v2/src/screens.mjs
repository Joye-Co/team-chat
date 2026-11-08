// Screen definitions for the v2 mockups.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { M } from "./data.mjs";
import { REPO, avatar, character, chip, esc, fit, icon, initials, kbd, midEll } from "./kit.mjs";
import { mainWindow, m, mentionPop, react } from "./main.mjs";

const mn = (n, me = false) => `<span class="mn ${me ? "me" : ""}">@${n}</span>`;

// ---------- Main window: a long topic with people and bots, a bot working, a card, a file ----------
export const mainTopic = (mode) => ({
  title: "夜间告警降噪：合并重复的 5xx",
  status: "waiting_answer",
  you: true,
  edge: true,
  cardVisible: true,
  people: ["tata", "forge", "li", "watch", "scout", "joye"],
  messages: [
    { m: "watch", time: "昨天 23:58", text: "<p><b>夜间巡检</b>：staging 共 37 条 5xx，34 条来自 <code>/api/share</code>，原因都是上游超时。生产正常。</p>" },
    { date: "今天" },
    { m: "tata", time: "09:12", text: `昨晚 staging 有 37 条 5xx 告警，其实都是同一个上游超时。${mn("Forge")} 能不能把同一原因的告警合并成一条？` },
    { m: "li", time: "09:40", text: "合并后要保留第一条的时间，复盘要用。" },
    { m: "li", cont: true, time: "09:41", text: `另外值班卡片也要跟着改，${mn("Watch")} 记一下。`, after: `<div class="reacts">${react("👍", "Joye、Tata", true)}${react("👀", "Watch")}</div>` },
    {
      m: "forge", time: "10:02",
      pre: `<div class="steps">${icon("chevron-right", 12)}看了 6 个文件，运行 3 条命令<span class="d">· 4 分 12 秒</span></div>`,
      text: `<p><b>改好了</b>：同一路由、同一错误码，10 分钟内只发一条，后面的次数累加到这一条上。第一条的时间保留。</p><p>PR 已开，等 ${mn("Scout")} 在 staging 验收。</p>`,
      after: `<div class="file"><span class="ft">${icon("file-code", 20)}</span><div class="grow"><div class="fn ell">告警合并规则说明</div><div class="fm">网页产出 · v2 · 18 KB</div></div><span class="ib">${icon("square-arrow-out-up-right", 18)}</span></div>`,
    },
    {
      m: "watch", time: "10:41", noPip: true,
      text: "",
      after: `<div class="card you"><div class="ct">要在 staging 先打开合并规则吗？</div>
<div class="cs">你选之前我不会改任何环境。生产要等 Scout 验收后再单独确认。</div>
<div class="opts"><div class="opt">${kbd("A")}<span class="grow">现在在 staging 打开</span></div><div class="opt">${kbd("B")}<span class="grow">等 Scout 验收完再开</span></div><div class="opt">${kbd("C")}<span class="grow">先不改，回到讨论</span></div></div></div>`,
    },
    { m: "li", time: "10:44", text: "我倾向 B，staging 上周刚因为配置出过一次事。" },
    {
      html: `<div class="live"><span class="gut" style="width:36px;display:flex;justify-content:center">${avatar(m("scout"), 24, mode, { pip: false })}</span><span class="lbl"><b>Scout</b> 正在 staging 上验收</span><span class="pulse"><i></i><i></i><i></i></span><span class="el">2:14</span><span class="t3">·</span><span class="linkbtn">查看过程</span></div>`,
    },
  ],
  composer: { draft: `验收结果出来后直接发在这里，有问题先改规则，不用再问我 <span class="mn-a">${mn("Sc")}${mentionPop(mode)}</span>` },
});

// ---------- Short topic: no void; topic start fills the top; side panel open ----------
export const shortTopic = (mode) => ({
  title: "周四微博：拾光 3.0 上线",
  status: "waiting_review",
  you: true,
  owner: "taro",
  people: ["taro", "joye", "li"],
  start: "Joye 发起 · Taro 负责 · 今天 14:05",
  messages: [
    { m: "joye", time: "14:05", text: `${mn("Taro")} 把拾光 3.0 上线写成一条微博，周四晚上发。重点讲“一键分享到群”。` },
    { m: "taro", time: "14:06", hover: true, noPip: true, text: "收到。先写两版标题，再配图。" },
    {
      m: "taro", time: "14:31", noPip: true,
      text: "<p><b>草稿好了</b>，按你上次喜欢的口吻写的。确认后我才会发出。</p>",
      after: `<div class="card you"><div class="ct">发布预览</div>
<div class="dl"><div class="r"><span class="k">平台</span><span>微博</span></div><div class="r"><span class="k">时间</span><span>周四 20:00</span></div><div class="r"><span class="k">标题</span><span class="ell">拾光 3.0：用户群的照片，一键分享回群里</span></div>
<div class="body">以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群…</div></div>
<div class="acts"><span class="btn primary">发布</span><span class="btn secondary">修改</span><span class="btn ghost">放弃</span></div></div>`,
    },
  ],
  composer: { placeholder: "回复「周四微博：拾光 3.0 上线」" },
});

const shortPanel = (mode) => `<aside class="sp"><div class="sp-hdr"><div class="r1"><span class="t">话题详情</span><span class="ib">${icon("x", 18)}</span></div><div class="r2"><span class="ell">Taro 负责 · 周四 20:00 截止</span></div></div>
<div class="sp-body">
<div class="sp-sec"><div class="h">Taro 的进度</div>
<div class="live-card"><div class="top">${avatar(m("taro"), 36, mode, { pip: false })}<div><div class="nm">Taro</div><div class="st">等你审发布预览</div></div></div>
<div class="plan"><div class="s ok">${icon("circle-check", 16)}写标题和正文</div><div class="s ok">${icon("circle-check", 16)}生成 3 张配图</div><div class="s now">${icon("eye", 16)}等 Joye 确认</div><div class="s later">${icon("circle-dashed", 16)}周四 20:00 发出</div></div></div></div>
<div class="sp-sec"><div class="h">产出 · 2</div>
<div class="prow"><span class="ft">${icon("file-text", 16)}</span><div class="grow"><div class="a ell">微博草稿</div><div class="b">网页 · v3 · 今天 14:31</div></div></div>
<div class="prow"><span class="ft">${icon("file-image", 16)}</span><div class="grow"><div class="a ell">配图 3 张</div><div class="b">图片 · 2.4 MB</div></div></div></div>
<div class="sp-sec"><div class="h">成员 · 3</div>
${["joye", "taro", "li"].map((k) => `<div class="mrow">${avatar(m(k), 24, mode, { pip: false })}<span>${esc(m(k).name)}</span><span class="grow"></span><span class="role">${m(k).char ? icon("bot", 12) : ""}${esc(m(k).role)}</span></div>`).join("")}
<div class="mrow add"><span class="slot">${icon("user-plus", 16)}</span><span>添加成员</span></div></div>
</div></aside>`;

export function main(mode) {
  return mainWindow(mode, { topic: mainTopic(mode) });
}

export function short(mode) {
  return mainWindow(mode, {
    topic: shortTopic(mode),
    panel: shortPanel,
    tlw: 300,
    sidebar: {
      todo: [{ icon: "eye", title: "周四微博：拾光 3.0 上线", ctx: "社媒 · Taro 等你审" }],
      groups: [
        { name: "orderbot 研发", palette: "sky" },
        { name: "社媒", selected: true },
        { name: "拾光增长", palette: "sage" },
        { name: "全员", muted: true },
      ],
      dms: [{ m: "pocket" }, { m: "li" }, { m: "jojo" }],
    },
    group: { name: "社媒", counts: [4, 0, 1] },
    topics: [
      { title: "周四微博：拾光 3.0 上线", owner: "taro", who: "Taro", text: "草稿好了，按你上次喜欢的口吻写的", time: "14:31", status: "waiting_review", you: true, selected: true },
      { title: "B 站视频：用户群剪辑", owner: "taro", who: "Taro", text: "已排期，周六 12:00 发", time: "昨天", status: "done" },
      { title: "博客改写：一个人带 5 个机器人", owner: "pocket", who: "Pocket", text: "初稿写到第三节", time: "周一", status: "in_progress" },
      { title: "X 上的回复要不要统一口吻", owner: "joye", who: "李雪", text: "我整理了 10 条例子", time: "10/27" },
    ],
  });
}

// ---------- Worst case: minimum window, long names, huge counts, emoji names, long roles ----------
export function worst(mode) {
  const longOwner = { name: "Aleksandra Wiśniewska-Kowalczyk", online: true, role: "Senior backend engineer（外包，周二周四在线）" };
  const emoji = { name: "🧑‍💻 Priya", online: false, role: "前端实习生" };
  const topic = {
    title: "【紧急】生产环境 v0.9.2 发布后 iOS 端 WebSocket 在弱网下频繁断线重连并导致消息重复与已读位置错乱的问题排查",
    status: "waiting_answer", you: false, name: "Aleksandra Wiśniewska-Kowalczyk", whoM: longOwner,
    people: ["watch", "forge", "li", "chen", "alex", "joye", "tata", "scout", "wang"],
    messages: [
            { mm: longOwner, time: "23:59", text: "https://staging.example.com/api/v1/organizations/9f8e7d6c-5b4a-4321-9876-0123456789ab/websocket/reconnect?attempt=17&reason=network_change_while_suspended" },
      { mm: emoji, time: "00:01", text: "👍👍👍", after: `<div class="reacts">${react("👀", "99+")}${react("✅", "14", true)}</div>` },
      { m: "watch", time: "00:03", text: `<p><b>3 个例外</b>：重连第 17 次时 seq 回退；已读位置写入了旧值；推送重复 2 次。</p><p>日志超过 2000 行，放进产出了。</p>`, after: `<div class="file"><span class="ft">${icon("file-text", 20)}</span><div class="grow"><div class="fn ell">${midEll("websocket-reconnect-trace-2025-11-04T23-59-59-production-ios-17.4.1-build-4412.log", 46, 14)}</div><div class="fm">文件 · 112.4 MB · 正在下载 64%</div></div><span class="ib">${icon("x", 18)}</span><span class="trk"><i style="width:64%"></i></span></div>` },
      { m: "joye", time: "00:05", text: `我来看。<span class="sendst">${icon("circle-alert", 14)}没有发出</span><span class="btn text">重试</span>` },
    ],
    composer: { placeholder: "回复「【紧急】生产环境 v0.9.2 发布后 iOS 端 WebSocket 在弱网下…」", typing: "李雪、陈晨 和另外 3 人正在输入…" },
  };
  return mainWindow(mode, {
    topic,
    sbw: 232, tlw: 288,
    sidebar: {
      todoCount: 128,
      me: "Joye Huang（周末不看消息，急事打电话）",
      todo: [
        { icon: "circle-question-mark", title: "【紧急】生产环境 v0.9.2 发布后 iOS 端 WebSocket 断线", ctx: "Infrastructure & Reliability Engineering · Aleksandra Wiśniewska-Kowalczyk 等你回答" },
        { icon: "at-sign", title: "👀👀👀", ctx: "全员 · 🧑‍💻 Priya @你" },
      ],
      groups: [
        { name: "Infrastructure & Reliability Engineering 夜间值班", mention: "99+", selected: true },
        { name: "😀 团建", unread: true },
        { name: "全员", muted: true },
      ],
      dms: [{ m: longOwner, count: 1234 }, { m: emoji }, { m: { ...M.pocket, name: "Pocket" } }],
    },
    group: { name: "Infrastructure & Reliability Engineering 夜间值班", counts: [999, 1204, 99] },
    topics: [
      { title: topic.title, owner: "watch", who: "Watch", text: "3 个例外：重连第 17 次时 seq 回退", time: "00:03", status: "waiting_answer", name: "Aleksandra Wiśniewska-Kowalczyk", selected: true },
      { title: "x", ownerM: longOwner, who: "Aleksandra Wiśniewska-Kowalczyk", text: "ok", time: "23:59", mention: "99+" },
      { title: "（无标题）", ownerM: emoji, who: "🧑‍💻 Priya", text: "[图片]", time: "24/12/31", unread: 999 },
      { title: "DMG 签名：缺少开发者证书，暂时发未签名版本，下周再看", owner: "forge", who: "Forge", text: "等证书", time: "10/1", status: "blocked" },
      { title: "Search CJK bigrams and mixed English tokens", owner: "ledger", who: "Ledger", text: "recall 96%", time: "9/30", status: "done" },
    ],
  });
}

// ---------- Quick switcher (Cmd+K) ----------
export function switcher(mode) {
  // Rows use the sidebar's glyphs (§7.11): # for a group, the lead avatar for a topic and a message.
  const hash = `<span class="slot">${icon("hash", 18)}</span>`;
  const lead = (k, sel = false) => `<span class="slot">${avatar(m(k), 24, mode, { presence: false, sel })}</span>`;
  // One mark for every match, in every row type (§7.11).
  const hl = '<span class="hl">告警</span>';
  const ov = () => `<div class="overlay"></div><div class="pop sw" style="z-index:21">
<div class="sw-q">${icon("search", 20)}<span class="q">告警</span><span class="caret"></span><span class="grow"></span>${kbd("esc")}</div>
<div class="sw-l">
<div class="ph">话题</div>
<div class="pr act">${lead("forge", true)}<div class="grow ell"><span class="b">夜间${hl}降噪：合并重复的 5xx</span><span class="g"> · orderbot 研发</span></div>${chip("waiting_answer", { you: true, row: true })}<span class="t3">${icon("corner-down-left", 16)}</span></div>
<div class="pr">${lead("li")}<div class="grow ell"><span class="b">${hl}值班表 11 月</span><span class="g"> · 运维告警</span></div></div>
<div class="pr">${lead("watch")}<div class="grow ell"><span class="b">5xx ${hl}阈值调整</span><span class="g"> · orderbot 研发</span></div>${chip("done")}</div>
<div class="ph">群</div>
<div class="pr">${hash}<div class="grow ell"><span class="b">运维${hl}</span><span class="g"> · 6 位成员</span></div></div>
<div class="ph">消息</div>
<div class="pr tall">${lead("watch")}<div class="grow" style="min-width:0"><div class="ell"><span class="b">Watch</span><span class="g"> · 运维告警 · 昨天</span></div><div class="ell sn">今晚 3 条 5xx ${hl}都是同一个上游超时，已合并</div></div></div>
</div>
<div class="sw-f"><span>${kbd("↑")}${kbd("↓")} 选择</span><span>${kbd("↵")} 打开</span><span>${kbd("⌘")}${kbd("F")} 在当前群里搜索</span></div></div>`;
  return mainWindow(mode, { topic: { ...mainTopic(mode), composer: { placeholder: "回复「夜间告警降噪：合并重复的 5xx」" } }, overlay: ov });
}
export const SWITCHER_CSS = `
.sw{left:50%;top:96px;transform:translateX(-50%);width:640px;padding:0;border-radius:12px;overflow:hidden}
.sw-q{display:flex;align-items:center;gap:12px;height:56px;padding:0 16px;border-bottom:0.5px solid var(--sep);color:var(--text-2)}
.sw-q .q{font-size:18px;line-height:24px;color:var(--text-1)}
.sw-q .caret{height:20px;margin-left:-11px}
.sw-q .grow{flex:1}
.sw-l{padding:4px 8px 8px}
.sw .ph{height:32px;padding:4px 12px 0}
.sw .pr{height:44px;gap:12px;padding:0 12px;border-radius:8px}
.sw .pr.tall{height:60px}
.sw .pr .b{font-weight:500}
.sw .pr .slot{width:24px;height:24px;flex:none;display:flex;align-items:center;justify-content:center;color:var(--text-3)}
.sw .pr.tall .slot{align-self:flex-start;margin-top:8px}
.sw .pr .g{font-size:13px;color:var(--text-3)}
.sw .pr .sn{font-size:13px;line-height:20px;color:var(--text-2)}
.sw .hl{background:var(--hl-bg);color:inherit;border-radius:4px;padding:0 2px;line-height:20px;display:inline-block}
.sw-f{display:flex;gap:20px;height:40px;align-items:center;padding:0 16px;border-top:0.5px solid var(--sep);font-size:12px;line-height:16px;color:var(--text-3)}
.sw-f span{display:inline-flex;gap:4px;align-items:center}
`;

// ---------- Settings: 成员 ----------
export function settings(mode) {
  const rows = [
    ["joye", "所有者", "在线", true],
    ["li", "成员", "在线", false],
    ["chen", "成员", "离线", false],
    ["alex", "成员", "在线", false],
    ["jojo", "所有者", "在线", false, "Mac mini"],
    ["forge", "成员", "工作中", false],
    ["watch", "成员", "等人回答", false],
    ["ledger", "成员", "离线", false],
  ];
  const dot = { 在线: "on", 工作中: "work", 等人回答: "wait", 离线: "" };
  const r = rows.map(([k, level, st, me, where]) => {
    const mm = m(k);
    return `<div class="mr"><div class="c1">${avatar({ ...mm, online: undefined, state: "idle" }, 36, mode)}<div style="min-width:0"><div class="n ell">${esc(mm.name)}${me ? '<span class="you">你</span>' : ""}</div><div class="rl">${mm.char ? icon("bot", 12) : ""}${esc(mm.role)}</div></div></div>
<div class="c2${where ? " two" : ""}"><span class="sdot ${dot[st]}"></span><div style="min-width:0"><div>${esc(st)}</div>${where ? `<div class="wh">${esc(where)}</div>` : ""}</div></div>
<div class="c3">${me ? `<span class="plain" title="你不能改自己的权限">${level}</span>` : `<span class="sel">${level}<span class="grow"></span>${icon("chevrons-up-down", 14)}</span>`}</div>
<div class="c4">${me ? "" : `<span class="ib">${icon("ellipsis", 18)}</span>`}</div></div>`;
  }).join("");
  const nav = [["user", "我的资料"], ["users", "成员", true], ["monitor", "机器"], ["bell", "通知"], ["settings", "通用"]].map(([i, t, on]) => `<div class="nv ${on ? "on" : ""}">${icon(i, 16)}${t}</div>`).join("");
  return `<div class="sw-win"><div class="lights"><span></span><span></span><span></span></div>
<aside class="snav"><div style="height:52px"></div>${nav}</aside>
<main class="sc"><div class="inner"><div class="sh"><div class="t">成员</div><span class="grow"></span><span class="btn primary md">${icon("user-plus", 16)}添加成员${icon("chevron-down", 16)}</span></div>
<div class="sfield">${icon("search", 16)}<span>搜索成员</span></div>
<div class="grp"><div class="mr hd"><div class="c1">成员 · 8</div><div class="c2">状态</div><div class="c3">权限</div><div class="c4"></div></div>${r}</div>
<div class="note">所有者可以改权限、停用成员和管理机器。</div>
</div></main></div>`;
}
export const SETTINGS_CSS = `
.sw-win{display:flex;width:var(--w);height:var(--h);background:var(--sidebar);position:relative}
.snav{width:200px;flex:none;padding:0 8px;border-right:0.5px solid var(--sep)}
.nv{display:flex;align-items:center;gap:10px;height:36px;padding:0 10px;border-radius:8px;color:var(--text-2);font-size:14px;line-height:20px}
.nv.on{background:var(--sidebar-selected);color:var(--text-1);font-weight:600}
.sc{flex:1;overflow:hidden;background:var(--content)}
.sc .inner{max-width:704px;padding:0 32px;margin:0 auto}
.sh{display:flex;align-items:center;gap:8px;padding-top:36px}
.sh .t{font-size:22px;line-height:30px;font-weight:600}
.sh .grow{flex:1}
.sfield{height:36px;border-radius:8px;background:var(--content);box-shadow:inset 0 0 0 1px var(--border);display:flex;align-items:center;gap:8px;padding:0 12px;color:var(--text-3);margin:20px 0 16px;width:320px;font-size:14px;line-height:20px}
.sfield span{color:var(--placeholder)}
.grp{border-radius:12px;box-shadow:inset 0 0 0 1px var(--border);overflow:hidden;background:var(--content)}
.mr{display:grid;grid-template-columns:1fr 152px 132px 40px;align-items:center;height:60px;padding:0 12px 0 16px;border-bottom:0.5px solid var(--sep)}
.mr:last-child{border-bottom:0}
.mr.hd{height:36px}.mr.hd>div{font-size:12px;line-height:16px;font-weight:500;color:var(--text-3)}
.mr .c1{display:flex;align-items:center;gap:12px;min-width:0}
.mr .n{font-size:14px;line-height:20px;font-weight:500}
.mr .rl{display:flex;align-items:center;gap:4px;font-size:13px;line-height:20px;color:var(--text-2)}
.mr .c2{display:flex;align-items:center;gap:8px;font-size:14px;line-height:20px;color:var(--text-2)}
.mr .c2 .wh{font-size:12px;line-height:16px;color:var(--text-3)}
.mr .c2.two{align-items:flex-start}.mr .c2.two .sdot{margin-top:6px}
.sdot{width:8px;height:8px;border-radius:50%;flex:none;box-shadow:inset 0 0 0 1.5px var(--text-3)}
.sdot.on{background:var(--online);box-shadow:none}
.sdot.work{background:var(--accent);box-shadow:none}
.sdot.wait{background:var(--you-glyph);box-shadow:none}
.sel{display:flex;align-items:center;gap:6px;width:120px;height:36px;padding:0 8px 0 12px;border-radius:8px;box-shadow:inset 0 0 0 1px var(--border);font-size:14px;line-height:20px;background:var(--content);color:var(--text-1)}
.sel .grow{flex:1}
.sel .i{color:var(--text-3)}
.plain{display:block;padding-left:12px;font-size:14px;line-height:20px;color:var(--text-2)}
.mr .n .you{margin-left:4px;font-size:12px;font-weight:400;color:var(--text-3)}
.mr .c4{display:flex;justify-content:flex-end}
.note{font-size:13px;line-height:20px;color:var(--text-3);margin-top:12px}
`;

// ---------- Login ----------
export function login(mode) {
  // The icon is drawn bare (§8.5): cropped to the squircle, its own shadow replaced by a CSS one.
  const svg = readFileSync(join(REPO, `docs/design/brand/${mode === "dark" ? "icon-dark" : "icon"}.svg`), "utf8")
    .replace(/viewBox="[^"]*"/, 'viewBox="100 100 824 824"').replace(/ filter="url\(#[^)]*\)"/g, "");
  const src = `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;
  const otp = (s) => [...s].map((c) => `<span class="ob">${c}</span>`).join("");
  return `<div class="lg"><div class="lights"><span></span><span></span><span></span></div>
<div class="col">
<img class="lgi" src="${src}" width="72" height="72" alt="">
<div class="lt">欢迎加入团队</div><div class="ls">输入管理员发给你的邀请码</div>
<div class="lab">邀请码</div>
<div class="otp">${otp("K7QM")}<span class="dash"></span>${otp("2X")}<span class="ob cur"><span class="caret"></span></span><span class="ob"></span></div>
<span class="btn primary lg dis" style="width:100%;margin-top:24px">加入</span>
<div class="lf"><span class="lk">使用其他服务器</span></div></div></div>`;
}
export const LOGIN_CSS = `
.lg{width:var(--w);height:var(--h);display:flex;align-items:center;justify-content:center;background:var(--content);position:relative}
.col{width:390px;display:flex;flex-direction:column;align-items:center;margin-top:-24px}
.lgi{display:block;filter:drop-shadow(0 4px 12px rgba(15,17,20,0.12))}
.lt{margin-top:24px;font-size:22px;line-height:30px;font-weight:600}
.ls{margin-top:6px;font-size:14px;line-height:22px;color:var(--text-2)}
.lab{align-self:flex-start;margin-top:28px;font-size:13px;line-height:20px;font-weight:500;color:var(--text-2)}
.otp{display:flex;align-items:center;gap:6px;margin-top:8px;width:100%}
.ob{width:42px;height:48px;border-radius:10px;box-shadow:inset 0 0 0 1px var(--border);display:flex;align-items:center;justify-content:center;font-family:var(--font-ui);font-size:20px;line-height:28px;font-weight:500;font-variant-numeric:tabular-nums;letter-spacing:0;color:var(--text-1);flex:none}
.ob.cur{box-shadow:inset 0 0 0 1px var(--focus-border),0 0 0 3px var(--focus-soft)}
.ob .caret{height:22px;margin:0}
.dash{width:6px;height:2px;border-radius:1px;background:var(--border-strong);margin:0;flex:none}
.lf{margin-top:12px;display:flex;align-items:center;gap:6px;font-size:13px;line-height:20px;color:var(--text-3)}
.lf .lk{color:var(--accent-text);font-weight:500}
`;
export { initials };
