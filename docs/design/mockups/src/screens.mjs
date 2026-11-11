// Settings (成员 + invite code sheet), login, the Cmd+K switcher overlay and the app icon.
import { avatar, chip, esc, icon, kbd, trafficLights } from "./base.mjs";

export const WINDOW_CSS = `
.floatwin { position: absolute; border-radius: 16px; overflow: hidden; box-shadow: var(--shadow-window); display: flex; }
`;

export const SETTINGS_CSS = `
.set { left: 270px; top: 120px; width: 900px; height: 640px; background: var(--window-bg); }
.set .side { width: 220px; flex-shrink: 0; background: var(--sidebar); display: flex; flex-direction: column; padding: 0 10px; }
.set .side .top { height: 52px; display: flex; align-items: center; padding-left: 10px; }
.set .prow { display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 8px; border-radius: 7px; }
.set .prow.sel { background: var(--sidebar-selected); }
.set .prow .tile { width: 20px; height: 20px; display: flex; align-items: center; justify-content: center; color: var(--text-2); flex-shrink: 0; }
.set .prow .soon { margin-left: auto; font-size: 11px; color: var(--text-3); }
.set .sep { height: 0.5px; background: var(--sep); margin: 8px 8px; }
.set .pane { flex: 1; min-width: 0; display: flex; flex-direction: column; position: relative; box-shadow: inset 0.5px 0 0 var(--sep); }
.set .pane .toolbar { height: 52px; display: flex; align-items: center; padding: 0 20px; gap: 8px; }
.set .pane .toolbar .t { font-size: 15px; font-weight: 600; }
.set .content { padding: 4px 20px 20px; overflow: hidden; flex: 1; }
.grp { background: var(--group); border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border); }
.grp-h { display: flex; align-items: center; gap: 8px; padding: 0 4px 8px; }
.grp-h .lab { font-size: 13px; font-weight: 600; }
.mrow { display: flex; align-items: center; gap: 10px; height: 52px; padding: 0 12px; position: relative; }
.mrow + .mrow::before { content: ""; position: absolute; left: 50px; right: 0; top: 0; height: 0.5px; background: var(--sep); }
.mrow .nm { font-weight: 500; }
.mrow .rl { font-size: 12px; line-height: 16px; color: var(--text-2); }
.mrow .lv { display: inline-flex; align-items: center; gap: 4px; height: 22px; padding: 0 4px 0 8px; border-radius: 6px; font-size: 12px; color: var(--text-1); background: var(--control); box-shadow: var(--shadow-control); width: 92px; justify-content: space-between; }
.mrow .st { width: 92px; font-size: 12px; color: var(--text-2); display: flex; align-items: center; gap: 5px; }
.mrow .st .on { width: 7px; height: 7px; border-radius: 50%; background: var(--online); }
.mrow.off { opacity: 0.5; }
.you { font-size: 11px; color: var(--text-2); font-weight: 400; margin-left: 4px; }
.pending { display: inline-flex; height: 18px; align-items: center; padding: 0 6px; border-radius: 5px; font-size: 11px; background: transparent; box-shadow: inset 0 0 0 0.5px var(--chip-ring); color: var(--st-quiet); }
.sheet-scrim { position: absolute; inset: 0; background: var(--scrim); }
.sheet { position: absolute; left: 50%; top: 52px; transform: translateX(-50%); width: 420px; background: var(--elevated); border-radius: 14px; box-shadow: var(--shadow-dialog); padding: 20px 20px 16px; }
.sheet h3 { font-size: 15px; line-height: 20px; font-weight: 600; }
.sheet p { font-size: 13px; line-height: 19px; color: var(--text-2); margin-top: 6px; }
.code-box { margin: 16px 0 12px; height: 64px; border-radius: 10px; background: var(--fill-field); display: flex; align-items: center; justify-content: center; gap: 12px; position: relative; }
.code-box .c { font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: 24px; font-weight: 500; letter-spacing: 2px; font-variant-numeric: tabular-nums; }
.code-box .c i { font-style: normal; color: var(--text-3); padding: 0 2px; }
.sheet dl { display: grid; grid-template-columns: 64px 1fr; gap: 6px; font-size: 13px; }
.sheet dt { color: var(--text-2); }
.sheet .foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 18px; }
.warn { display: flex; gap: 8px; align-items: flex-start; font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 12px; padding: 8px 10px; border-radius: 8px; background: var(--fill-field); }
.warn .i { color: var(--glyph-wait); margin-top: 1px; }
`;

export function settings(mode) {
  const panes = [
    ["我的资料", "user"],
    ["成员", "users", null, true],
    ["机器", "server"],
    ["通知", "bell"],
    ["成本", "chart-column", null, false, "即将推出"],
    ["诊断", "activity"],
  ];
  const side = panes
    .map(([n, ic, c, sel, soon]) => `<div class="prow${sel ? " sel" : ""}"><span class="tile">${icon(ic, 16)}</span><span>${n}</span>${soon ? `<span class="soon">${soon}</span>` : ""}</div>`)
    .join("");
  const members = [
    { name: "Joye", role: "产品负责人", lv: "管理员", st: "online", you: true, palette: "graphite" },
    { name: "林晓", role: "前端", lv: "普通成员", st: "online", palette: "rose" },
    { name: "陈思远", role: "后端", lv: "普通成员", st: "online", palette: "sky" },
    { name: "王磊", role: "iOS", lv: "普通成员", st: "2 小时前", palette: "clay" },
    { name: "设计助手", role: "界面与交互设计 · 运行在 Mac mini（工作室）", lv: "管理员", st: "online", bot: true, palette: "teal" },
    { name: "构建助手", role: "构建与发布 · 运行在 Mac mini（工作室）", lv: "管理员", st: "online", bot: true, palette: "sage" },
    { name: "周报助手", role: "整理周报 · 已停用", lv: "普通成员", st: "已停用", bot: true, off: true, palette: "graphite" },
    { name: "赵一鸣", role: "运营 · 已发邀请码", lv: "普通成员", st: "pending", palette: "sand" },
  ];
  const rows = members
    .map(
      (p) => `<div class="mrow${p.off ? " off" : ""}">${avatar(p.name, { size: 28, bot: p.bot, palette: p.palette, mode, ring: "var(--group)" })}<div class="grow" style="min-width:0"><div class="nm ell">${esc(p.name)}${p.you ? `<span class="you">（你）</span>` : ""}</div><div class="rl ell">${esc(p.role)}</div></div>
<span class="st">${p.st === "online" ? `<span class="on"></span>在线` : p.st === "pending" ? `<span class="pending">等待加入</span>` : esc(p.st)}</span>
<span class="lv">${p.lv}${icon("chevrons-up-down", 12)}</span><span class="iconbtn" style="width:24px;height:24px">${icon("ellipsis", 15)}</span></div>`,
    )
    .join("");
  return `<div class="desk"></div>
<div class="floatwin set">
  <div class="side">
    <div class="top">${trafficLights()}</div>
    <div class="field" style="margin:0 0 10px">${icon("search", 14)}<span>搜索</span></div>
    ${side}
    <div class="sep"></div>
    <div class="prow"><span class="tile">${icon("code", 16)}</span><span>开发者视图</span></div>
    <div style="margin-top:auto;padding:12px 8px" class="t2 row"><span class="row" style="gap:6px">${icon("log-out", 14)}退出登录</span></div>
  </div>
  <div class="pane">
    <div class="toolbar"><span class="t">成员</span><span class="t2" style="font-size:12px">8 位</span><span class="grow"></span><span class="btn sm">${icon("bot", 13)}添加自动成员</span><span class="btn sm primary">${icon("user-plus", 13)}邀请成员</span></div>
    <div class="content">
      <div class="grp-h"><div class="field grow" style="max-width:240px">${icon("search", 14)}<span>按名字或岗位查找</span></div><span class="grow"></span><span class="t2" style="font-size:12px">权限</span></div>
      <div class="grp">${rows}</div>
    </div>
  </div>
</div>`;
}

/** The one-time invite code sheet (Dialog, FadeSlide). Shown in the states sheet. */
export function inviteSheet() {
  return `<div class="sheet" style="position:relative;left:auto;top:auto;transform:none">
      <h3>邀请码已生成</h3>
      <p>把邀请码发给 赵一鸣。新成员在登录页输入它就能加入。</p>
      <div class="code-box"><span class="c">K7QF<i>-</i>9M2X<i>-</i>RT4P</span><span class="btn sm">${icon("copy", 13)}复制<span class="kbd" style="margin-left:2px">⌘C</span></span></div>
      <dl><dt>权限</dt><dd>普通成员</dd><dt>有效期</dt><dd>24 小时，至 <span class="num">11月4日 15:30</span></dd><dt>次数</dt><dd>只能用一次</dd></dl>
      <div class="warn">${icon("eye", 14)}<span>邀请码只显示这一次，关闭后无法再查看。</span></div>
      <div class="foot"><span class="btn">撤销邀请</span><span class="btn primary">完成</span></div>
    </div>`;
}

export const LOGIN_CSS = `
.login { left: 500px; top: 150px; width: 440px; height: 580px; background: var(--content); flex-direction: column; align-items: center; }
.login .top { align-self: stretch; height: 44px; display: flex; align-items: center; padding-left: 16px; }
.login .appicon { width: 88px; height: 88px; margin-top: 28px; }
.login h1 { font-size: 22px; line-height: 28px; font-weight: 600; margin-top: 18px; letter-spacing: -0.2px; }
.login .lead { font-size: 13px; line-height: 19px; color: var(--text-2); margin-top: 6px; text-align: center; }
.login form { width: 344px; margin-top: 28px; display: flex; flex-direction: column; gap: 14px; }
.login label { display: block; font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; }
.login .input { height: 34px; }
.otp-row { display: flex; align-items: center; gap: 6px; }
.otp-g { display: flex; gap: 4px; }
.otp-dash { color: var(--text-3); font-size: 15px; width: 8px; text-align: center; }
.otp { width: 22px; height: 36px; border-radius: 7px; background: var(--control); box-shadow: var(--shadow-control); display: flex; align-items: center; justify-content: center; font-family: Menlo, ui-monospace, monospace; font-size: 16px; font-weight: 500; }
.otp.cur { box-shadow: 0 0 0 0.5px var(--accent), 0 0 0 3.5px var(--focus-ring); }
.ok-dot { color: var(--success); display: flex; }
.login .hint { font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 6px; }
.login .btn.lg { width: 100%; margin-top: 6px; }
.login .foot { margin-top: auto; padding-bottom: 18px; font-size: 11px; color: var(--text-3); }
`;

/**
 * App icon. macOS grid: an 824 px rounded-square body inside the 1024 canvas (radius 186).
 * Jojo, the lead character of the default bot family, calm, on the family's cream. The drawing is
 * jojo-friends' own (renderSvg, character jojo, emotion calm: src/library/geometry.ts); never
 * redraw it by hand. The icon is the same on light and dark surfaces (`variant` only picks the edge).
 * Below 64 px Jojo is larger and the fills are flat (optical sizing for 16/32 px).
 */
const JOJO_SHELL = "M260,380 C260,312 310,262 365,262 C396,262 418,288 422,318 A80,80 0 0 0 582,318 C590,284 622,258 670,258 L875,258 A120,120 0 0 1 995,378 L995,838 A120,120 0 0 1 875,958 L380,958 A120,120 0 0 1 260,838 Z";
const JOJO_FACE = "M418,582 A130,130 0 0 1 548,452 L925,452 A70,70 0 0 1 995,522 L995,838 A120,120 0 0 1 875,958 L498,958 A80,80 0 0 1 418,878 Z";
export function appIconSvg(size, { variant = "light", id = "a" } = {}) {
  const dark = variant === "dark";
  const small = size < 64;
  const shadow = size >= 64;
  const s = small ? 0.92 : 0.86;
  const at = `translate(${+(512 - 627.5 * s).toFixed(2)} ${+(522 - 608 * s).toFixed(2)}) scale(${s})`;
  return `<svg width="${size}" height="${size}" viewBox="0 0 1024 1024" xmlns="http://www.w3.org/2000/svg">
<defs>
  <linearGradient id="bg${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fbf6ec"/><stop offset="1" stop-color="#f1e7d4"/></linearGradient>
  <linearGradient id="js${id}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#32377b"/><stop offset="1" stop-color="#292d6b"/></linearGradient>
  <linearGradient id="jf${id}" x1="0" y1="0" x2="0.35" y2="1"><stop offset="0" stop-color="#d6f3ea"/><stop offset="1" stop-color="#c9ede1"/></linearGradient>
  <radialGradient id="jd${id}" cx="0.36" cy="0.32" r="0.75"><stop offset="0" stop-color="#ff8a6c"/><stop offset="0.55" stop-color="#fe6a4a"/></radialGradient>
  <filter id="ws${id}" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="${shadow ? (dark ? 0.45 : 0.22) : 0}"/></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg${id})" filter="url(#ws${id})"/>
<rect x="100.75" y="100.75" width="822.5" height="822.5" rx="185.25" fill="none" stroke="#5e4828" stroke-opacity="${small ? 0.16 : 0.12}" stroke-width="${small ? 10 : 1.5}"/>
<g transform="${at}">
  <path d="${JOJO_SHELL}" fill="${small ? "#2d3272" : `url(#js${id})`}"/>
  <path d="${JOJO_FACE}" fill="${small ? "#cff0e6" : `url(#jf${id})`}"/>
  <rect x="555" y="592" width="68" height="196" rx="34" fill="#2d3272"/>
  <rect x="747" y="572.5" width="110" height="215" rx="55" fill="#2d3272"/>
  <rect x="723" y="823" width="162" height="47" rx="23.5" fill="#2d3272"/>
  <circle cx="501" cy="297" r="53" fill="${small ? "#fe6a4a" : `url(#jd${id})`}"/>
</g>
</svg>`;
}

export function login(mode) {
  const chars = "K7QF9M2XRT".split("");
  const boxes = [0, 1, 2].map((g) =>
    `<span class="otp-g">${[0, 1, 2, 3].map((i) => { const n = g * 4 + i; const c = chars[n]; return `<span class="otp${n === chars.length ? " cur" : ""}">${c ?? (n === chars.length ? `<span class="caret" style="height:18px;margin:0"></span>` : "")}</span>`; }).join("")}</span>`).join(`<span class="otp-dash">-</span>`);
  return `<div class="desk"></div>
<div class="floatwin login">
  <div class="top">${trafficLights()}</div>
  <div class="appicon">${appIconSvg(88, { variant: mode, id: "l" })}</div>
  <h1>欢迎使用 TeamChat</h1>
  <div class="lead">工作在话题里推进，成员一起完成。</div>
  <form>
    <div><label>服务器</label><div class="input">team.example.com<span class="grow"></span><span class="ok-dot">${icon("circle-check", 14)}</span></div></div>
    <div><label>邀请码</label><div class="otp-row">${boxes}</div><div class="hint">向管理员要一个邀请码。粘贴带横线或空格的格式也可以。</div></div>
    <span class="btn primary lg disabled">登录<span class="kbd" style="background:transparent;color:inherit;margin-left:2px">↵</span></span>
  </form>
  <div class="foot num">版本 0.4.2 · 登录后凭证保存在钥匙串里</div>
</div>`;
}

export const SWITCHER_CSS = `
.sw-scrim { position: absolute; inset: 0; background: rgba(0,0,0,0.0); }
.switcher { position: absolute; left: 50%; top: 132px; transform: translateX(-50%); width: 640px; border-radius: 14px; background: var(--elevated); box-shadow: var(--shadow-dialog); overflow: hidden; }
.sw-in { height: 52px; display: flex; align-items: center; gap: 10px; padding: 0 16px; font-size: 18px; box-shadow: inset 0 -0.5px 0 var(--sep); }
.sw-in .i { color: var(--text-2); }
.sw-in .q { flex: 1; }
.sw-list { padding: 6px; }
.sw-h { font-size: 11px; font-weight: 600; color: var(--text-3); padding: 8px 10px 4px; }
.sw-item { display: flex; align-items: center; gap: 10px; height: 40px; padding: 0 10px; border-radius: 8px; }
.sw-item .where { font-size: 12px; color: var(--text-2); }
.sw-item.active { background: var(--fill-active); }
.sw-item .ticon { width: 24px; height: 24px; border-radius: 6px; display: flex; align-items: center; justify-content: center; color: var(--text-2); background: var(--fill-field); flex-shrink: 0; }
.hl { font-weight: 600; }
.sw-foot { height: 36px; display: flex; align-items: center; gap: 14px; padding: 0 16px; font-size: 11px; color: var(--text-2); box-shadow: inset 0 0.5px 0 var(--sep); }
.sw-foot .row { gap: 5px; }
`;

export function switcherOverlay(mode) {
  const topic = (title, where, st, opts, active) =>
    `<div class="sw-item${active ? " active" : ""}"><span class="ticon">${icon("message-square-plus", 14)}</span><span class="ell">${title}</span>${chip(st, opts)}<span class="where ell grow">${where}</span>${active ? kbd("↵") : ""}</div>`;
  return `<div class="switcher">
  <div class="sw-in">${icon("search", 18)}<span class="q">登录<span class="caret" style="height:22px;vertical-align:-4px"></span></span>${kbd("esc")}</div>
  <div class="sw-list">
    <div class="sw-h">话题</div>
    ${topic(`<span class="hl">登录</span>页改版`, "开发 · 14:32", "waiting_review", { you: true }, true)}
    ${topic(`<span class="hl">登录</span>失败时的报错文案`, "产品 · 昨天", "done", {}, false)}
    ${topic(`统一<span class="hl">登录</span>与账号体系`, "全员 · 10月28日", "waiting_answer", { name: "王磊" }, false)}
    <div class="sw-h">群和成员</div>
    <div class="sw-item"><span class="ticon" style="background:none">${avatar("构建助手", { size: 24, bot: true, palette: "sage", mode, ring: "var(--elevated)" })}</span><span>构建助手</span><span class="where ell grow">构建与发布 · 负责「<span class="hl">登录</span>页改版」的预览</span></div>
    <div class="sw-h">消息</div>
    <div class="sw-item"><span class="ticon">${icon("text-quote", 14)}</span><span class="ell grow">陈思远：<span class="hl">登录</span>凭证存在钥匙串里，不写进配置文件</span><span class="where">开发 · 昨天</span></div>
  </div>
  <div class="sw-foot"><span class="row">${kbd("↑")}${kbd("↓")} 选择</span><span class="row">${kbd("↵")} 打开</span><span class="row">${kbd("⌘")}${kbd("↵")} 在新窗口打开</span></div>
</div>`;
}

export const ICON_CSS = `
.sheet-icon { position: absolute; inset: 0; display: grid; grid-template-columns: 1fr 1fr; }
.half { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 32px; }
.half.l { background: #f2f2f4; color: rgba(0,0,0,0.55); }
.half.d { background: #1c1c1e; color: rgba(235,235,245,0.6); }
.big { width: 440px; height: 440px; }
.sizes { display: flex; align-items: flex-end; gap: 36px; }
.sizes figure { display: flex; flex-direction: column; align-items: center; gap: 10px; font-size: 11px; }
.dock { display: flex; gap: 10px; align-items: center; padding: 8px 12px; border-radius: 18px; }
.half.l .dock { background: rgba(255,255,255,0.7); box-shadow: 0 0 0 0.5px rgba(0,0,0,0.1); }
.half.d .dock { background: rgba(255,255,255,0.08); box-shadow: 0 0 0 0.5px rgba(255,255,255,0.12); }
.ph-icon { width: 48px; height: 48px; border-radius: 11px; }
`;

export function iconSheet() {
  const sizes = (variant, p) =>
    [128, 64, 32, 16].map((s) => `<figure>${appIconSvg(s, { variant, id: `${p}${s}` })}<figcaption class="num">${s} px</figcaption></figure>`).join("");
  const dock = (variant, p) => {
    const others = variant === "dark" ? ["#3a3a3e", "#2f6fd6", "#3b3b40"] : ["#e9e9ee", "#3a86ff", "#d9d9df"];
    return `<div class="dock"><span class="ph-icon" style="background:${others[0]}"></span>${appIconSvg(56, { variant, id: `${p}dk` })}<span class="ph-icon" style="background:${others[1]}"></span><span class="ph-icon" style="background:${others[2]}"></span></div>`;
  };
  return `<div class="sheet-icon">
  <div class="half l"><div class="big">${appIconSvg(440, { variant: "light", id: "bl" })}</div><div class="sizes">${sizes("light", "l")}</div>${dock("light", "l")}<div style="font-size:12px">默认 · 浅色外观</div></div>
  <div class="half d" data-theme="dark"><div class="big">${appIconSvg(440, { variant: "dark", id: "bd" })}</div><div class="sizes">${sizes("dark", "d")}</div>${dock("dark", "d")}<div style="font-size:12px">深色外观</div></div>
</div>`;
}
