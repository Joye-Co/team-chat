// Settings (成员 + invite code sheet), login, the Cmd+K switcher overlay and the app icon.
import { avatar, chip, esc, icon, kbd, trafficLights } from "./base.mjs";

export const WINDOW_CSS = `
.floatwin { position: absolute; border-radius: 16px; overflow: hidden; box-shadow: var(--shadow-window); display: flex; }
`;

export const SETTINGS_CSS = `
.set { left: 270px; top: 120px; width: 900px; height: 640px; background: var(--window-bg); }
.set .side { width: 220px; flex-shrink: 0; background: var(--sidebar-tint); display: flex; flex-direction: column; padding: 0 10px; }
.set .side .top { height: 52px; display: flex; align-items: center; padding-left: 10px; }
.set .prow { display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 8px; border-radius: 7px; }
.set .prow.sel { background: var(--accent); color: var(--on-accent); }
.set .prow .tile { width: 20px; height: 20px; border-radius: 5px; display: flex; align-items: center; justify-content: center; color: #fff; flex-shrink: 0; }
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
.pending { display: inline-flex; height: 18px; align-items: center; padding: 0 6px; border-radius: 5px; font-size: 11px; background: var(--st-other-bg); color: var(--st-other); }
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
.warn { display: flex; gap: 8px; align-items: flex-start; font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 12px; padding: 8px 10px; border-radius: 8px; background: var(--st-you-bg); }
.warn .i { color: var(--st-you); margin-top: 1px; }
`;

export function settings(mode) {
  const panes = [
    ["我的资料", "user", "#8e8e93"],
    ["成员", "users", "#007aff", true],
    ["机器", "server", "#5e5ce6"],
    ["通知", "bell", "#ff3b30"],
    ["成本", "chart-column", "#34c759", false, "即将推出"],
    ["诊断", "activity", "#ff9500"],
  ];
  const side = panes
    .map(([n, ic, c, sel, soon]) => `<div class="prow${sel ? " sel" : ""}"><span class="tile" style="background:${c}">${icon(ic, 13)}</span><span>${n}</span>${soon ? `<span class="soon">${soon}</span>` : ""}</div>`)
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
    <div class="prow"><span class="tile" style="background:#636366">${icon("code", 13)}</span><span>开发者视图</span></div>
    <div style="margin-top:auto;padding:12px 8px" class="t2 row"><span class="row" style="gap:6px">${icon("log-out", 14)}退出登录</span></div>
  </div>
  <div class="pane">
    <div class="toolbar"><span class="t">成员</span><span class="t2" style="font-size:12px">8 位</span><span class="grow"></span><span class="btn sm">${icon("bot", 13)}添加自动成员</span><span class="btn sm primary">${icon("user-plus", 13)}邀请成员</span></div>
    <div class="content">
      <div class="grp-h"><div class="field grow" style="max-width:240px">${icon("search", 14)}<span>按名字或岗位查找</span></div><span class="grow"></span><span class="t2" style="font-size:12px">权限</span></div>
      <div class="grp">${rows}</div>
    </div>
  </div>
    <div class="sheet-scrim"></div>
    <div class="sheet">
      <h3>邀请码已生成</h3>
      <p>把邀请码发给 赵一鸣。新成员在登录页输入它就能加入。</p>
      <div class="code-box"><span class="c">K7QF<i>-</i>9M2X<i>-</i>RT4P</span><span class="btn sm">${icon("copy", 13)}复制</span></div>
      <dl><dt>权限</dt><dd>普通成员</dd><dt>有效期</dt><dd>24 小时，至 <span class="num">11月4日 15:30</span></dd><dt>次数</dt><dd>只能用一次</dd></dl>
      <div class="warn">${icon("eye", 14)}<span>邀请码只显示这一次，关闭后无法再查看。</span></div>
      <div class="foot"><span class="btn">撤销邀请</span><span class="btn primary">完成</span></div>
    </div>
</div>`;
}

export const LOGIN_CSS = `
.login { left: 500px; top: 150px; width: 440px; height: 580px; background: var(--content); flex-direction: column; align-items: center; }
.login .top { align-self: stretch; height: 44px; display: flex; align-items: center; padding-left: 16px; }
.login .appicon { width: 88px; height: 88px; margin-top: 28px; }
.login h1 { font-size: 22px; line-height: 28px; font-weight: 600; margin-top: 18px; letter-spacing: -0.2px; }
.login .lead { font-size: 13px; line-height: 19px; color: var(--text-2); margin-top: 6px; text-align: center; }
.login form { width: 320px; margin-top: 28px; display: flex; flex-direction: column; gap: 14px; }
.login label { display: block; font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; }
.login .input { height: 34px; }
.login .input.code { font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: 15px; letter-spacing: 1.5px; }
.login .hint { font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 6px; }
.login .btn.lg { width: 100%; margin-top: 6px; }
.login .foot { margin-top: auto; padding-bottom: 18px; font-size: 11px; color: var(--text-3); }
`;

export function appIconSvg(size, { variant = "light", id = "a" } = {}) {
  // macOS 26 grid: an 824 px rounded-square body inside the 1024 canvas (radius 186).
  // A round bubble (a person) in front of a square bubble (a bot): equal members in one conversation.
  const dark = variant === "dark";
  const bgTop = dark ? "#38383c" : "#4ea0ff";
  const bgBot = dark ? "#1c1c1f" : "#0a66f0";
  const front = dark ? `url(#fg${id})` : "#ffffff";
  const back = dark ? "#0a84ff" : "#ffffff";
  const backOpacity = dark ? 0.55 : 0.62;
  const shadow = size >= 32;
  return `<svg width="${size}" height="${size}" viewBox="0 0 1024 1024" xmlns="http://www.w3.org/2000/svg">
<defs>
  <linearGradient id="bg${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${bgTop}"/><stop offset="1" stop-color="${bgBot}"/></linearGradient>
  <linearGradient id="fg${id}" gradientUnits="userSpaceOnUse" x1="0" y1="378" x2="0" y2="760"><stop offset="0" stop-color="#5aa9ff"/><stop offset="1" stop-color="#0a74f0"/></linearGradient>
  <linearGradient id="hi${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="${dark ? 0.10 : 0.22}"/><stop offset="0.45" stop-color="#fff" stop-opacity="0"/></linearGradient>
  <filter id="sh${id}" x="-30%" y="-30%" width="160%" height="170%"><feDropShadow dx="0" dy="14" stdDeviation="18" flood-color="${dark ? "#000" : "#003a8c"}" flood-opacity="${shadow ? (dark ? 0.5 : 0.30) : 0}"/></filter>
  <filter id="ws${id}" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="${size >= 64 ? 0.22 : 0}"/></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg${id})" filter="url(#ws${id})"/>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#hi${id})"/>
<rect x="101.5" y="101.5" width="821" height="821" rx="184.5" fill="none" stroke="#fff" stroke-opacity="${dark ? 0.14 : 0.25}" stroke-width="3"/>
<g opacity="${backOpacity}">
  <rect x="452" y="272" width="330" height="300" rx="92" fill="${back}"/>
  <path d="M690 556 L 742 630 L 640 570 Z" fill="${back}" stroke="${back}" stroke-width="20" stroke-linejoin="round"/>
</g>
<g filter="url(#sh${id})">
  <circle cx="408" cy="560" r="182" fill="${front}"/>
  <path d="M300 668 L 272 748 L 366 712 Z" fill="${front}" stroke="${front}" stroke-width="22" stroke-linejoin="round"/>
</g>
</svg>`;
}

export function login(mode) {
  return `<div class="desk"></div>
<div class="floatwin login">
  <div class="top">${trafficLights()}</div>
  <div class="appicon">${appIconSvg(88, { variant: mode, id: "l" })}</div>
  <h1>欢迎使用 TeamChat</h1>
  <div class="lead">工作在话题里推进，成员一起完成。</div>
  <form>
    <div><label>服务器地址</label><div class="input">https://team.example.com</div></div>
    <div><label>邀请码或登录凭证</label><div class="input code focus">K7QF-9M2X-RT<span class="caret" style="height:18px"></span></div><div class="hint">向管理员要一个邀请码，邀请码只能用一次</div></div>
    <span class="btn primary lg">登录</span>
  </form>
  <div class="foot num">版本 0.4.2</div>
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
.sw-item.active { background: var(--accent); color: var(--on-accent); }
.sw-item.active .where, .sw-item.active .t2 { color: rgba(255,255,255,0.8); }
.sw-item.active .chip { background: rgba(255,255,255,0.22); color: #fff; }
.sw-item.active .chip .wdot { background: #fff; }
.sw-item .ticon { width: 24px; height: 24px; border-radius: 6px; display: flex; align-items: center; justify-content: center; color: var(--text-2); background: var(--fill-field); flex-shrink: 0; }
.sw-item.active .ticon { background: rgba(255,255,255,0.2); color: #fff; }
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
.half { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 36px; }
.half.l { background: #f2f2f4; color: rgba(0,0,0,0.55); }
.half.d { background: #1c1c1e; color: rgba(255,255,255,0.55); }
.big { width: 512px; height: 512px; }
.sizes { display: flex; align-items: flex-end; gap: 40px; }
.sizes figure { display: flex; flex-direction: column; align-items: center; gap: 10px; font-size: 11px; }
`;

export function iconSheet() {
  const sizes = (variant, p) =>
    [64, 32, 16].map((s) => `<figure>${appIconSvg(s, { variant, id: `${p}${s}` })}<figcaption class="num">${s} px</figcaption></figure>`).join("");
  return `<div class="sheet-icon">
  <div class="half l"><div class="big">${appIconSvg(512, { variant: "light", id: "bl" })}</div><div class="sizes">${sizes("light", "l")}</div><div style="font-size:12px">默认 · 浅色桌面</div></div>
  <div class="half d"><div class="big">${appIconSvg(512, { variant: "dark", id: "bd" })}</div><div class="sizes">${sizes("dark", "d")}</div><div style="font-size:12px">深色外观 · 深色桌面</div></div>
</div>`;
}
