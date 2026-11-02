// The main window: sidebar, topic list, topic view, optional side panel (DESIGN §8).
import { M, sidebar as SB, topics as TOPICS } from "./data.mjs";
import { avatar, character, chip, esc, groupTile, icon, kbd } from "./kit.mjs";

export const MAIN_CSS = `
.win{display:flex;width:var(--w);height:var(--h);background:var(--content);position:relative}
.lights{position:absolute;left:20px;top:20px;display:flex;gap:8px;z-index:5}
.lights span{width:12px;height:12px;border-radius:50%;background:#FF5F57;box-shadow:inset 0 0 0 0.5px rgba(0,0,0,.15)}
.lights span:nth-child(2){background:#FEBC2E}.lights span:nth-child(3){background:#28C840}
/* Sidebar */
.sb{width:var(--sbw);flex:none;background:var(--sidebar);display:flex;flex-direction:column;border-right:0.5px solid var(--sep)}
.sb-top{height:52px;flex:none}
.sb-search{margin:0 12px 8px;height:30px;border-radius:8px;background:var(--field);display:flex;align-items:center;gap:8px;padding:0 8px 0 10px;color:var(--placeholder);font-size:13px}
.sb-search .grow{flex:1}
.sb-scroll{flex:1;overflow:hidden;padding:0 8px}
.sec{display:flex;align-items:center;height:28px;padding:0 8px;margin-top:12px;color:var(--text-2);font-size:12px;font-weight:600;gap:4px}
.sec .chev{color:var(--text-3)}
.sec .grow{flex:1}
.sec .add{width:22px;height:22px;border-radius:6px;display:flex;align-items:center;justify-content:center;color:var(--text-3)}
.row{display:flex;align-items:center;gap:8px;height:30px;padding:0 8px;border-radius:8px;color:var(--text-2);font-size:13px}
.row .name{flex:1}
.row.unread .name{color:var(--text-1);font-weight:600}
.row.sel{background:var(--sidebar-selected);color:var(--text-1)}
.row.sel .name{font-weight:600;color:var(--text-1)}
.row.hov{background:var(--sidebar-hover)}
.row.muted{opacity:.6}
.todo{display:flex;gap:8px;padding:6px 8px;border-radius:8px;align-items:flex-start}
.todo .gl{width:20px;height:20px;display:flex;align-items:center;justify-content:center;flex:none;color:var(--you-glyph)}
.todo .gl.at{color:var(--accent)}
.todo .tt{font-size:13px;line-height:18px;font-weight:500;color:var(--text-1)}
.todo .cx{font-size:12px;line-height:16px;color:var(--text-2)}
.todo .body{flex:1;min-width:0}
.sb-foot{height:52px;flex:none;display:flex;align-items:center;gap:8px;padding:0 12px;margin:0 8px 0;border-top:0.5px solid var(--sep)}
.sb-foot .who{flex:1;min-width:0}
.sb-foot .n{font-size:13px;font-weight:500}
.sb-foot .s{font-size:12px;line-height:16px;color:var(--text-2)}
/* Topic list */
.tl{width:var(--tlw);flex:none;display:flex;flex-direction:column;border-right:0.5px solid var(--sep);background:var(--content)}
.hdr{height:52px;flex:none;display:flex;align-items:center;gap:8px;padding:0 12px 0 16px}
.hdr .ttl{font-size:16px;line-height:22px;font-weight:600}
.hdr .sub{font-size:12px;color:var(--text-2)}
.tabs{display:flex;gap:20px;padding:0 16px;height:34px;align-items:stretch;border-bottom:0.5px solid var(--sep);flex:none}
.tab{display:flex;align-items:center;gap:4px;font-size:13px;color:var(--text-2);position:relative}
.tab .n{color:var(--text-3)}
.tab.on{color:var(--text-1);font-weight:600}
.tab.on::after{content:"";position:absolute;left:0;right:0;bottom:-0.5px;height:2px;border-radius:1px;background:var(--text-1)}
.tl-list{flex:1;overflow:hidden;padding:6px 8px}
.trow{display:flex;gap:10px;padding:10px 10px 10px 10px;border-radius:8px;position:relative;height:62px}
.trow.sel{background:var(--selected)}
.trow.hov{background:var(--hover)}
.trow .body{flex:1;min-width:0;display:flex;flex-direction:column;gap:3px}
.trow .l1,.trow .l2{display:flex;align-items:center;gap:8px;height:20px}
.trow .tt{flex:1;font-size:13px;font-weight:500}
.trow.unread .tt{font-weight:600}
.trow .tm{font-size:12px;color:var(--text-3);flex:none}
.trow .pv{flex:1;font-size:13px;color:var(--text-2)}
.trow .dot{position:absolute;left:2px;top:19px;width:6px;height:6px;border-radius:50%;background:var(--unread-dot)}
.trow .cnt{font-size:12px;font-weight:600;color:var(--accent-text);flex:none}
/* Topic view */
.tv{flex:1;min-width:0;display:flex;flex-direction:column;position:relative;background:var(--content)}
.tv-hdr{height:52px;flex:none;display:flex;align-items:center;gap:10px;padding:0 12px 0 24px}
.tv-hdr.edge{box-shadow:0 0.5px 0 var(--sep)}
.tv-hdr .ttl{font-size:16px;line-height:22px;font-weight:600}
.tv-hdr .grow{flex:1}
.pile{display:flex;align-items:center;margin-right:4px}
.pile .av{margin-left:-6px;border-radius:50%;box-shadow:0 0 0 2px var(--content)}
.pile .av:first-child{margin-left:0}
.pile .more{margin-left:4px;font-size:12px;color:var(--text-2)}
.tv-scroll{flex:1;overflow:hidden;position:relative}
.tv-inner{max-width:760px;padding:8px 24px 8px 24px}
.tv-inner.bottom{position:absolute;left:0;right:0;bottom:0}
.datediv{display:flex;align-items:center;gap:0;margin:16px 0 8px;position:relative;justify-content:center}
.datediv::before{content:"";position:absolute;left:0;right:0;top:50%;height:0.5px;background:var(--sep)}
.datediv span{position:relative;background:var(--content);border:0.5px solid var(--border);border-radius:11px;height:22px;padding:0 10px;display:flex;align-items:center;font-size:12px;font-weight:500;color:var(--text-2)}
.msg{display:flex;gap:10px;padding:6px 8px;margin:0 -8px;border-radius:8px;position:relative}
.msg.cont{padding-top:1px}
.msg.hov{background:var(--hover)}
.msg.mine{}
.msg .gut{width:32px;flex:none;display:flex;justify-content:center}
.msg .gut .ht{font-size:12px;line-height:22px;color:var(--text-3);visibility:hidden}
.msg.hov .gut .ht{visibility:visible}
.msg .main{flex:1;min-width:0;max-width:620px}
.meta{display:flex;align-items:center;gap:6px;height:20px}
.meta .nm{font-size:13px;font-weight:600}
.meta .tm{font-size:12px;color:var(--text-3)}
.rtag{display:inline-flex;align-items:center;gap:3px;height:18px;padding:0 5px;border-radius:4px;background:var(--field);color:var(--text-2);font-size:11px;line-height:14px;font-weight:500}
.txt{font-size:14px;line-height:22px;color:var(--text-1)}
.txt p+p{margin-top:6px}
.txt b{font-weight:600}
.txt code{font-family:ui-monospace,Menlo,monospace;font-size:12.5px;background:var(--code-bg);padding:1px 4px;border-radius:4px}
.mn{background:var(--mention-bg);color:var(--accent-text);border-radius:4px;padding:0 3px;font-weight:500}
.mn.me{background:var(--me-bg);color:var(--me-text)}
.edited{font-size:12px;color:var(--text-3);margin-left:4px}
.reacts{display:flex;gap:6px;margin-top:6px}
.react{display:inline-flex;align-items:center;gap:5px;height:24px;padding:0 8px;border-radius:12px;background:var(--field);font-size:12px;color:var(--text-2)}
.react.mine{background:var(--accent-soft);color:var(--accent-text);box-shadow:inset 0 0 0 1px var(--accent)}
.hbar{position:absolute;right:12px;top:-14px;display:flex;gap:2px;padding:2px;border-radius:8px;background:var(--overlay);box-shadow:var(--shadow-float)}
.hbar .ib{width:28px;height:26px;border-radius:6px}
.steps{display:inline-flex;align-items:center;gap:6px;height:24px;font-size:12px;color:var(--text-2);margin:2px 0 4px;padding:0 8px 0 4px;border-radius:6px}
.steps .d{color:var(--text-3)}
.live{display:flex;align-items:center;gap:10px;padding:8px 0 4px}
.live .lbl{font-size:13px;color:var(--text-2)}
.live .lbl b{color:var(--text-1);font-weight:600}
.live .el{font-size:12px;color:var(--text-3)}
.live .grow{flex:1}
.pulse{display:inline-flex;gap:3px;align-items:center}
.pulse i{width:4px;height:4px;border-radius:50%;background:var(--text-3)}
.pulse i:nth-child(2){opacity:.6}.pulse i:nth-child(3){opacity:.3}
/* Cards (DESIGN §9) */
.card{margin-top:8px;width:min(520px,100%);border-radius:12px;background:var(--raised);box-shadow:inset 0 0 0 0.5px var(--border);padding:12px 14px}
.card .ch{display:flex;align-items:center;gap:8px}
.card .ct{flex:1;font-size:14px;line-height:20px;font-weight:600}
.card .cs{font-size:13px;line-height:18px;color:var(--text-2);margin-top:4px}
.opts{margin-top:10px;display:flex;flex-direction:column;gap:6px}
.opt{display:flex;align-items:center;gap:10px;height:36px;padding:0 10px;border-radius:8px;background:var(--content);box-shadow:inset 0 0 0 0.5px var(--border);font-size:13px}
.opt.hov{box-shadow:inset 0 0 0 1px var(--accent);background:var(--accent-soft)}
.opt .grow{flex:1}
.card .acts{display:flex;gap:8px;margin-top:12px}
.dl{margin-top:10px;border-radius:8px;background:var(--content);box-shadow:inset 0 0 0 0.5px var(--border);overflow:hidden}
.dl .r{display:flex;gap:12px;padding:7px 10px;font-size:13px;border-bottom:0.5px solid var(--sep)}
.dl .r:last-child{border-bottom:0}
.dl .k{width:40px;flex:none;color:var(--text-3)}
.dl .body{padding:10px;font-size:13px;line-height:20px;color:var(--text-1)}
.file{margin-top:8px;width:min(520px,100%);display:flex;align-items:center;gap:12px;padding:10px 12px;border-radius:12px;box-shadow:inset 0 0 0 0.5px var(--border);background:var(--content)}
.file .ft{width:36px;height:36px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.file .fn{font-size:13px;font-weight:500}
.file .fm{font-size:12px;color:var(--text-2)}
.file .grow{flex:1;min-width:0}
.sendst{font-size:12px;color:var(--text-3);display:inline-flex;gap:4px;align-items:center;margin-left:6px;vertical-align:-2px}
/* Composer (DESIGN §10) */
.cmp-wrap{flex:none;padding:0 24px 16px;max-width:808px}
.typing{height:20px;font-size:12px;line-height:20px;color:var(--text-3);padding-left:2px}
.cmp{border-radius:12px;box-shadow:inset 0 0 0 0.5px var(--border-strong);background:var(--content);padding:9px 8px 8px 12px;display:flex;align-items:flex-end;gap:8px;position:relative}
.cmp.focus-w{box-shadow:inset 0 0 0 1px var(--border-strong),0 0 0 3px var(--field)}
.cmp .field{flex:1;font-size:14px;line-height:22px;padding:2px 0 3px;min-height:27px}
.cmp .ph{color:var(--placeholder)}
.cmp .tools{display:flex;gap:2px;align-items:center}
.send{width:28px;height:28px;border-radius:50%;display:flex;align-items:center;justify-content:center;background:var(--field);color:var(--text-3)}
.send.on{background:var(--accent);color:#fff}
.caret{display:inline-block;width:1.5px;height:17px;background:var(--accent);vertical-align:-3px;margin-left:1px}
.pop{position:absolute;background:var(--overlay);border-radius:12px;box-shadow:var(--shadow-overlay);padding:4px}
.pop .pr{display:flex;align-items:center;gap:8px;height:32px;padding:0 8px;border-radius:6px;font-size:13px}
.pop .pr.act{background:var(--active-row)}
.pop .pr .grow{flex:1}
.pop .ph{padding:6px 8px 4px;font-size:12px;color:var(--text-3);font-weight:500}
/* Side panel */
.sp{width:var(--spw);flex:none;border-left:0.5px solid var(--sep);display:flex;flex-direction:column;background:var(--content)}
.sp-hdr{height:52px;display:flex;align-items:center;padding:0 12px 0 16px;gap:6px;flex:none}
.sp-hdr .t{flex:1;font-size:13px;font-weight:600}
.sp-body{padding:0 16px;display:flex;flex-direction:column;gap:20px}
.sp-sec .h{display:flex;align-items:center;font-size:12px;font-weight:600;color:var(--text-2);height:24px;margin-bottom:6px}
.sp-sec .h .grow{flex:1}
.live-card{border-radius:12px;background:var(--raised);box-shadow:inset 0 0 0 0.5px var(--border);padding:12px}
.live-card .top{display:flex;gap:10px;align-items:center}
.live-card .nm{font-size:13px;font-weight:600}
.live-card .st{font-size:12px;color:var(--text-2)}
.plan{margin-top:10px;display:flex;flex-direction:column;gap:2px}
.plan .s{display:flex;gap:8px;align-items:center;height:24px;font-size:12px;color:var(--text-2)}
.plan .s.ok .i{color:var(--success)}
.plan .s.now{color:var(--text-1);font-weight:500}
.plan .s.now .i{color:var(--accent)}
.plan .s.todo .i{color:var(--text-3)}
.prow{display:flex;align-items:center;gap:10px;height:40px;padding:0 6px;margin:0 -6px;border-radius:8px}
.prow .grow{flex:1;min-width:0}
.prow .a{font-size:13px;font-weight:500}
.prow .b{font-size:12px;color:var(--text-2)}
.prow .ft{width:28px;height:28px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.mrow{display:flex;align-items:center;gap:8px;height:32px;font-size:13px}
.mrow .grow{flex:1}
.linkbtn{font-size:13px;color:var(--accent-text);display:inline-flex;gap:4px;align-items:center}
.start{padding:28px 0 12px}
.start .t{font-size:20px;line-height:28px;font-weight:600;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.start .m{display:flex;align-items:center;gap:8px;margin-top:8px;font-size:12px;color:var(--text-2);min-width:0}.start .m>span:last-child{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.start .m .av{margin-right:-2px}
.overlay{position:absolute;inset:0;background:var(--scrim);z-index:20}
`;

const m = (k) => M[k];

function sidebarHtml(mode, opt = {}) {
  const sb = opt.sidebar ?? SB;
  const todo = sb.todo.map((t, i) => `<div class="todo ${i === 0 && opt.todoHover ? "row hov" : ""}"><span class="gl ${t.icon === "at-sign" ? "at" : ""}">${icon(t.icon, 15)}</span><div class="body"><div class="tt ell">${esc(t.title)}</div><div class="cx ell">${esc(t.ctx)}</div></div></div>`).join("");
  const groups = sb.groups.map((g) => {
    const trail = g.mention ? `<span class="badge">${g.mention}</span>` : g.muted ? `<span class="t3">${icon("bell-off", 13)}</span>` : "";
    return `<div class="row ${g.selected ? "sel" : ""} ${g.unread ? "unread" : ""} ${g.muted ? "muted" : ""} ${g.hover ? "hov" : ""}">${groupTile(g.name, g.palette, mode, 20)}<span class="name ell">${esc(g.name)}</span>${trail}</div>`;
  }).join("");
  const dms = sb.dms.map((d) => {
    const mm = typeof d.m === "string" ? m(d.m) : d.m;
    const trail = d.count ? `<span class="badge">${d.count > 99 ? "99+" : d.count}</span>` : "";
    return `<div class="row ${d.count ? "unread" : ""}">${avatar(mm, 20, mode, { ring: "var(--sidebar)" })}<span class="name ell">${esc(mm.name)}</span>${trail}</div>`;
  }).join("");
  return `<aside class="sb"><div class="sb-top"></div>
<div class="sb-search">${icon("search", 14)}<span class="grow">搜索</span>${kbd("⌘K")}</div>
<div class="sb-scroll">
<div class="sec" style="margin-top:4px"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">待办</span><span class="badge">${sb.todoCount ?? sb.todo.length}</span></div>${todo}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">群</span><span class="add">${icon("plus", 14)}</span></div>${groups}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">私聊</span><span class="add">${icon("plus", 14)}</span></div>${dms}
</div>
<div class="sb-foot">${avatar(m("joye"), 28, mode, { ring: "var(--sidebar)" })}<div class="who"><div class="n ell">${esc(sb.me ?? "Joye")}</div><div class="s">在线</div></div><span class="ib">${icon("settings", 16)}</span></div>
</aside>`;
}

function topicListHtml(mode, opt = {}) {
  const list = opt.topics ?? TOPICS;
  const rows = list.map((t) => {
    const unread = t.unread || t.mention;
    // One trailing element per row: 等你… chip > @ badge > other chip > unread count.
    const c = chip(t.status, { you: t.you, name: t.name, row: true });
    const trail = t.you && c ? c : t.mention ? `<span class="badge">@${t.mention}</span>` : c ? c : t.unread ? `<span class="cnt">${t.unread > 999 ? "999+" : t.unread}</span>` : "";
    return `<div class="trow ${t.selected ? "sel" : ""} ${unread ? "unread" : ""} ${t.hover ? "hov" : ""}">${unread && !t.selected ? '<span class="dot"></span>' : ""}
${avatar(m(t.owner) ?? t.ownerM, 32, mode, { presence: false })}
<div class="body"><div class="l1"><span class="tt ell">${esc(t.title)}</span><span class="tm">${esc(t.time)}</span></div>
<div class="l2"><span class="pv ell">${esc(t.who)}：${esc(t.text)}</span>${trail}</div></div></div>`;
  }).join("");
  const g = opt.group ?? { name: "qqbot 研发", sub: "9 位成员", counts: [9, 3, 2] };
  return `<section class="tl"><div class="hdr"><div style="flex:1;min-width:0"><div class="ttl ell">${esc(g.name)}</div></div><span class="btn secondary" style="height:28px;padding:0 10px">${icon("square-pen", 14)}新话题</span></div>
<div class="tabs"><span class="tab on">全部</span><span class="tab">未读${g.counts[1] ? ` <span class="n">${g.counts[1]}</span>` : ""}</span><span class="tab">等我${g.counts[2] ? ` <span class="n">${g.counts[2]}</span>` : ""}</span></div>
<div class="tl-list">${rows}</div></section>`;
}

function msgHtml(x, mode) {
  if (x.date) return `<div class="datediv"><span>${esc(x.date)}</span></div>`;
  if (x.html) return x.html;
  const mm = m(x.m) ?? x.mm;
  const head = x.cont
    ? ""
    : `<div class="meta"><span class="nm">${esc(mm.name)}</span>${mm.char ? `<span class="rtag">${icon("bot", 11)}${esc(mm.role)}</span>` : ""}<span class="tm">${esc(x.time)}</span></div>`;
  const gut = x.cont ? `<div class="gut"><span class="ht">${esc(x.time)}</span></div>` : `<div class="gut">${avatar(mm, 32, mode)}</div>`;
  const bar = x.hover ? `<div class="hbar"><span class="ib">${icon("face-slightly-smiling-plus", 16)}</span><span class="ib">${icon("reply", 15)}</span><span class="ib">${icon("text-quote", 15)}</span><span class="ib">${icon("ellipsis", 15)}</span></div>` : "";
  return `<div class="msg ${x.cont ? "cont" : ""} ${x.hover ? "hov" : ""}">${gut}<div class="main">${head}${x.pre ?? ""}<div class="txt">${x.text ?? ""}</div>${x.after ?? ""}</div>${bar}</div>`;
}

function composerHtml(mode, opt = {}) {
  const pop = opt.mention ? `<div class="pop" style="left:${opt.popX ?? 0}px;bottom:calc(100% + 6px);width:260px">
<div class="ph">成员</div>
<div class="pr act">${avatar(m("scout"), 20, mode)}<span>Scout</span><span class="grow t3">测试验收</span><span class="t3">${icon("corner-down-left", 13)}</span></div>
<div class="pr">${avatar(m("chen"), 20, mode, { ring: "var(--overlay)" })}<span>陈晨</span><span class="grow t3">设计实习生</span></div>
<div class="pr">${avatar({ ...m("forge"), state: "idle" }, 20, mode)}<span>Forge</span><span class="grow t3">开发</span></div>
</div>` : "";
  const field = opt.draft ? `<div class="field">${opt.draft}<span class="caret"></span></div>` : `<div class="field ell"><span class="ph">${esc(opt.placeholder ?? "回复")}</span></div>`;
  return `<div class="cmp-wrap"><div class="typing">${opt.typing ?? ""}</div><div class="cmp ${opt.draft ? "focus-w" : ""}">${pop}
<span class="ib" style="margin-left:-4px">${icon("plus", 18)}</span>${field}
<div class="tools"><span class="ib">${icon("at-sign", 17)}</span><span class="ib">${icon("face-slightly-smiling", 17)}</span><span class="send ${opt.draft ? "on" : ""}">${icon("arrow-up", 16)}</span></div></div></div>`;
}

function headerHtml(mode, t) {

  return `<div class="tv-hdr ${t.edge ? "edge" : ""}"><span class="ttl ell">${esc(t.title)}</span>${chip(t.status, { you: t.you, name: t.name })}<span class="grow"></span>
<span class="btn ghost" style="padding:0 8px;gap:5px">${icon("users", 15)}${t.people.length}</span>
<span class="ib">${icon("bell", 16)}</span><span class="ib">${icon(t.panelOpen ? "panel-right" : "panel-right", 16)}</span><span class="ib">${icon("ellipsis", 16)}</span></div>`;
}

export function mainWindow(mode, o) {
  const tv = o.topic;
  const msgs = tv.messages.map((x) => msgHtml(x, mode)).join("");
  const start = tv.start ? `<div class="start"><div class="t">${esc(tv.title)}</div><div class="m">${chip(tv.status, { you: tv.you, name: tv.name })}<span>${tv.start}</span></div></div>` : "";
  const panel = o.panel ? o.panel(mode) : "";
  return `<div class="win" style="--sbw:${o.sbw ?? 248}px;--tlw:${o.tlw ?? 320}px;--spw:${o.spw ?? 300}px">
<div class="lights"><span></span><span></span><span></span></div>
${sidebarHtml(mode, o)}
${topicListHtml(mode, o)}
<main class="tv">${headerHtml(mode, tv)}
<div class="tv-scroll"><div class="tv-inner ${tv.start ? "" : "bottom"}">${start}${msgs}</div></div>
${composerHtml(mode, tv.composer ?? {})}</main>
${panel}
${o.overlay ? o.overlay(mode) : ""}
</div>`;
}

export { character, avatar, icon, chip, esc, m };
