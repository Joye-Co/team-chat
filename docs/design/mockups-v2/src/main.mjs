// The main window: sidebar, topic list, topic view, optional side panel (DESIGN §4.4, §7, §8.1).
import { M, sidebar as SB, topics as TOPICS } from "./data.mjs";
import { avatar, badge, cap, character, chip, esc, fit, icon, kbd, lead, nbadge } from "./kit.mjs";

/** Reaction pill (§7.4): 16 pt emoji, 12/16 label. */
export const react = (e, label, mine = false) => `<span class="react ${mine ? "mine" : ""}"><span class="e">${e}</span>${esc(label)}</span>`;
/** A message of one to three emoji and nothing else renders at 32/40 (§7.4). */
const JUMBO = /^(\p{Extended_Pictographic}\uFE0F?(\u200d\p{Extended_Pictographic}\uFE0F?)*){1,3}$/u;

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
.row.sel .slot{color:var(--sel-glyph)}
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
.hdr .ttl{flex:1;font-size:16px;line-height:24px;font-weight:600}
.tabs{display:flex;gap:24px;padding:0 16px;height:40px;align-items:stretch;border-bottom:0.5px solid var(--sep);flex:none}
.tab{display:flex;align-items:center;gap:6px;font-size:14px;line-height:20px;color:var(--text-2);position:relative}
.tab .n{color:var(--text-3);font-size:13px}
.tab.on{color:var(--text-1);font-weight:600}
.tab.on::after{content:"";position:absolute;left:0;right:0;bottom:-0.5px;height:2px;border-radius:1px;background:var(--text-1)}
.tl-list{flex:1;overflow:hidden;padding:8px}
.trow{display:flex;gap:12px;padding:12px 10px 12px 18px;border-radius:8px;position:relative;height:68px}
.trow.sel{background:var(--selected)}
.trow.sel .tt{font-weight:600;color:var(--text-1)}
.trow.hov{background:var(--hover)}
.trow .body{flex:1;min-width:0;display:flex;flex-direction:column;gap:4px;margin-top:-2px}
.trow .l1,.trow .l2{display:flex;align-items:center;gap:8px;height:20px}
.trow .tt{flex:1;font-size:14px;line-height:20px;font-weight:500}
.trow.unread .tt{font-weight:600}
.trow .tm{font-size:12px;line-height:16px;color:var(--text-3);flex:none}
.trow .pv{flex:1;min-width:0;font-size:13px;line-height:20px;color:var(--text-2);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.trow .dot{position:absolute;left:5px;top:16px;width:8px;height:8px;border-radius:50%;background:var(--unread-dot)}
.trow .tt .lb,.todo .tt .lb,.tv-hdr .ttl .lb{margin-left:-0.5em}
/* Topic view (§7.5, §8.1) */
.tv{flex:1;min-width:0;display:flex;flex-direction:column;position:relative;background:var(--content)}
.tv-hdr{height:92px;flex:none;display:flex;flex-direction:column;padding:0 12px 0 24px;border-bottom:0.5px solid var(--sep)}
.tv-hdr .r1{height:52px;display:flex;align-items:center;gap:8px}
.tv-hdr .r2{height:39.5px;display:flex;align-items:center;gap:8px;font-size:13px;line-height:20px;color:var(--text-2);min-width:0}
.tv-hdr .ttl{font-size:18px;line-height:26px;font-weight:600;min-width:0;flex:1}
.tv-hdr .lbl-s{flex:0 1 auto;min-width:0;max-width:200px;font-size:13px;line-height:20px;font-weight:400}
.tv-hdr .lbl-s.t-quiet,.tv-hdr .lbl-s.t-neutral{color:var(--text-2)}
.tv-hdr .crumb{display:flex;align-items:center;gap:4px;min-width:0;flex:0 1 auto}
.tv-hdr .crumb .i{color:var(--text-3)}
.tv-hdr .sepd{color:var(--text-3);flex:none}
.tv-hdr .ppl{display:inline-flex;align-items:center;gap:4px;flex:none;white-space:nowrap}
.tv-hdr .ppl .i{color:var(--text-3)}
.tv-hdr .acts{display:flex;gap:4px;align-items:center;flex:none}
.tv-scroll{flex:1;overflow:hidden;position:relative;-webkit-mask-image:linear-gradient(to bottom,transparent 0,#000 24px)}
.box{padding:0 24px}
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
.mn-a{position:relative}
.txt p+p{margin-top:8px}
.txt b{font-weight:600}
.txt code{font-family:ui-monospace,Menlo,monospace;font-size:13px;background:var(--code-bg);padding:1px 4px;border-radius:4px}
.mn{background:var(--mention-bg);color:var(--accent-text);border-radius:4px;padding:0 3px;font-weight:500}
.mn.me{background:var(--me-bg);color:var(--me-text)}
.reacts{display:flex;gap:6px;margin-top:6px}
.react{display:inline-flex;align-items:center;gap:4px;height:26px;padding:0 8px 0 6px;border-radius:13px;background:var(--field);font-size:12px;line-height:16px;color:var(--text-2)}
.react .e{font-size:16px;line-height:16px}
.txt.jumbo{font-size:32px;line-height:40px}
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
.card{margin-top:6px;width:min(620px,100%);border-radius:12px;background:var(--content);box-shadow:inset 0 0 0 1px var(--border);padding:14px 16px 16px;position:relative}
.card.you::before{content:"";position:absolute;left:0;top:12px;bottom:12px;width:3px;border-radius:0 2px 2px 0;background:var(--you-bar)}
.card .ct{font-size:15px;line-height:22px;font-weight:600;color:var(--text-1)}
.card .cs{font-size:14px;line-height:22px;color:var(--text-2);margin-top:4px}
.opts{margin-top:12px;display:flex;flex-direction:column;gap:6px}
.opt{display:flex;align-items:center;gap:8px;height:40px;padding:0 12px 0 8px;border-radius:8px;background:var(--opt-bg);font-size:14px;line-height:20px}
.opt.hov{background:var(--opt-hover)}
.opt .kbd{background:none;color:var(--text-3);min-width:20px;padding:0}
.card .acts{display:flex;gap:8px;margin-top:14px}
.dl{margin-top:8px}
.dl .r{display:flex;gap:12px;padding:10px 0;font-size:14px;line-height:20px;border-bottom:0.5px solid var(--sep)}
.dl .k{width:48px;flex:none;color:var(--text-3)}
.dl .body{padding:12px 0 0;font-size:14px;line-height:22px;color:var(--text-1)}
.file{margin-top:8px;width:min(620px,100%);display:flex;align-items:center;gap:12px;padding:12px 8px 12px 12px;border-radius:12px;box-shadow:inset 0 0 0 1px var(--border);background:var(--content);position:relative;overflow:hidden}
.file .ft{width:40px;height:40px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.file .fn{font-size:14px;line-height:20px;font-weight:500}
.file .fm{font-size:12px;line-height:16px;color:var(--text-2);margin-top:2px}
.file .grow{flex:1;min-width:0}
.file .trk{position:absolute;left:12px;right:12px;bottom:6px;height:3px;border-radius:1.5px;background:var(--field);overflow:hidden}
.file .trk i{display:block;height:100%;border-radius:1.5px;background:var(--accent)}
.sendst{font-size:12px;line-height:16px;color:var(--danger-text);display:inline-flex;gap:4px;align-items:center;margin-left:8px;vertical-align:-2px}
.sendst + .btn.text{margin-left:8px;vertical-align:0}
/* Composer (§7.2) */
.cmp-wrap{flex:none;padding-top:8px;padding-bottom:16px}
.typing{height:20px;font-size:12px;line-height:20px;color:var(--text-3);padding-left:14px;margin-top:-4px;margin-bottom:4px}
.typing:empty{display:none}
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
.pop.mpop{border-radius:10px;width:320px;left:-12px;bottom:calc(100% + 21px);z-index:3;font-weight:400}
.pop.mpop .pr{height:40px}
.pop .pr{display:flex;align-items:center;gap:10px;height:36px;padding:0 10px;border-radius:6px;font-size:14px;line-height:20px;color:var(--text-1)}
.pop .pr.act{background:var(--selected)}
.pop .pr .grow{flex:1}
.pop .pr .role{font-size:13px;color:var(--text-3)}
.pop .ph{display:flex;align-items:center;height:28px;padding:0 10px;font-size:12px;line-height:16px;color:var(--text-3);font-weight:500}
/* Side panel (§8.1) */
.sp{width:var(--spw);flex:none;border-left:0.5px solid var(--sep);display:flex;flex-direction:column;background:var(--content)}
.sp-hdr{height:92px;display:flex;flex-direction:column;padding:0 10px 0 16px;flex:none;border-bottom:0.5px solid var(--sep)}
.sp-hdr .r1{height:52px;display:flex;align-items:center;gap:6px}
.sp-hdr .t{flex:1;font-size:16px;line-height:24px;font-weight:600}
.sp-hdr .r2{height:39.5px;display:flex;align-items:center;font-size:13px;line-height:20px;color:var(--text-2);padding-right:6px;min-width:0}
.sp-body{padding:0 16px}
.sp-sec{margin-top:16px}
.sp-sec:first-child{margin-top:16px}
.sp-sec .h{display:flex;align-items:center;font-size:12px;line-height:16px;font-weight:600;color:var(--text-3);height:16px;margin-bottom:8px}
.live-card{padding:4px 0 0}
.live-card .top{display:flex;gap:12px;align-items:center}
.live-card .nm{font-size:14px;line-height:20px;font-weight:600}
.live-card .st{font-size:13px;line-height:20px;color:var(--text-2)}
.plan{margin-top:10px;display:flex;flex-direction:column}
.plan .s{display:flex;gap:10px;align-items:center;height:28px;font-size:13px;line-height:20px;color:var(--text-2)}
.plan .s.ok .i{color:var(--success)}
.plan .s.now{color:var(--text-1);font-weight:500}
.plan .s.now .i{color:var(--text-1)}
.plan .s.later .i{color:var(--text-3)}
.prow{display:flex;align-items:center;gap:12px;height:48px;padding:0 8px;margin:0 -8px;border-radius:8px}
.prow .grow{flex:1;min-width:0}
.prow .a{font-size:14px;line-height:20px;font-weight:500}
.prow .b{font-size:12px;line-height:16px;color:var(--text-2)}
.prow .ft{width:32px;height:32px;border-radius:8px;background:var(--accent-soft);color:var(--accent-text);display:flex;align-items:center;justify-content:center;flex:none}
.mrow{display:flex;align-items:center;gap:10px;height:40px;font-size:14px;line-height:20px}
.mrow .grow{flex:1}
.mrow .role{display:inline-flex;align-items:center;gap:4px;font-size:13px;line-height:20px;color:var(--text-3)}
.mrow.add{margin:0 -8px;padding:0 8px;border-radius:8px;color:var(--accent-text);font-weight:500}
.mrow.add .slot{width:24px;height:24px;display:flex;align-items:center;justify-content:center;flex:none}
.start{padding:8px 0 16px;border-bottom:1px solid var(--border);margin-bottom:4px}
.start .av{margin-bottom:12px}
.start .t{font-size:22px;line-height:30px;font-weight:600;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.start .m{display:flex;align-items:center;gap:8px;margin-top:8px;font-size:13px;line-height:20px;color:var(--text-2);min-width:0}.start .m .nw{flex:none;white-space:pre;margin-left:-8px}
.start + .datediv,.start + .msg{margin-top:12px}
.overlay{position:absolute;inset:0;background:var(--scrim);z-index:20}
`;

const m = (k) => M[k];

function sidebarHtml(mode, opt = {}) {
  const sb = opt.sidebar ?? SB;
  const w = (opt.sbw ?? 248) - 66;
  const todo = sb.todo.map((t) => `<div class="todo"><span class="gl">${icon(t.icon, 16)}</span><div class="body"><div class="tt ell">${lead(esc(fit(t.title, w, 14)))}</div><div class="cx ell">${esc(fit(t.ctx, w, 12))}</div></div></div>`).join("");
  const groups = sb.groups.map((g) => {
    const trail = g.mention ? badge(g.mention) : g.muted ? `<span class="t3">${icon("bell-off", 14)}</span>` : "";
    return `<div class="row ${g.selected ? "sel" : ""} ${g.unread ? "unread" : ""} ${g.muted ? "muted" : ""}"><span class="slot">${icon("hash", 16)}</span><span class="name ell">${esc(fit(g.name, w - 6 - (trail ? 40 : 0), 14))}</span>${trail}</div>`;
  }).join("");
  const dms = sb.dms.map((d) => {
    const mm = typeof d.m === "string" ? m(d.m) : d.m;
    const trail = d.count ? nbadge(d.count) : "";
    return `<div class="row ${d.count ? "unread" : ""}"><span class="slot">${avatar(mm, 24, mode, { ring: "var(--sidebar)" })}</span><span class="name ell">${esc(fit(mm.name, w - 6 - (trail ? 40 : 0), 14))}</span>${trail}</div>`;
  }).join("");
  return `<aside class="sb"><div class="sb-top"></div>
<div class="sb-search">${icon("search", 16)}<span class="ph">搜索</span>${kbd("⌘K")}</div>
<div class="sb-scroll">
<div class="sec" style="margin-top:4px"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">待办 <span class="n">${cap(sb.todoCount ?? sb.todo.length)}</span></span></div>${todo}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">群</span><span class="ib sm add">${icon("plus", 16)}</span></div>${groups}
<div class="sec"><span class="chev">${icon("chevron-down", 12)}</span><span class="grow">私聊</span><span class="ib sm add">${icon("plus", 16)}</span></div>${dms}
</div>
<div class="sb-foot">${avatar(m("joye"), 32, mode, { ring: "var(--sidebar)" })}<div class="who"><div class="n ell">${esc(fit(sb.me ?? "Joye", (opt.sbw ?? 248) - 128, 14))}</div><div class="s">在线</div></div><span class="ib">${icon("settings", 18)}</span></div>
</aside>`;
}

function topicListHtml(mode, opt = {}) {
  const list = opt.topics ?? TOPICS;
  const base = (opt.tlw ?? 320) - 16 - 28 - 48;
  const rows = list.map((t) => {
    const unread = t.unread || t.mention;
    // One trailing element per row (§9.1): 等你… chip > @ badge > 卡住了 > other label > unread count.
    const c = chip(t.status, { you: t.you, name: t.name, row: true });
    const trail = t.you && c ? c : t.mention ? badge(`@${cap(t.mention)}`) : c ? c : t.unread ? nbadge(t.unread) : "";
    return `<div class="trow ${t.selected ? "sel" : ""} ${unread ? "unread" : ""} ${t.hover ? "hov" : ""}">${unread && !t.selected ? '<span class="dot"></span>' : ""}
${avatar(m(t.owner) ?? t.ownerM, 36, mode, { presence: false, sel: t.selected })}
<div class="body"><div class="l1"><span class="tt ell">${lead(esc(fit(t.title, base - 8 - [...t.time].length * 8, 14)))}</span><span class="tm">${esc(t.time)}</span></div>
<div class="l2"><span class="pv">${esc(`${t.who}：${t.text}`)}</span>${trail}</div></div></div>`;
  }).join("");
  const g = opt.group ?? { name: "qqbot 研发", counts: [9, 3, 2] };
  const fmt = cap;
  return `<section class="tl"><div class="hdr"><span class="ttl ell">${esc(fit(g.name, (opt.tlw ?? 320) - 76, 16))}</span><span class="ib" title="新话题 ⌘N">${icon("square-pen", 18)}</span></div>
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
  const gut = x.cont ? `<div class="gut"><span class="ht">${esc(x.time)}</span></div>` : `<div class="gut">${avatar(mm, 36, mode, { pip: !x.noPip })}</div>`;
  const jumbo = x.text && JUMBO.test(x.text) ? " jumbo" : "";
  const bar = x.hover ? `<div class="hbar"><span class="ib">${icon("face-slightly-smiling-plus", 18)}</span><span class="ib">${icon("reply", 18)}</span><span class="ib">${icon("text-quote", 18)}</span><span class="ib">${icon("ellipsis", 18)}</span></div>` : "";
  return `<div class="msg ${x.cont ? "cont" : ""} ${x.hover ? "hov" : ""}">${gut}<div class="main">${head}${x.pre ?? ""}${x.text ? `<div class="txt${jumbo}">${x.text}</div>` : ""}${x.after ?? ""}</div>${bar}</div>`;
}

/** The @ list (§7.3) hangs from the token being typed: its left edge 12 pt left of the "@",
 * its bottom 8 pt above the composer's top border. */
export const mentionPop = (mode) => `<div class="pop mpop">
<div class="ph">成员</div>
<div class="pr act">${avatar(m("scout"), 24, mode, { ring: "var(--overlay)" })}<span>Scout</span><span class="grow role">测试验收</span><span class="t3">${icon("corner-down-left", 14)}</span></div>
</div>`;

function composerHtml(mode, opt = {}) {
  const field = opt.draft ? `<div class="field">${opt.draft}<span class="caret"></span></div>` : `<div class="field"><div class="ph ell">${esc(opt.placeholder ?? "回复")}</div></div>`;
  return `<div class="cmp-wrap box"><div class="typing">${opt.typing ?? ""}</div><div class="cmp ${opt.draft ? "focus-w" : ""}">${field}
<div class="bar"><span class="ib" title="添加附件">${icon("plus", 18)}</span><span class="ib" title="提及">${icon("at-sign", 18)}</span><span class="ib" title="表情">${icon("face-slightly-smiling", 18)}</span><span class="ib" title="格式">${icon("type", 18)}</span><span class="grow"></span><span class="send ${opt.draft ? "on" : ""}">${icon("arrow-up", 18)}</span></div></div></div>`;
}

function headerHtml(mode, t, group) {
  // Two rows, 92 pt (§8.1): the title row, then a meta row (group, status, members). Its bottom
  // rule sits on the same y as the topic list's tab rule and the side panel's header rule.
  // While the ask card is on screen the header shows no 等你… label: the card is the one place
  // that asks (§9.1).
  const status = t.cardVisible ? "" : chip(t.status, { you: t.you, name: t.name, quiet: true, whoM: t.whoM, mode });
  const dot = '<span class="sepd">·</span>';
  return `<div class="tv-hdr"><div class="r1"><span class="ttl ell">${lead(esc(t.title))}</span>
<div class="acts"><span class="ib">${icon("bell", 18)}</span><span class="ib">${icon("panel-right", 18)}</span><span class="ib">${icon("ellipsis", 18)}</span></div></div>
<div class="r2"><span class="crumb ell">${icon("hash", 14)}<span class="ell">${esc(group)}</span></span>${status ? dot + status : ""}${dot}<span class="ppl">${icon("users", 14)}${t.people.length} 位成员</span></div></div>`;
}

export function mainWindow(mode, o) {
  const tv = o.topic;
  const msgs = tv.messages.map((x) => msgHtml(x, mode)).join("");
  const start = tv.start ? `<div class="start">${avatar({ ...m(tv.owner), state: "idle" }, 48, mode, { presence: false })}<div class="t">${esc(tv.title)}</div><div class="m"><span class="ell">${esc(tv.start.slice(0, tv.start.lastIndexOf(" · ")))}</span><span class="nw">${esc(tv.start.slice(tv.start.lastIndexOf(" · ")))}</span></div></div>` : "";
  const panel = o.panel ? o.panel(mode) : "";
  return `<div class="win" style="--sbw:${o.sbw ?? 248}px;--tlw:${o.tlw ?? 320}px;--spw:${o.spw ?? 300}px">
<div class="lights"><span></span><span></span><span></span></div>
${sidebarHtml(mode, o)}
${topicListHtml(mode, o)}
<main class="tv">${headerHtml(mode, tv, (o.group ?? { name: "qqbot 研发" }).name)}
<div class="tv-scroll"><div class="tv-inner box bottom">${start}${msgs}</div></div>
${composerHtml(mode, tv.composer ?? {})}</main>
${panel}
${o.overlay ? o.overlay(mode) : ""}
</div>`;
}

export { character, avatar, icon, chip, esc, m };
