// The main window: sidebar, topic list, topic view, side panel.
import { avatar, badge, chip, esc, groupTile, icon, kbd, trafficLights } from "./base.mjs";

export const MAIN_CSS = `
.win { position: absolute; inset: 0; display: flex; overflow: hidden; }

/* Sidebar: window-level vibrancy (NSVisualEffectView behind the whole window) + a tint */
.sidebar { width: 236px; flex-shrink: 0; position: relative; display: flex; flex-direction: column; background: var(--sidebar-tint); }
.sb-top { height: 52px; display: flex; align-items: center; padding: 0 12px 0 20px; flex-shrink: 0; }
.sb-top .iconbtn { margin-left: auto; }
.sb-search { margin: 0 10px 10px; }
.sb-search .kbd { margin-left: auto; background: transparent; color: var(--text-3); padding: 0; }
.sb-scroll { flex: 1; overflow: hidden; padding: 0 10px; }
.sb-row { display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 8px; border-radius: 7px; color: var(--text-1); }
.sb-row .label { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sb-row.unread .label { font-weight: 600; }
.sb-row.sel { background: var(--sidebar-selected); }
.sb-row.muted .label { color: var(--text-2); }
.sb-row .sicon { width: 20px; display: flex; justify-content: center; color: var(--accent); }
.sb-head { font-size: 11px; line-height: 14px; font-weight: 600; color: var(--text-3); padding: 16px 8px 6px; display: flex; align-items: center; }
.sb-head .iconbtn { width: 20px; height: 20px; margin-left: auto; color: var(--text-3); }
.sb-foot { height: 52px; display: flex; align-items: center; gap: 8px; padding: 0 12px 0 18px; flex-shrink: 0; }
.sb-foot .who { line-height: 16px; }
.sb-foot .who b { font-weight: 500; display: block; }
.sb-foot .who span { font-size: 11px; color: var(--text-2); }
.conn { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-2); }

/* Columns */
.list { width: 300px; flex-shrink: 0; background: var(--content); display: flex; flex-direction: column; box-shadow: inset 0.5px 0 0 var(--sep); }
.view { flex: 1; min-width: 0; background: var(--content); display: flex; flex-direction: column; box-shadow: inset 0.5px 0 0 var(--sep); position: relative; }
.panel { width: 280px; flex-shrink: 0; background: var(--panel); display: flex; flex-direction: column; box-shadow: inset 0.5px 0 0 var(--sep); }
.toolbar { height: 52px; flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding: 0 12px 0 16px; position: relative; z-index: 2; }
.toolbar .title { font-size: 15px; line-height: 20px; font-weight: 600; }
.toolbar .sub { font-size: 11px; line-height: 14px; color: var(--text-2); }
.edge { position: absolute; left: 0; right: 0; top: 52px; height: 14px; background: linear-gradient(var(--content), transparent); z-index: 1; pointer-events: none; }

/* Topic list */
.list-filter { padding: 0 12px 8px; display: flex; align-items: center; gap: 8px; }
.list-filter .seg { flex: 1; } .list-filter .seg > span { flex: 1; justify-content: center; }
.topics { flex: 1; overflow: hidden; padding: 0 8px; }
.topic { position: relative; display: flex; flex-direction: column; gap: 4px; padding: 9px 10px 10px 20px; border-radius: 8px; }
.topic + .topic::before { content: ""; position: absolute; left: 20px; right: 10px; top: 0; height: 0.5px; background: var(--sep); }
.topic.sel::before, .topic.sel + .topic::before { display: none; }
.topic.sel { background: var(--fill-selected); }
.topic .l1 { display: flex; align-items: baseline; gap: 8px; }
.topic .ttl { flex: 1; min-width: 0; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.topic.unread .ttl { font-weight: 600; }
.topic time { font-size: 12px; color: var(--text-2); flex-shrink: 0; }
.topic .l2 { display: flex; align-items: center; gap: 6px; font-size: 12px; line-height: 16px; color: var(--text-2); }
.topic .l2 .pv { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.topic .udot { position: absolute; left: 6px; top: 14px; }

/* Topic view */
.view .head { display: flex; flex-direction: column; min-width: 0; flex: 1; }
.view .head .row { gap: 8px; min-width: 0; }
.stack { display: flex; }
.stack .avatar { box-shadow: 0 0 0 1.5px var(--content); }
.stack .avatar + .avatar { margin-left: -6px; }
.timeline { flex: 1; min-height: 0; overflow: hidden; display: flex; flex-direction: column; justify-content: flex-end; padding: 0 0 4px; }
.timeline.up { justify-content: flex-start; padding-top: 8px; }
.tstart { display: flex; gap: 12px; align-items: center; padding: 10px 24px 14px 20px; margin-bottom: 4px; }
.tstart .tsi { width: 32px; height: 32px; border-radius: 50%; background: var(--fill-field); color: var(--text-2); display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
.tstart b { font-size: 15px; line-height: 20px; font-weight: 600; }
.tstart .t2 { font-size: 12px; }
.jump { position: absolute; left: 50%; transform: translateX(-50%); bottom: 150px; height: 30px; padding: 0 12px; border-radius: 15px; background: var(--elevated); box-shadow: var(--shadow-popover); display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 500; z-index: 3; }
.day { align-self: center; font-size: 11px; font-weight: 500; color: var(--text-3); margin: 8px 0 4px; }
.msg { display: flex; gap: 12px; padding: 4px 24px 4px 20px; position: relative; }
.msg + .msg:not(.cont) { margin-top: 12px; }
.msg.cont { padding-top: 2px; }
.msg .gut { width: 32px; flex-shrink: 0; }
.msg .meta { display: flex; align-items: baseline; gap: 6px; margin-bottom: 2px; min-width: 0; }
.msg .meta b { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; flex-shrink: 1; }
.msg .meta .role { font-size: 12px; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; flex-shrink: 2; }
.msg .meta time { font-size: 11px; color: var(--text-3); flex-shrink: 0; }
.body { font-size: 14px; line-height: 22px; max-width: 620px; overflow-wrap: anywhere; }
.body p + p, .body p + ol, .body ol + p { margin-top: 6px; }
.body ol { padding-left: 20px; }
.body .blk { margin-top: 8px; }
.mention { color: var(--accent-text); background: var(--mention-bg); border-radius: 4px; padding: 0 2px; font-weight: 500; }
.mention.me { color: var(--mention-me-text); background: var(--mention-me-bg); }
.msg.hover { background: var(--fill-hover); }
.actions { position: absolute; right: 24px; top: -14px; height: 30px; padding: 0 2px; border-radius: 8px; background: var(--elevated); box-shadow: var(--shadow-popover); display: flex; align-items: center; }
.actions .iconbtn { width: 28px; height: 26px; border-radius: 6px; }
.ack { display: flex; align-items: center; gap: 12px; padding: 2px 24px 2px 20px; margin-top: 6px; font-size: 12.5px; color: var(--text-2); }
.ack .gut { display: flex; justify-content: center; }
.ack b { font-weight: 500; color: var(--text-1); }
.ack time { font-size: 11px; color: var(--text-3); margin-left: 2px; }
.quote { border-left: 2px solid var(--quote-bar); padding: 0 0 0 10px; margin: 2px 0 6px; font-size: 13px; line-height: 19px; color: var(--text-2); }
.quote b { font-weight: 600; font-size: 12px; }
.code { border-radius: 8px; background: var(--code-bg); overflow: hidden; max-width: 560px; }
.code .ch { display: flex; align-items: center; height: 28px; padding: 0 6px 0 12px; font-size: 11px; color: var(--text-3); }
.code .ch .iconbtn { margin-left: auto; width: 24px; height: 22px; color: var(--text-3); }
.code { position: relative; }
.code.wide::after { content: ""; position: absolute; right: 0; top: 28px; bottom: 0; width: 40px; background: linear-gradient(90deg, transparent, var(--code-bg)); }
.composer.idle { box-shadow: 0 0 0 0.5px var(--control-border), 0 1px 2px rgba(0,0,0,0.04); }
.code pre { font-size: 12.5px; line-height: 19px; padding: 0 12px 10px; color: var(--code-text); white-space: pre; overflow: hidden; }
.k { color: var(--code-keyword); } .s { color: var(--code-string); } .ty { color: var(--code-type); } .fn { color: var(--code-fn); } .cm { color: var(--code-comment); } .n { color: var(--code-number); }
.img { border-radius: 8px; overflow: hidden; box-shadow: 0 0 0 0.5px var(--image-border); display: block; }
.card { border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border); background: var(--content); max-width: 460px; }
.art { display: flex; align-items: center; gap: 12px; padding: 10px 12px; }
.art .tile { width: 40px; height: 40px; border-radius: 8px; background: var(--accent-soft); color: var(--accent); display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
.art b { font-weight: 600; display: block; }
.art .m { font-size: 12px; line-height: 16px; color: var(--text-2); }
.sc { padding: 12px 14px 12px; }
.sc .sh { display: flex; align-items: center; gap: 8px; }
.sc .sh b { font-weight: 600; flex: 1; min-width: 0; }
.sc dl { display: grid; grid-template-columns: 64px 1fr 64px 1fr; gap: 6px 10px; margin: 10px 0 12px; font-size: 13px; }
.sc dt { color: var(--text-2); }
.sc dd { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sc .acts { display: flex; gap: 8px; }
.ok { color: var(--success); display: inline-flex; vertical-align: -2px; margin-right: 3px; }
.divider { display: flex; align-items: center; gap: 8px; margin: 14px 24px 2px 20px; color: var(--new-divider); font-size: 11px; font-weight: 600; }
.divider::before { content: ""; flex: 1; height: 0.5px; background: var(--new-divider); opacity: 0.7; }
.stream-caret { display: inline-block; width: 2px; height: 16px; background: var(--accent); vertical-align: -3px; margin-left: 2px; border-radius: 1px; }
.streaming { display: flex; align-items: center; gap: 6px; margin-top: 6px; font-size: 12px; color: var(--text-3); }
.shimmer { background: linear-gradient(90deg, var(--text-3) 0%, var(--text-3) 35%, var(--text-1) 50%, var(--text-3) 65%, var(--text-3) 100%); -webkit-background-clip: text; background-clip: text; color: transparent; }
.typing { height: 22px; display: flex; align-items: center; gap: 6px; padding: 0 20px 0 24px; font-size: 12px; color: var(--text-2); flex-shrink: 0; }
.dots { display: inline-flex; gap: 3px; } .dots i { width: 4px; height: 4px; border-radius: 50%; background: var(--text-3); }
.dots i:nth-child(2) { opacity: .7; } .dots i:nth-child(3) { opacity: .4; }
.composer-wrap { padding: 0 20px 16px; flex-shrink: 0; position: relative; }
.composer { border-radius: 12px; background: var(--content); box-shadow: 0 0 0 1px var(--focus-ring), 0 1px 2px rgba(0,0,0,0.04); padding: 10px 12px 8px; }
:root[data-theme="dark"] .composer { background: rgba(255,255,255,0.04); }
.composer .txt { font-size: 14px; line-height: 22px; min-height: 22px; }
.composer .ph { color: var(--placeholder); }
.composer .bar { display: flex; align-items: center; gap: 2px; margin-top: 6px; }
.composer .bar .iconbtn { width: 28px; height: 28px; color: var(--text-2); }
.composer .hint { margin-left: auto; margin-right: 8px; font-size: 11px; color: var(--text-3); }
.send { width: 28px; height: 28px; border-radius: 50%; background: var(--accent); color: var(--on-accent); display: flex; align-items: center; justify-content: center; }
.send.off { background: var(--fill-pressed); color: var(--text-3); }
.mention-pop { width: 280px; bottom: 112px; } .mention-pop .menu-item { height: 32px; } .hlm { text-decoration: none; font-weight: 700; }
.offline-bar { margin: 0 20px 8px; height: 30px; border-radius: 8px; background: var(--fill-field); display: flex; align-items: center; gap: 8px; padding: 0 10px; font-size: 12px; color: var(--text-2); }

/* Side panel */
.panel .toolbar { justify-content: center; padding: 0 12px; }
.panel .seg { width: 100%; } .panel .seg > span { flex: 1; justify-content: center; }
.psec { padding: 4px 12px 0; }
.psec h4 { font-size: 11px; line-height: 14px; font-weight: 600; color: var(--text-3); padding: 12px 6px 6px; }
.pitem { display: flex; align-items: center; gap: 10px; padding: 7px 6px; border-radius: 7px; }
.pitem .tile { width: 32px; height: 32px; border-radius: 7px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: var(--fill-field); color: var(--text-2); }
.pitem .tile.html { background: var(--accent-soft); color: var(--accent); }
.pitem b { font-weight: 500; display: block; }
.pitem .m { font-size: 11px; line-height: 14px; color: var(--text-2); }
.decision { margin: 0 6px; padding: 10px 12px; border-radius: 8px; background: var(--content); box-shadow: 0 0 0 0.5px var(--card-border); font-size: 13px; line-height: 19px; }
.decision .m { font-size: 11px; color: var(--text-2); margin-top: 4px; }
.kv { display: grid; grid-template-columns: 56px 1fr; gap: 8px 8px; padding: 0 6px; font-size: 13px; align-items: center; }
.kv dt { color: var(--text-2); font-size: 12px; }
.kv dd { display: flex; align-items: center; gap: 6px; min-width: 0; }
.steps { padding: 0 6px; }
.step { display: flex; gap: 10px; position: relative; padding-bottom: 14px; }
.step .ic { width: 16px; height: 16px; flex-shrink: 0; margin-top: 1px; color: var(--success); position: relative; z-index: 1; background: var(--panel); }
.step.run .ic { color: var(--accent); } .step.wait .ic { color: var(--text-3); }
.step:not(:last-child)::after { content: ""; position: absolute; left: 7.5px; top: 20px; bottom: 2px; width: 1px; background: var(--sep-strong); }
.step b { font-weight: 500; display: block; font-size: 13px; }
.step .m { font-size: 11px; line-height: 14px; color: var(--text-2); }
.bar-track { height: 4px; border-radius: 2px; background: var(--fill-pressed); margin-top: 6px; overflow: hidden; width: 180px; }
.bar-fill { height: 100%; border-radius: 2px; background: var(--accent); }
.runbox { margin: 4px 6px 0; padding: 10px 12px; border-radius: 8px; background: var(--fill-field); font-size: 12px; color: var(--text-2); display: flex; align-items: center; gap: 8px; }
`;

function sidebar(d, mode) {
  const ring = "var(--sidebar-ring, #e9e9ee)";
  const rows = d.groups
    .map((g) => {
      const lead = g.dm
        ? avatar(g.name, { size: 20, bot: g.bot, presence: g.presence, palette: g.palette, mode, ring: mode === "dark" ? "#2b2b2f" : "#e7e7ec" })
        : groupTile(g.name, g.palette, mode);
      const tail = g.mention ? badge(g.mention, { mention: true, max: 99 }) : g.unread ? badge(g.unread, { max: 999 }) : g.mutedIcon ? `<span class="t3">${icon("bell-off", 13)}</span>` : "";
      return `<div class="sb-row${g.sel ? " sel" : ""}${g.unread || g.mention ? " unread" : ""}${g.muted ? " muted" : ""}">${lead}<span class="label">${esc(g.name)}</span>${tail}</div>`;
    })
    .join("");
  const foot = d.offline
    ? `<div class="conn grow">${icon("wifi-off", 14)}<span class="ell">离线，正在重新连接…</span></div>`
    : `${avatar(d.me.name, { size: 26, presence: "online", palette: d.me.palette, mode, ring: mode === "dark" ? "#2b2b2f" : "#e7e7ec" })}<div class="who grow ell"><b class="ell">${esc(d.me.name)}</b><span>${esc(d.me.role)}</span></div>`;
  return `<aside class="sidebar">
  <div class="sb-top">${trafficLights()}<span class="iconbtn">${icon("panel-left", 16)}</span></div>
  <div class="field sb-search">${icon("search", 14)}<span>搜索</span><span class="kbd">⌘K</span></div>
  <div class="sb-scroll">
    <div class="sb-row${d.todoSel ? " sel" : ""} unread"><span class="sicon">${icon("inbox", 16)}</span><span class="label">待办</span>${badge(d.todo, { mention: true, max: 99 }).replace("@", "")}</div>
    <div class="sb-head">群<span class="iconbtn">${icon("plus", 14)}</span></div>
    ${rows}
  </div>
  <div class="sb-foot">${foot}<span class="iconbtn">${icon("settings", 16)}</span></div>
</aside>`;
}

function topicList(d) {
  const topics = d.topics
    .map(
      (t) => `<div class="topic${t.sel ? " sel" : ""}${t.unread ? " unread" : ""}">
  ${t.unread ? `<span class="udot"></span>` : ""}
  <div class="l1"><span class="ttl">${esc(t.title)}</span><time>${esc(t.time)}</time></div>
  <div class="l2">${chip(t.status, t.chip || {})}<span class="pv">${t.preview}</span>${t.count ? badge(t.count, { max: 999 }) : ""}${t.mention ? badge(t.mention, { mention: true, max: 99 }) : ""}</div>
</div>`,
    )
    .join("");
  return `<section class="list">
  <div class="toolbar"><div class="col grow" style="min-width:0"><span class="title ell">${esc(d.group.name)}</span><span class="sub ell">${esc(d.group.sub)}</span></div><span class="iconbtn" title="新话题">${icon("square-pen", 17)}</span></div>
  <div class="list-filter"><div class="seg"><span class="on">全部</span><span>未读</span><span>待办</span></div></div>
  <div class="topics">${topics}</div>
</section>`;
}

export function message(m, mode) {
  if (m.kind === "day") return `<div class="day">${esc(m.text)}</div>`;
  if (m.kind === "start") return `<div class="tstart"><span class="tsi">${icon("message-square-plus", 18)}</span><div><b>登录页改版</b><div class="t2">${esc(m.text)}</div></div></div>`;
  if (m.kind === "divider") return `<div class="divider">新消息</div>`;
  if (m.kind === "ack")
    return `<div class="ack"><span class="gut">${avatar(m.author, { size: 20, bot: true, palette: m.palette, mode })}</span><span class="ell"><b>${esc(m.author)}</b> ${m.text}</span><time>${esc(m.time)}</time></div>`;
  const head = m.cont
    ? `<span class="gut"></span>`
    : `<span class="gut">${avatar(m.author, { size: 32, bot: m.bot, palette: m.palette, mode, presence: m.presence })}</span>`;
  const meta = m.cont
    ? ""
    : `<div class="meta"><b>${esc(m.author)}</b><span class="role">${esc(m.role || "")}</span><time>${esc(m.time)}</time></div>`;
  const actions = m.hover
    ? `<div class="actions"><span class="iconbtn">${icon("text-quote", 15)}</span><span class="iconbtn">${icon("at-sign", 15)}</span><span class="iconbtn">${icon("copy", 15)}</span><span class="iconbtn">${icon("ellipsis", 15)}</span></div>`
    : "";
  return `<div class="msg${m.cont ? " cont" : ""}${m.hover ? " hover" : ""}">${head}<div class="grow">${meta}<div class="body">${m.html}</div></div>${actions}</div>`;
}

function topicView(d, mode, { mentionPopup = true } = {}) {
  const t = d.topic;
  const people = t.people.map((p) => avatar(p.name, { size: 22, bot: p.bot, palette: p.palette, mode })).join("");
  const msgs = d.messages.map((m) => message(d.noHover ? { ...m, hover: false } : m, mode)).join("\n");
  const pop = mentionPopup && d.mentionList ? mentionPop(d.mentionList, mode) : "";
  return `<main class="view">
  <div class="toolbar">
    <div class="head"><div class="row"><span class="title ell">${esc(t.title)}</span>${chip(t.status, t.chip || {})}</div><span class="sub ell">${esc(t.sub)}</span></div>
    <div class="stack">${people}</div>
    <span class="capsule"><span class="iconbtn">${icon("bell", 16)}</span><span class="iconbtn" style="color:var(--accent)">${icon("panel-right", 16)}</span><span class="iconbtn">${icon("ellipsis", 16)}</span></span>
  </div>
  <div class="edge"></div>
  <div class="timeline${d.scrolledUp ? " up" : ""}">${msgs}</div>
  ${d.scrolledUp ? `<div class="jump"><span class="udot"></span>3 条新消息${icon("arrow-down", 14)}</div>` : ""}
  ${d.offline ? `<div class="offline-bar">${icon("wifi-off", 14)}<span>网络已断开。新消息会在连上后发出。</span></div>` : ""}
  <div class="typing">${d.typing ? `<span class="dots"><i></i><i></i><i></i></span><span class="ell">${d.typing}</span>` : ""}</div>
  <div class="composer-wrap">
    ${pop}
    <div class="composer${d.composerIdle ? " idle" : ""}">
      <div class="txt">${d.draft}</div>
      <div class="bar"><span class="iconbtn">${icon("paperclip", 16)}</span><span class="iconbtn">${icon("image", 16)}</span><span class="iconbtn">${icon("at-sign", 16)}</span><span class="iconbtn">${icon("text-quote", 16)}</span><span class="hint">↵ 发送　⇧↵ 换行</span><span class="send${d.draftEmpty ? " off" : ""}">${icon("arrow-up", 16)}</span></div>
    </div>
  </div>
</main>`;
}

function mentionPop(list, mode) {
  const items = list
    .map(
      (p, i) => `<div class="menu-item${i === 0 ? " active" : ""}">${avatar(p.name, { size: 24, bot: p.bot, palette: p.palette, mode, presence: p.presence, ring: i === 0 ? "var(--accent)" : "var(--elevated)" })}<span class="ell" style="font-weight:500">${p.hl ? esc(p.name).replace(p.hl, `<u class="hlm">${p.hl}</u>`) : esc(p.name)}</span><span class="t2 ell grow" style="font-size:12px">${esc(p.role)}</span>${i === 0 ? kbd("↵") : ""}</div>`,
    )
    .join("");
  return `<div class="popover mention-pop" style="left:${list.left ?? 196}px"><div class="menu-head">成员</div>${items}</div>`;
}

function panel(d, mode, tab) {
  if (tab === "process") {
    const steps = d.process
      .map(
        (s) => `<div class="step ${s.state}"><span class="ic">${icon(s.state === "done" ? "circle-check" : s.state === "run" ? "loader" : "circle-dashed", 16)}</span><div class="grow"><b>${esc(s.title)}</b><div class="m">${esc(s.meta)}</div>${s.progress ? `<div class="bar-track"><div class="bar-fill" style="width:${s.progress}%"></div></div>` : ""}</div></div>`,
      )
      .join("");
    return `<aside class="panel">
  <div class="toolbar"><div class="seg"><span>产出 <span class="num t3">3</span></span><span class="on">过程</span></div></div>
  <div class="psec"><div class="row" style="gap:10px;padding:8px 6px 4px">${avatar("设计助手", { size: 28, bot: true, palette: "teal", mode, ring: "var(--panel)" })}<div class="col grow"><b style="font-weight:600">设计助手</b><span class="t2" style="font-size:11px;line-height:14px">正在工作 · 已用 18 分钟</span></div></div></div>
  <div class="psec"><h4>步骤</h4><div class="steps">${steps}</div></div>
  <div class="psec"><div class="runbox">${icon("monitor", 14)}<span class="grow ell">运行在 Mac mini（工作室）</span><span class="num">18:04</span></div></div>
</aside>`;
  }
  const items = d.artifacts
    .map(
      (a) => `<div class="pitem"><span class="tile ${a.kind}">${icon(a.icon, 16)}</span><div class="grow" style="min-width:0"><b class="ell">${esc(a.name)}</b><div class="m ell">${esc(a.meta)}</div></div></div>`,
    )
    .join("");
  return `<aside class="panel">
  <div class="toolbar"><div class="seg"><span class="on">产出 <span class="num t3">3</span></span><span>过程</span></div></div>
  <div class="psec"><h4>结论</h4><div class="decision">邀请码输入框移到窗口中部并自动聚焦；粘贴时自动规整格式。<div class="m">林晓 定下 · 今天 14:10</div></div></div>
  <div class="psec"><h4>产出</h4>${items}</div>
  <div class="psec"><h4>谁在做什么</h4><dl class="kv">
    <dt>负责</dt><dd>${avatar("设计助手", { size: 20, bot: true, palette: "teal", mode, ring: "var(--panel)" })}<span class="ell">设计助手</span></dd>
    <dt>等待</dt><dd>${avatar("Joye", { size: 20, palette: "graphite", mode })}<span class="ell">你审方案 v3</span></dd>
    <dt>关注</dt><dd><span class="t2 ell">林晓、陈思远 和另外 2 位</span></dd>
  </dl></div>
</aside>`;
}

export function mainWindow(d, mode, { panelTab = "artifacts", mentionPopup = true, overlay = "" } = {}) {
  return `<div class="desk"></div>
<div class="win">
${sidebar(d, mode)}
${topicList(d)}
${topicView(d, mode, { mentionPopup })}
${panel(d, mode, panelTab)}
</div>
${overlay}`;
}
