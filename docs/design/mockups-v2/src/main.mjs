// The main window: sidebar, topic list, topic view, optional side panel (DESIGN §4.4, §7, §8.1).
import { M, sidebar as SB, topics as TOPICS } from "./data.mjs";
import { avatar, badge, character, chip, count, esc, icon, kbd } from "./kit.mjs";

export const MAIN_CSS = `
.win{display:flex;width:var(--w);height:var(--h);background:var(--content);position:relative}
.lights{position:absolute;left:20px;top:20px;display:flex;gap:8px;z-index:5}
.lights span{width:12px;height:12px;border-radius:50%;background:#FF5F57;box-shadow:inset 0 0 0 0.5px rgba(0,0,0,.15)}
.lights span:nth-child(2){background:#FEBC2E}.lights span:nth-child(3){background:#28C840}
/* Sidebar (§7.7) */
.sb{width:var(--sbw);flex:none;background:var(--sidebar);display:flex;flex-direction:column;border-right:0.5px solid var(--sep)}
.sb-top{height:52px;flex:none}
.sb-search{margin:0 12px 8px;height:36px;border-radius:8px;background:var(--field);display:flex;align-items:center;gap:8px;padding:0 8px 0 12px;color:var(--text-3);font-size:14px;line-height:20px}
.sb-search .ph{flex:1;color:var(--placeholder)}
.sb-scroll{flex:1;overflow:hidden;padding:0 8px}
.sec{display:flex;align-items:center;height:28px;padding:0 8px;margin-top:16px;color:var(--text-2);font-size:12px;line-height:16px;font-weight:600;gap:6px}
.sec .chev{color:var(--text-3)}
.sec .grow{flex:1}
.sec .n{color:var(--text-3);font-weight:500}
.sec .add{margin-right:-6px}
.row{display:flex;align-items:center;gap:10px;height:32px;padding:0 8px;border-radius:8px;color:var(--text-2);font-size:14px;line-height:20px}
.row .slot{width:24px;height:24px;flex:none;display:flex;align-items:center;justify-content:center;color:var(--text-3)}
.row .name{flex:1}
.row.unread .name{color:var(--text-1);font-weight:600}
.row.unread .slot{color:var(--text-1)}
.row.sel{background:var(--sidebar-selected);color:var(--text-1)}
.row.sel .name{font-weight:600}
.row.sel .slot{color:var(--text-1)}
.row.hov{background:var(--sidebar-hover)}
.row.muted{opacity:.6}
.todo{display:flex;gap:10px;padding:6px 8px;border-radius:8px;align-items:flex-start}
.todo .gl{width:24px;height:20px;display:flex;align-items:center;justify-content:center;flex:none;color:var(--text-3)}
.todo .tt{font-size:14px;line-height:20px;font-weight:500;color:var(--text-1)}
.todo .cx{font-size:12px;line-height:16px;color:var(--text-2);margin-top:1px}
.todo .body{flex:1;min-width:0}
.sb-foot{height:56px;flex:none;display:flex;align-items:center;gap:10px;padding:0 8px 0 12px;margin:0 8px;border-top:0.5px solid var(--sep)}
.sb-foot .who{flex:1;min-width:0}
.sb-foot .n{font-size:14px;line-height:20px;font-weight:500}
.sb-foot .s{font-size:12px;line-height:16px;color:var(--text-2)}
/* Topic list (§7.6, §8.1) */
.tl{width:var(--tlw);flex:none;display:flex;flex-direction:column;border-right:0.5px solid var(--sep);background:var(--content)}
.hdr{height:52px;flex:none;display:flex;align-items:center;gap:8px;padding:0 10px 0 16px}
.hdr .ttl{flex:1;font-size:17px;line-height:24px;font-weight:600}
.tabs{display:flex;gap:24px;padding:0 16px;height:40px;align-items:stretch;border-bottom:0.5px solid var(--sep);flex:none}
.tab{display:flex;align-items:center;gap:6px;font-size:14px;line-height:20px;color:var(--text-2);position:relative}
.tab .n{color:var(--text-3);font-size:13px}
.tab.on{color:var(--text-1);font-weight:600}
.tab.on::after{content:"";position:absolute;left:0;right:0;bottom:-0.5px;height:2px;border-radius:1px;background:var(--text-1)}
.tl-list{flex:1;overflow:hidden;padding:8px}
.trow{display:flex;gap:12px;padding:12px 10px 12px 14px;border-radius:8px;position:relative;height:68px}
.trow.sel{background:var(--selected)}
.trow.hov{background:var(--hover)}
.trow .body{flex:1;min-width:0;display:flex;flex-direction:column;gap:4px;margin-top:-2px}
.trow .l1,.trow .l2{display:flex;align-items:center;gap:8px;height:20px}
.trow .tt{flex:1;font-size:14px;line-height:20px;font-weight:500}
.trow.unread .tt{font-weight:600}
.trow .tm{font-size:12px;line-height:16px;color:var(--text-3);flex:none}
.trow .pv{flex:1;font-size:13px;line-height:20px;color:var(--text-2)}
.trow .dot{position:absolute;left:4px;top:19px;width:6px;height:6px;border-radius:50%;background:var(--unread-dot)}
/* Topic view (§7.5, §8.1) */
.tv{flex:1;min-width:0;display:flex;flex-direction:column;position:relative;background:var(--content)}
.tv-hdr{height:52px;flex:none;display:flex;align-items:center;gap:8px;padding:0 12px 0 24px}
.tv-hdr.edge{box-shadow:0 0.5px 0 var(--sep)}
.tv-hdr .ttl{font-size:17px;line-height:24px;font-weight:600;min-width:0}
.tv-hdr .grow{flex:1}
.tv-hdr .acts{display:flex;gap:4px;align-items:center}
.tv-scroll{flex:1;overflow:hidden;position:relative}
.box{max-width:808px;padding:0 24px}
.tv-inner{padding-top:16px;padding-bottom:8px}
.tv-inner.bottom{position:absolute;left:0;right:0;bottom:0}
.datediv{display:flex;align-items:center;gap:12px;margin:16px 0 4px;height:20px;font-size:12px;line-height:16px;font-weight:500;color:var(--text-3)}
.datediv::before,.datediv::after{content:"";flex:1;height:0.5px;background:var(--sep)}
.msg{display:flex;gap:12px;padding:6px 8px;margin:6px -8px 0;border-radius:8px;position:relative}
.msg.cont{margin-top:0;padding-top:2px}
.msg.hov{background:var(--hover)}
.msg .gut{width:36px;flex:none;display:flex;justify-content:center}
.msg .gut .ht{font-size:12px;line-height:22px;color:var(--text-3);visibility:hidden}
.msg.hov .gut .ht{visibility:visible}
.msg .main{flex:1;min-width:0}
.meta{display:flex;align-items:center;gap:8px;height:22px;min-width:0}
.meta .nm{font-size:14px;line-height:22px;font-weight:600;flex:0 1 auto;max-width:160px}
.meta .tm{font-size:12px;line-height:16px;color:var(--text-3);flex:none}
.rtag{display:inline-flex;align-items:center;gap:3px;height:20px;padding:0 6px;border-radius:6px;background:var(--field);color:var(--text-2);font-size:12px;line-height:16px;font-weight:500;flex:none}
.txt{font-size:14px;line-height:22px;color:var(--text-1);max-width:620px;overflow-wrap:anywhere}
.txt p+p{margin-top:8px}
.txt b{font-weight:600}
.txt code{font-family:ui-monospace,Menlo,monospace;font-size:13px;background:var(--code-bg);padding:1px 4px;border-radius:4px}
.mn{background:var(--mention-bg);color:var(--accent-text);border-radius:4px;padding:0 3px;font-weight:500}
.mn.me{background:var(--me-bg);color:var(--me-text)}
.reacts{display:flex;gap:6px;margin-top:6px}
.react{display:inline-flex;align-items:center;gap:5px;height:24px;padding:0 8px;border-radius:12px;background:var(--field);font-size:12px;line-height:16px;color:var(--text-2)}
.react.mine{background:var(--accent-soft);color:var(--accent-text);box-shadow:inset 0 0 0 1px var(--accent)}
.hbar{position:absolute;right:8px;top:-18px;display:flex;gap:2px;padding:2px;border-radius:8px;background:var(--overlay);box-shadow:var(--shadow-float)}
.hbar .ib{border-radius:6px}
.steps{display:inline-flex;align-items:center;gap:6px;height:24px;font-size:13px;line-height:20px;color:var(--text-2);margin:2px 0 2px -4px;padding:0 8px 0 4px;border-radius:6px}
.steps .d{color:var(--text-3)}
.live{display:flex;align-items:center;gap:12px;margin-top:12px;height:28px;font-size:13px;line-height:20px}
.live .lbl{color:var(--text-2)}
.live .lbl b{color:var(--text-1);font-weight:600}
.live .el{font-size:12px;color:var(--text-3)}
.pulse{display:inline-flex;gap:3px;align-items:center}
.pulse i{width:4px;height:4px;border-radius:50%;background:var(--text-3)}
.pulse i:nth-child(2){opacity:.6}.pulse i:nth-child(3){opacity:.3}
.linkbtn{font-size:13px;line-height:20px;color:var(--accent-text);display:inline-flex;gap:6px;align-items:center}
/* Cards (§7.10) */
.card{margin-top:6px;width:min(560px,100%);border-radius:12px;background:var(--raised);box-shadow:inset 0 0 0 1px var(--border);padding:14px 16px 16px}
.card.you{box-shadow:inset 0 0 0 1px var(--you-border)}
.card .ct{font-size:14px;line-height:22px;font-weight:600}
.card .cs{font-size:14px;line-height:22px;color:var(--text-2);margin-top:2px}
.opts{margin-top:12px;display:flex;flex-direction:column;gap:6px}
.opt{display:flex;align-items:center;gap:10px;height:40px;padding:0 12px 0 10px;border-radius:8px;background:var(--content);box-shadow:inset 0 0 0 1px var(--border);font-size:14px;line-height:20px}
.opt.hov{box-shadow:inset 0 0 0 1px var(--border-strong);background:var(--content)}
.opt.hov .kbd{background:var(--selected);color:var(--text-1)}
.card .acts{display:flex;gap:8px;margin-top:14px}
.dl{margin-top:12px;border-radius:8px;background:var(--content);box-shadow:inset 0 0 0 1px var(--border);overflow:hidden}
.dl .r{display:flex;gap:12px;padding:9px 12px;font-size:14px;line-height:20px;border-bottom:0.5px solid var(--sep)}
.dl .k{width:48px;flex:none;color:var(--text-3)}
.dl .body{padding:10px 12px 12px;font-size:14px;line-height:22px;color:var(--text-1)}
.file{margin-top:8px;width:min(560px,100%);display:flex;align-items:center;gap:12px;padding:12px 8px 12px 12px;border-radius:12px;box-shadow:inset 0 0 0 1px var(--border);background:var(--content);position:relative;overflow:hidden}
.file .ft{width:40px;height:40px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.file .fn{font-size:14px;line-height:20px;font-weight:500}
.file .fm{font-size:12px;line-height:16px;color:var(--text-2);margin-top:2px}
.file .grow{flex:1;min-width:0}
.file .bar{position:absolute;left:0;bottom:0;height:2px;background:var(--accent)}
.sendst{font-size:12px;color:var(--danger-text);display:inline-flex;gap:4px;align-items:center;margin-left:8px;vertical-align:-1px}
/* Composer (§7.2) */
.cmp-wrap{flex:none;padding-bottom:16px}
.typing{height:20px;font-size:12px;line-height:20px;color:var(--text-3);padding-left:2px}
.cmp{border-radius:12px;box-shadow:inset 0 0 0 1px var(--border);background:var(--content);display:flex;flex-direction:column;position:relative;min-height:96px}
.cmp.focus-w{box-shadow:inset 0 0 0 1px var(--focus-border),0 0 0 3px var(--focus-soft)}
.cmp .field{font-size:14px;line-height:22px;padding:12px 14px 0;min-height:56px;overflow-wrap:anywhere}
.cmp .ph{color:var(--placeholder)}
.cmp .bar{height:40px;display:flex;align-items:center;gap:2px;padding:0 8px 8px}
.cmp .bar .grow{flex:1}
.cmp .bar .div{width:1px;height:16px;background:var(--sep);margin:0 4px}
.send{width:32px;height:32px;border-radius:8px;display:flex;align-items:center;justify-content:center;background:var(--field);color:var(--placeholder)}
.send.on{background:var(--accent);color:#fff}
.caret{display:inline-block;width:1.5px;height:18px;background:var(--accent);vertical-align:-4px;margin-left:1px}
.pop{position:absolute;background:var(--overlay);border-radius:12px;box-shadow:var(--shadow-overlay);padding:4px}
.pop .pr{display:flex;align-items:center;gap:10px;height:36px;padding:0 10px;border-radius:6px;font-size:14px;line-height:20px}
.pop .pr.act{background:var(--selected)}
.pop .pr .grow{flex:1}
.pop .pr .role{font-size:13px;color:var(--text-3)}
.pop .ph{display:flex;align-items:center;height:28px;padding:0 10px;font-size:12px;line-height:16px;color:var(--text-3);font-weight:500}
/* Side panel (§8.1) */
.sp{width:var(--spw);flex:none;border-left:0.5px solid var(--sep);display:flex;flex-direction:column;background:var(--content)}
.sp-hdr{height:52px;display:flex;align-items:center;padding:0 10px 0 16px;gap:6px;flex:none}
.sp-hdr .t{flex:1;font-size:17px;line-height:24px;font-weight:600}
.sp-body{padding:0 16px}
.sp-sec{margin-top:16px}
.sp-sec:first-child{margin-top:8px}
.sp-sec .h{display:flex;align-items:center;font-size:12px;line-height:16px;font-weight:600;color:var(--text-3);height:16px;margin-bottom:8px}
.live-card{border-radius:12px;box-shadow:inset 0 0 0 1px var(--border);padding:14px}
.live-card .top{display:flex;gap:12px;align-items:center}
.live-card .nm{font-size:14px;line-height:20px;font-weight:600}
.live-card .st{font-size:13px;line-height:20px;color:var(--text-2)}
.plan{margin-top:10px;display:flex;flex-direction:column}
.plan .s{display:flex;gap:10px;align-items:center;height:28px;font-size:13px;line-height:20px;color:var(--text-2)}
.plan .s.ok .i{color:var(--success)}
.plan .s.now{color:var(--text-1);font-weight:500}
.plan .s.now .i{color:var(--you-glyph)}
.plan .s.later .i{color:var(--text-3)}
.prow{display:flex;align-items:center;gap:12px;height:48px;padding:0 8px;margin:0 -8px;border-radius:8px}
.prow .grow{flex:1;min-width:0}
.prow .a{font-size:14px;line-height:20px;font-weight:500}
.prow .b{font-size:12px;line-height:16px;color:var(--text-2)}
.prow .ft{width:32px;height:32px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.mrow{display:flex;align-items:center;gap:10px;height:40px;font-size:14px;line-height:20px}
.mrow .grow{flex:1}
.mrow .role{font-size:13px;color:var(--text-3)}
.start{padding:12px 0 8px}
.start .t{font-size:22px;line-height:30px;font-weight:600;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.start .m{display:flex;align-items:center;gap:8px;margin-top:8px;font-size:13px;line-height:20px;color:var(--text-2);min-width:0}.start .m .nw{flex:none;white-space:pre;margin-left:-8px}
.start + .datediv,.start + .msg{margin-top:16px}
.overlay{position:absolute;inset:0;background:var(--scrim);z-index:20}
`;

const m = (k) => M[k];

function sidebarHtml(mode, opt = {}) {
  const sb = opt.sidebar ?? SB;
  const todo = sb.todo.map((t) => `<div class="todo"><span class="gl">${icon(t.icon, 16)}</span><div class="body"><div class="tt ell">${esc(t.title)}</div><div class="cx ell">${esc(t.ctx)}</div></div></div>`).join("");
  const groups = sb.groups.map((g) => {
    const trail = g.mention ? badge(g.mention) : g.muted ? `<span class="t3">${icon("bell-off", 14)}</span>` : "";
    return `<div class="row ${g.selected ? "sel" : ""} ${g.unread ? "unread" : ""} ${g.muted ? "muted" : ""}"><span class="slot">${icon("hash", 16)}</span><span class="name ell">${esc(g.name)}</span>${trail}</div>`;
  }).join("");
  const dms = sb.dms.map((d) => {
    const mm = typeof d.m === "string" ? m(d.m) : d.m;
    const trail = d.count ? badge(d.count > 99 ? "99+" : d.count) : "";
    return `<div class="row ${d.count ? "unread" : ""}"><span class="slot">${avatar(mm, 24, mode, { ring: "var(--sidebar)" })}</span><span class="name ell">${esc(mm.name)}</span>${trail}</div>`;
  }).join("");
  return `<aside class="sb"><div class="sb-top"></div>
<div class="sb-search">${icon("search", 16)}<span class="ph">搜索</span>${kbd("⌘K")}</div>
<div class="sb-scroll">
<div class="sec" style="margin-top:4px"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">待办 <span class="n">${sb.todoCount ?? sb.todo.length}</span></span></div>${todo}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">群</span><span class="ib sm add">${icon("plus", 16)}</span></div>${groups}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">私聊</span><span class="ib sm add">${icon("plus", 16)}</span></div>${dms}
</div>
<div class="sb-foot">${avatar(m("joye"), 32, mode, { ring: "var(--sidebar)" })}<div class="who"><div class="n ell">${esc(sb.me ?? "Joye")}</div><div class="s">在线</div></div><span class="ib">${icon("settings", 18)}</span></div>
</aside>`;
}

function topicListHtml(mode, opt = {}) {
  const list = opt.topics ?? TOPICS;
  const rows = list.map((t) => {
    const unread = t.unread || t.mention;
    // One trailing element per row (§9.1): 等你… chip > @ badge > 卡住了 > other label > unread count.
    const c = chip(t.status, { you: t.you, name: t.name, row: true });
    const n = (v) => (typeof v === "number" && v > 999 ? "999+" : v);
    const trail = t.you && c ? c : t.mention ? badge(`@${t.mention}`) : c ? c : t.unread ? count(n(t.unread)) : "";
    return `<div class="trow ${t.selected ? "sel" : ""} ${unread ? "unread" : ""} ${t.hover ? "hov" : ""}">${unread && !t.selected ? '<span class="dot"></span>' : ""}
${avatar(m(t.owner) ?? t.ownerM, 36, mode, { presence: false })}
<div class="body"><div class="l1"><span class="tt ell">${esc(t.title)}</span><span class="tm">${esc(t.time)}</span></div>
<div class="l2"><span class="pv ell">${esc(t.who)}：${esc(t.text)}</span>${trail}</div></div></div>`;
  }).join("");
  const g = opt.group ?? { name: "qqbot 研发", counts: [9, 3, 2] };
  const fmt = (v) => (typeof v === "number" && v > 999 ? "999+" : v);
  return `<section class="tl"><div class="hdr"><span class="ttl ell">${esc(g.name)}</span><span class="ib" title="新话题 ⌘N">${icon("square-pen", 18)}</span></div>
<div class="tabs"><span class="tab on">全部</span><span class="tab">未读${g.counts[1] ? `<span class="n">${fmt(g.counts[1])}</span>` : ""}</span><span class="tab">等我${g.counts[2] ? `<span class="n">${fmt(g.counts[2])}</span>` : ""}</span></div>
<div class="tl-list">${rows}</div></section>`;
}

function msgHtml(x, mode) {
  if (x.date) return `<div class="datediv">${esc(x.date)}</div>`;
  if (x.html) return x.html;
  const mm = m(x.m) ?? x.mm;
  const head = x.cont
    ? ""
    : `<div class="meta"><span class="nm ell">${esc(mm.name)}</span>${mm.char ? `<span class="rtag">${icon("bot", 12)}${esc(mm.role)}</span>` : ""}<span class="tm">${esc(x.time)}</span></div>`;
  const gut = x.cont ? `<div class="gut"><span class="ht">${esc(x.time)}</span></div>` : `<div class="gut">${avatar(mm, 36, mode)}</div>`;
  const bar = x.hover ? `<div class="hbar"><span class="ib">${icon("face-slightly-smiling-plus", 18)}</span><span class="ib">${icon("reply", 18)}</span><span class="ib">${icon("text-quote", 18)}</span><span class="ib">${icon("ellipsis", 18)}</span></div>` : "";
  return `<div class="msg ${x.cont ? "cont" : ""} ${x.hover ? "hov" : ""}">${gut}<div class="main">${head}${x.pre ?? ""}${x.text ? `<div class="txt">${x.text}</div>` : ""}${x.after ?? ""}</div>${bar}</div>`;
}

function composerHtml(mode, opt = {}) {
  const pop = opt.mention ? `<div class="pop" style="left:${opt.popX ?? 0}px;bottom:calc(100% + 8px);width:280px">
<div class="ph">成员</div>
<div class="pr act">${avatar(m("scout"), 24, mode, { ring: "var(--overlay)" })}<span>Scout</span><span class="grow role">测试验收</span><span class="t3">${icon("corner-down-left", 14)}</span></div>
</div>` : "";
  const field = opt.draft ? `<div class="field">${opt.draft}<span class="caret"></span></div>` : `<div class="field"><div class="ph ell">${esc(opt.placeholder ?? "回复")}</div></div>`;
  return `<div class="cmp-wrap box"><div class="typing">${opt.typing ?? ""}</div><div class="cmp ${opt.draft ? "focus-w" : ""}">${pop}${field}
<div class="bar"><span class="ib" title="添加附件">${icon("plus", 18)}</span><span class="ib" title="提及">${icon("at-sign", 18)}</span><span class="ib" title="表情">${icon("face-slightly-smiling", 18)}</span><span class="ib" title="格式">${icon("type", 18)}</span><span class="grow"></span><span class="send ${opt.draft ? "on" : ""}">${icon("arrow-up", 18)}</span></div></div></div>`;
}

function headerHtml(mode, t) {
  // While the topic start block is on screen the header shows no title or chip (§8.1).
  const left = t.start ? "" : `<span class="ttl ell">${esc(t.title)}</span>${chip(t.status, { you: t.you, name: t.name })}`;
  return `<div class="tv-hdr ${t.edge ? "edge" : ""}">${left}<span class="grow"></span>
<div class="acts"><span class="btn ghost" style="padding:0 10px;gap:6px">${icon("users", 18)}${t.people.length}</span>
<span class="ib">${icon("bell", 18)}</span><span class="ib">${icon("panel-right", 18)}</span><span class="ib">${icon("ellipsis", 18)}</span></div></div>`;
}

export function mainWindow(mode, o) {
  const tv = o.topic;
  const msgs = tv.messages.map((x) => msgHtml(x, mode)).join("");
  const start = tv.start ? `<div class="start"><div class="t">${esc(tv.title)}</div><div class="m">${chip(tv.status, { you: tv.you, name: tv.name })}<span class="ell">${esc(tv.start.slice(0, tv.start.lastIndexOf(" · ")))}</span><span class="nw">${esc(tv.start.slice(tv.start.lastIndexOf(" · ")))}</span></div></div>` : "";
  const panel = o.panel ? o.panel(mode) : "";
  return `<div class="win" style="--sbw:${o.sbw ?? 248}px;--tlw:${o.tlw ?? 320}px;--spw:${o.spw ?? 300}px">
<div class="lights"><span></span><span></span><span></span></div>
${sidebarHtml(mode, o)}
${topicListHtml(mode, o)}
<main class="tv">${headerHtml(mode, tv)}
<div class="tv-scroll"><div class="tv-inner box ${tv.start ? "" : "bottom"}">${start}${msgs}</div></div>
${composerHtml(mode, tv.composer ?? {})}</main>
${panel}
${o.overlay ? o.overlay(mode) : ""}
</div>`;
}

export { character, avatar, icon, chip, esc, m };
