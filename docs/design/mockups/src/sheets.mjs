// Spec sheets: the side panel tabs, every card block type, empty states, message and window
// states (banners, toasts, dialogs, menus) and the 产出 window. Each one is a real surface of
// the app laid out on one 1440 x 900 page so light and dark can be compared one to one.
import { avatar, chip, esc, icon, kbd, trafficLights } from "./base.mjs";
import { loginScreenshot } from "./data.mjs";
import { message } from "./main.mjs";
import { inviteSheet } from "./screens.mjs";

const sheetCss = `
.sheetpage { position: absolute; inset: 0; background: var(--window-bg); padding: 28px 32px; display: flex; flex-direction: column; gap: 18px; }
.sheetpage > header { display: flex; align-items: baseline; gap: 12px; }
.sheetpage > header h2 { font-size: 15px; line-height: 20px; font-weight: 600; }
.sheetpage > header span { font-size: 12px; color: var(--text-2); }
.cap { font-size: 11px; line-height: 14px; font-weight: 600; color: var(--text-3); margin-bottom: 8px; }
.surface { background: var(--content); border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border), 0 1px 2px rgba(0,0,0,0.04); overflow: hidden; position: relative; }
`;

/* ---------- Side panel ---------- */

export const PANEL_SHEET_CSS = sheetCss + `
.pcols { display: grid; grid-template-columns: repeat(4, 280px); gap: 24px; flex: 1; min-height: 0; }
.pcols .panel { width: 280px; flex: 1; min-height: 0; border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border); overflow: hidden; }
.pcols .panel .toolbar { height: 48px; }
.att { display: flex; align-items: center; gap: 10px; padding: 7px 6px; border-radius: 7px; }
.att .tile { width: 32px; height: 32px; border-radius: 7px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: var(--fill-field); color: var(--text-2); }
.att .tile.html { background: var(--accent-soft); color: var(--accent); }
.att b { font-weight: 500; display: block; }
.att .m { font-size: 11px; line-height: 14px; color: var(--text-2); }
.att .m.err { color: var(--danger-text); }
.att .mini { height: 3px; border-radius: 2px; background: var(--fill-pressed); margin-top: 5px; overflow: hidden; }
.att .mini i { display: block; height: 100%; background: var(--accent); }
.att .act { color: var(--text-2); font-size: 12px; display: flex; align-items: center; gap: 4px; }
.step.err .ic { color: var(--danger); }
.raw { margin: 6px 6px 0; font-size: 12px; color: var(--accent-text); display: flex; align-items: center; gap: 4px; }
.pempty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 6px; padding: 0 28px 60px; color: var(--text-2); }
.pempty .i { color: var(--text-3); margin-bottom: 4px; }
.pempty b { color: var(--text-1); font-weight: 600; font-size: 13px; }
.pempty span { font-size: 12px; line-height: 17px; }
`;

const seg = (on, a = 3) => `<div class="toolbar"><div class="seg"><span class="${on === "a" ? "on" : ""}">产出 <span class="num">${a}</span></span><span class="${on === "p" ? "on" : ""}">过程</span></div></div>`;

function stepsHtml(steps) {
  const ic = { done: "circle-check", run: "loader", wait: "circle-dashed", err: "circle-alert" };
  return steps
    .map((s) => `<div class="step ${s.state}"><span class="ic">${icon(ic[s.state], 16)}</span><div class="grow"><b>${esc(s.title)}</b><div class="m">${esc(s.meta)}</div>${s.progress ? `<div class="bar-track"><div class="bar-fill" style="width:${s.progress}%"></div></div>` : ""}</div></div>`)
    .join("");
}

export function panelSheet(mode) {
  const ring = "var(--panel)";
  const artifacts = `<aside class="panel">${seg("a", 5)}
  <div class="psec"><h4>结论</h4><div class="decision">邀请码输入框移到窗口中部并自动聚焦；粘贴时自动规整格式。<div class="m">林晓 定下 · 今天 14:10</div></div></div>
  <div class="psec"><h4>产出</h4>
    <div class="att"><span class="tile html">${icon("file-code", 16)}</span><div class="grow" style="min-width:0"><b class="ell">登录页改版方案</b><div class="m ell">HTML 页面 · 第 3 版 · 14:30</div></div></div>
    <div class="att"><span class="tile">${icon("image", 16)}</span><div class="grow" style="min-width:0"><b class="ell">login-v3-dark.png</b><div class="m ell">图片 · 1.2 MB · 14:32</div></div></div>
    <div class="att"><span class="tile">${icon("file-text", 16)}</span><div class="grow" style="min-width:0"><b class="ell">邀请码规则.md</b><div class="m ell">文件 · 4 KB · 10:31</div></div></div>
    <div class="att"><span class="tile">${icon("file", 16)}</span><div class="grow" style="min-width:0"><b class="ell">登录页录屏.mov</b><div class="m ell num">正在上传 · 38.2 / 61.5 MB</div><div class="mini"><i style="width:62%"></i></div></div><span class="act">${icon("x", 14)}</span></div>
    <div class="att"><span class="tile">${icon("image", 16)}</span><div class="grow" style="min-width:0"><b class="ell">IMG_2041…edited.HEIC</b><div class="m err ell">上传失败 · 超过 100 MB</div></div><span class="act">${icon("rotate-ccw", 14)}重试</span></div>
  </div>
  <div class="psec"><h4>谁在做什么</h4><dl class="kv">
    <dt>负责</dt><dd>${avatar("设计助手", { size: 20, bot: true, palette: "teal", mode, ring })}<span class="ell">设计助手</span></dd>
    <dt>等待</dt><dd>${avatar("Joye", { size: 20, palette: "graphite", mode, ring })}<span class="ell">你审方案 v3</span></dd>
    <dt>关注</dt><dd><span class="t2 ell">林晓、陈思远 和另外 2 位</span></dd>
  </dl></div>
</aside>`;
  const head = (name, palette, state) => `<div class="psec"><div class="row" style="gap:10px;padding:8px 6px 4px">${avatar(name, { size: 28, bot: true, palette, mode, ring })}<div class="col grow" style="min-width:0"><b style="font-weight:600" class="ell">${esc(name)}</b><span class="t2 ell" style="font-size:11px;line-height:14px">${state}</span></div></div></div>`;
  const running = `<aside class="panel">${seg("p")}
  ${head("设计助手", "teal", "正在工作 · 已用 18 分钟")}
  <div class="psec"><h4>步骤</h4><div class="steps">${stepsHtml([
    { state: "done", title: "读了话题和 2 条引用", meta: "10:12" },
    { state: "done", title: "做出方案 v1", meta: "10:31 · 产出 第 1 版" },
    { state: "done", title: "按反馈改到 v3", meta: "14:30 · 产出 第 3 版" },
    { state: "run", title: "补充暗色模式截图", meta: "2 / 3 张", progress: 66 },
    { state: "wait", title: "等你审方案 v3", meta: "完成后通知你" },
  ])}</div></div>
  <div class="psec"><div class="runbox">${icon("monitor", 14)}<span class="grow ell">运行在 Mac mini（工作室）</span><span class="num">18:04</span></div></div>
</aside>`;
  const stuck = `<aside class="panel">${seg("p", 1)}
  ${head("构建助手", "sage", `<span style="color:var(--st-blocked)">卡住了</span> · 14:52 起没有进展`)}
  <div class="psec"><h4>步骤</h4><div class="steps">${stepsHtml([
    { state: "done", title: "拉取 design/login-v3", meta: "14:31" },
    { state: "done", title: "构建预览包", meta: "14:38 · 2 分 41 秒" },
    { state: "err", title: "上传到预发环境", meta: "14:52 · 证书过期，需要账号持有人确认" },
    { state: "wait", title: "通知负责人", meta: "等待上一步" },
  ])}</div></div>
  <div class="psec" style="padding-top:8px"><div class="row" style="gap:8px;padding:0 6px"><span class="btn sm primary">我来处理</span><span class="btn sm">让它重试</span></div>
  <div class="raw">${icon("list", 13)}查看完整记录</div></div>
</aside>`;
  const empty = `<aside class="panel">${seg("a", 0)}
  <div class="pempty">${icon("layers", 20)}<b>还没有产出</b><span>成员上传的文件、页面和定下的结论会出现在这里。</span></div>
</aside>`;
  return `<div class="sheetpage">
  <header><h2>侧边面板</h2><span>280 pt，宽度切换不做动画；内容淡入 120 ms。⌥⌘0 打开或关闭。</span></header>
  <div class="pcols">
    <div class="col" style="min-height:0"><div class="cap">产出 · 有上传中和失败的项</div>${artifacts}</div>
    <div class="col" style="min-height:0"><div class="cap">过程 · 成员正在工作</div>${running}</div>
    <div class="col" style="min-height:0"><div class="cap">过程 · 卡住了</div>${stuck}</div>
    <div class="col" style="min-height:0"><div class="cap">产出 · 空</div>${empty}</div>
  </div>
</div>`;
}

/* ---------- Cards: every block type ---------- */

export const CARDS_CSS = sheetCss + `
.ccols { display: grid; grid-template-columns: repeat(3, 1fr); gap: 28px; flex: 1; min-height: 0; }
.ccol { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
.ccol .card { max-width: none; }
.blk-h { display: flex; align-items: center; gap: 8px; padding: 10px 14px 0; }
.blk-h b { font-weight: 600; flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.blk-h .gi { color: var(--text-2); display: flex; }
.blk-h .gi.bad { color: var(--danger); } .blk-h .gi.good { color: var(--success); }
.blk { padding: 10px 14px; }
.blk + .blk { padding-top: 0; }
.blk .sum { font-size: 12px; line-height: 16px; color: var(--text-2); }
.fields { display: grid; grid-template-columns: 56px 1fr 56px 1fr; gap: 6px 10px; font-size: 13px; }
.fields dt { color: var(--text-2); } .fields dd { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tbl { width: 100%; border-collapse: collapse; font-size: 12.5px; }
.tbl th { text-align: left; font-size: 11px; font-weight: 600; color: var(--text-2); padding: 0 8px 6px 0; }
.tbl td { padding: 6px 8px 6px 0; box-shadow: inset 0 0.5px 0 var(--sep); white-space: nowrap; }
.tbl td.r, .tbl th.r { text-align: right; font-variant-numeric: tabular-nums; padding-right: 0; }
.prog { display: flex; align-items: baseline; justify-content: space-between; font-size: 13px; }
.prog .num { font-size: 12px; color: var(--text-2); }
.ptrack { height: 6px; border-radius: 3px; background: var(--fill-pressed); margin-top: 8px; overflow: hidden; }
.ptrack i { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
.clist { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
.clist div { display: flex; align-items: center; gap: 8px; }
.clist .i { color: var(--text-3); } .clist .done .i { color: var(--success); } .clist .done span { color: var(--text-2); }
.acts { display: flex; gap: 8px; align-items: center; }
.result { font-size: 12px; color: var(--text-2); display: flex; align-items: center; gap: 6px; }
.result .i { color: var(--success); }
.cap-row { display: flex; align-items: center; justify-content: space-between; }
.msgdemo { background: var(--content); border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border); padding: 10px 0; }
.msgdemo .msg { padding-right: 16px; padding-left: 14px; }
.body code { font-family: Menlo, ui-monospace, monospace; font-size: 12.5px; background: var(--fill-field); border-radius: 4px; padding: 1px 4px; }
.body a { color: var(--accent-text); text-decoration: none; }
.body ul { padding-left: 18px; }
`;

const card = (inner) => `<div class="card">${inner}</div>`;

export function cardsSheet(mode) {
  const status = card(`<div class="blk-h"><span class="gi bad">${icon("circle-alert", 16)}</span><b>CI 失败 · main</b>${chip("blocked")}</div><div class="blk"><div class="sum">3 个检查里 1 个失败 · 构建助手 · 2 分钟前</div></div>`);
  const fields = card(`<div class="blk-h"><span class="gi">${icon("git-branch", 16)}</span><b>登录页改版 v3 的预览已就绪</b>${chip("waiting_review", { you: true })}</div><div class="blk"><dl class="fields"><dt>分支</dt><dd class="mono" style="font-size:12px">design/login-v3</dd><dt>改动</dt><dd><span class="num">6</span> 个文件</dd><dt>检查</dt><dd><span class="ok">${icon("circle-check", 14)}</span>全部通过</dd><dt>预览</dt><dd>预发环境</dd></dl></div><div class="blk"><div class="acts"><span class="btn primary sm">批准合并</span><span class="btn sm">要求修改</span></div></div>`);
  const table = card(`<div class="blk-h"><span class="gi">${icon("table", 16)}</span><b>本周构建耗时</b></div><div class="blk"><table class="tbl"><tr><th>分支</th><th>状态</th><th class="r">耗时</th><th class="r">次数</th></tr>
<tr><td>main</td><td>${chip("done")}</td><td class="r">2:41</td><td class="r">128</td></tr>
<tr><td>design/login-v3</td><td>${chip("in_progress")}</td><td class="r">3:05</td><td class="r">12</td></tr>
<tr><td>fix/push-cert</td><td>${chip("blocked")}</td><td class="r">—</td><td class="r">1,284</td></tr></table></div>`);
  const progress = card(`<div class="blk-h"><span class="gi">${icon("loader", 16)}</span><b>回归测试</b>${chip("in_progress")}</div><div class="blk"><div class="prog"><span>已通过 41 个</span><span class="num">41 / 58</span></div><div class="ptrack"><i style="width:70.7%"></i></div></div>`);
  const list = card(`<div class="blk-h"><span class="gi">${icon("list-checks", 16)}</span><b>发布前检查</b></div><div class="blk"><div class="clist">
<div class="done">${icon("circle-check", 15)}<span>预发环境冒烟测试</span></div>
<div class="done">${icon("circle-check", 15)}<span>更新说明已写好</span></div>
<div>${icon("circle", 15)}<span>安装包签名与公证</span></div>
<div>${icon("circle", 15)}<span>通知全员群</span></div></div></div>`);
  const code = card(`<div class="blk-h"><span class="gi bad">${icon("terminal", 16)}</span><b>失败的步骤：上传到预发环境</b></div><div class="blk"><div class="code wide"><div class="ch">日志<span class="iconbtn">${icon("copy", 14)}</span></div><pre><span class="cm"># 14:52:07</span>
error: certificate has expired (APNs production, 2026-11-02)
  at upload (scripts/deploy.ts:<span class="n">88</span>:<span class="n">11</span>)</pre></div></div>`);
  const image = card(`<div class="blk-h"><span class="gi">${icon("image", 16)}</span><b>暗色模式截图</b></div><div class="blk">${loginScreenshot(mode, 380, 190)}<div class="sum" style="margin-top:6px">2 / 3 张 · 设计助手</div></div>`);
  const actions = card(`<div class="blk-h"><span class="gi">${icon("circle-question-mark", 16)}</span><b>可以发布 v0.4.2 吗？</b>${chip("waiting_answer", { you: true })}</div><div class="blk"><div class="sum">预发环境检查都通过了。选一个，构建助手会接着做。</div></div><div class="blk"><div class="acts"><span class="btn primary sm">现在发布</span><span class="btn sm">今晚 22:00</span><span class="btn sm">先不发</span></div></div>`);
  const answered = card(`<div class="blk-h"><span class="gi good">${icon("circle-check", 16)}</span><b>可以发布 v0.4.2 吗？</b>${chip("done")}</div><div class="blk"><div class="result">${icon("circle-check", 14)}Joye 选了「现在发布」 · 14:40</div></div>`);
  const art = (ic, cls, name, meta, act = "打开") => `<div class="card art"><span class="tile"${cls ? "" : ` style="background:var(--fill-field);color:var(--text-2)"`}>${icon(ic, 20)}</span><div class="grow" style="min-width:0"><b class="ell">${name}</b><div class="m ell">${meta}</div></div><span class="btn sm">${act}</span></div>`;
  const md = `<div class="msgdemo">${message({ author: "陈思远", role: "后端", time: "14:02", palette: "sky", presence: "online", html: `<div class="quote"><b>设计助手</b> 主要把输入框移到了窗口中部。</div><p>两点：</p><ul><li>粘贴时先去掉空格和 <code>-</code> 再校验</li><li>错误提示放在输入框下方，见 <a>邀请码规则.md</a></li></ul>` }, mode)}</div>`;
  return `<div class="sheetpage">
  <header><h2>卡片和消息块</h2><span>卡片：内容底色，0.5 pt 描边，圆角 10，无阴影，最宽 460 pt。按钮最多 3 个一行，结果写回卡片。</span></header>
  <div class="ccols">
    <div class="ccol"><div><div class="cap">status</div>${status}</div><div><div class="cap">fields + actions</div>${fields}</div><div><div class="cap">table</div>${table}</div><div><div class="cap">progress</div>${progress}</div></div>
    <div class="ccol"><div><div class="cap">list</div>${list}</div><div><div class="cap">code</div>${code}</div><div><div class="cap">image</div>${image}</div></div>
    <div class="ccol"><div><div class="cap">actions · 等你回答</div>${actions}</div><div><div class="cap">actions · 已回答</div>${answered}</div><div><div class="cap">产出 · HTML 页面 / 文件</div><div class="col" style="gap:8px">${art("file-code", true, "登录页改版方案", "HTML 页面 · 第 3 版 · 14:30")}${art("file", false, "Q3 Board Deck — FINAL (r…legal].pdf", "文件 · 98.6 MB", "在访达中显示")}</div></div><div><div class="cap">消息：引用、列表、行内代码、链接</div>${md}</div></div>
  </div>
</div>`;
}

/* ---------- Empty, loading and error states ---------- */

export const EMPTY_CSS = sheetCss + `
.egrid { display: grid; grid-template-columns: repeat(4, 1fr); grid-template-rows: 1fr 1fr; gap: 20px; flex: 1; min-height: 0; }
.etile { display: flex; flex-direction: column; min-height: 0; }
.etile .surface { flex: 1; display: flex; flex-direction: column; }
.empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 6px; padding: 0 28px; }
.empty .i { color: var(--text-3); margin-bottom: 6px; }
.empty b { font-weight: 600; font-size: 13px; }
.empty span { font-size: 12px; line-height: 17px; color: var(--text-2); max-width: 240px; }
.empty .btn { margin-top: 10px; }
.sk { display: flex; flex-direction: column; gap: 6px; padding: 12px 14px; }
.sk i { display: block; height: 10px; border-radius: 4px; background: var(--fill-field); }
.sk + .sk { box-shadow: inset 0 0.5px 0 var(--sep); }
.alert { margin: 12px; padding: 10px 12px; border-radius: 8px; background: var(--fill-field); display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--text-1); }
.alert .i { color: var(--danger); }
.alert .btn { margin-left: auto; }
.fakehead { height: 44px; display: flex; align-items: center; padding: 0 14px; font-weight: 600; box-shadow: inset 0 -0.5px 0 var(--sep); flex-shrink: 0; gap: 8px; }
.fakehead .field { flex: 1; font-weight: 400; }
`;

export function emptySheet() {
  const t = (cap, head, inner) => `<div class="etile"><div class="cap">${cap}</div><div class="surface">${head ? `<div class="fakehead">${head}</div>` : ""}${inner}</div></div>`;
  const e = (ic, title, desc, btn = "") => `<div class="empty">${icon(ic, 20)}<b>${title}</b><span>${desc}</span>${btn}</div>`;
  const sk = Array.from({ length: 5 }, (_, i) => `<div class="sk"><i style="width:${[62, 48, 70, 55, 40][i]}%"></i><i style="width:${[86, 74, 80, 68, 77][i]}%;height:8px"></i></div>`).join("");
  return `<div class="sheetpage">
  <header><h2>空状态、加载和错误</h2><span>Empty 组件，居中，最宽 280 pt；一个 20 pt 线条图标，不放插图。文字来自文案目录。</span></header>
  <div class="egrid">
    ${t("还没有群（新成员第一次登录）", "", e("users-round", "还没有群", "管理员把你加进群后，群会出现在侧边栏里。", `<span class="btn sm">新建群</span>`))}
    ${t("群里还没有话题", "开发", e("message-square-plus", "这个群还没有话题", "写下第一条消息就会开始一个话题，标题默认取第一行。"))}
    ${t("待办为空", "待办", e("circle-check", "没有等你的事", "有人 @你 或话题等你回答、等你审时会出现在这里。"))}
    ${t("搜索没有结果", `<span class="field">${icon("search", 14)}<span style="color:var(--text-1)">推送证书 v2</span></span>`, e("search-x", "没有找到「推送证书 v2」", "只搜了「开发」。", `<span class="btn sm">在所有群里搜索<span class="kbd" style="margin-left:2px">⌘↵</span></span>`))}
    ${t("话题列表加载中（超过 300 ms 才显示）", "开发", sk)}
    ${t("时间线加载失败", "登录页改版", `<div class="alert">${icon("circle-alert", 15)}<span>没能加载更早的消息。</span><span class="btn sm">重试</span></div><div class="empty"></div>`)}
    ${t("过程：还没有成员在工作", "", e("activity", "还没有成员在这个话题里工作", "@ 一位自动工作的成员，它的步骤会显示在这里。"))}
    ${t("通知：没有新通知", "", e("bell", "都看过了", "只在有人 @你、话题等你、或你关注的话题有结果时通知你。"))}
  </div>
</div>`;
}

/* ---------- Message, window and feedback states ---------- */

export const STATES_CSS = sheetCss + `
.scols { display: grid; grid-template-columns: 540px 1fr 420px; gap: 28px; flex: 1; min-height: 0; }
.scol { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
.tlx { background: var(--content); border-radius: 10px; box-shadow: 0 0 0 0.5px var(--card-border); padding: 6px 0 10px; }
.tlx .msg { padding-right: 16px; padding-left: 14px; }
.pending-msg .body { opacity: 0.6; }
.mstate { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-3); margin-left: 4px; }
.fail { display: flex; align-items: center; gap: 6px; margin-top: 2px; font-size: 12px; color: var(--danger-text); }
.fail .btn { height: 20px; padding: 0 6px; font-size: 12px; color: var(--accent-text); background: none; box-shadow: none; }
.deleted { font-size: 13px; color: var(--text-3); font-style: normal; }
.emoji-only { font-size: 32px; line-height: 40px; }
.edited { font-size: 11px; color: var(--text-3); margin-left: 4px; }
.band { height: 30px; border-radius: 8px; background: var(--fill-field); display: flex; align-items: center; gap: 8px; padding: 0 10px; font-size: 12px; color: var(--text-2); }
.band.ok .i { color: var(--success); }
.toast { width: 340px; background: var(--elevated); border-radius: 12px; box-shadow: var(--shadow-popover); padding: 12px; display: flex; gap: 10px; }
.toast b { font-weight: 600; display: block; }
.toast .m { font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 2px; }
.toast .x { color: var(--text-3); }
.dlg { width: 360px; background: var(--elevated); border-radius: 14px; box-shadow: var(--shadow-dialog); padding: 18px 18px 14px; }
.dlg h3 { font-size: 15px; line-height: 20px; font-weight: 600; }
.dlg p { font-size: 13px; line-height: 19px; color: var(--text-2); margin-top: 6px; }
.dlg .foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
.btn.danger-fill { background: var(--danger); color: #fff; box-shadow: none; }
.menu { width: 220px; background: var(--elevated); border-radius: 10px; box-shadow: var(--shadow-popover); padding: 5px; }
.menu .menu-item { height: 28px; font-size: 13px; }
.menu .menu-item .kbd { margin-left: auto; background: none; color: var(--text-3); }
.menu .menu-item.danger { color: var(--danger-text); }
.menu .msep { height: 0.5px; background: var(--sep); margin: 4px 8px; }
.tip { display: inline-flex; align-items: center; gap: 8px; height: 26px; padding: 0 8px; border-radius: 6px; background: var(--elevated); box-shadow: var(--shadow-popover); font-size: 12px; }
.tip .kbd { background: var(--fill-field); }
.stage { position: relative; }
`;

export function statesSheet(mode) {
  const msgs = [
    { author: "Joye", role: "产品负责人", time: "14:40", palette: "graphite", presence: "online", html: `<p>可以，今晚发。</p>` },
  ].map((m) => message(m, mode)).join("");
  const pending = `<div class="msg cont pending-msg"><span class="gut"></span><div class="grow"><div class="body"><p>发之前把更新说明再过一遍<span class="mstate">${icon("clock", 11)}发送中</span></p></div></div></div>`;
  const failed = `<div class="msg cont"><span class="gut"></span><div class="grow"><div class="body"><p>截图我放到产出里了。</p></div><div class="fail">${icon("circle-alert", 13)}发送失败<span class="btn">重试</span><span class="btn" style="color:var(--text-2)">删除</span></div></div></div>`;
  const edited = message({ author: "林晓", role: "前端", time: "14:42", palette: "rose", presence: "online", html: `<p>暗色截图对过了，没问题。<span class="edited">（已编辑）</span></p>` }, mode);
  const deleted = `<div class="msg cont"><span class="gut"></span><div class="grow"><div class="body deleted">此消息已删除</div></div></div>`;
  const emoji = message({ author: "王磊", role: "iOS", time: "14:43", palette: "clay", html: `<p class="emoji-only">👍🎉</p>` }, mode);
  return `<div class="sheetpage">
  <header><h2>消息状态、提示和弹出层</h2><span>发送中、失败、编辑、删除；断网与重连；通知；确认对话框；菜单；工具提示；邀请码弹窗。</span></header>
  <div class="scols">
    <div class="scol">
      <div><div class="cap">消息状态</div><div class="tlx">${msgs}${pending}${failed}${edited}${deleted}${emoji}</div></div>
      <div><div class="cap">横条（输入框上方与侧边栏顶部）</div><div class="col" style="gap:8px">
        <div class="band">${icon("wifi-off", 14)}<span>网络已断开。新消息会在连上后发出。</span></div>
        <div class="band">${icon("loader", 14)}<span>正在重新连接…</span></div>
        <div class="band ok">${icon("circle-check", 14)}<span>已重新连接，补齐了 12 条消息</span></div>
      </div></div>
    </div>
    <div class="scol">
      <div><div class="cap">通知（Notification，右上角，组件内置 400 ms 进入）</div><div class="toast">${avatar("林晓", { size: 32, palette: "rose", mode, ring: "var(--elevated)" })}<div class="grow" style="min-width:0"><b>林晓 @了你</b><div class="m ell">开发 · 登录页改版</div><div class="m">暗色截图对过了，你看一下能不能合并？</div></div><span class="x">${icon("x", 14)}</span></div></div>
      <div><div class="cap">确认对话框（AlertDialog）</div><div class="dlg"><h3>停用 周报助手？</h3><p>停用后它不再接收消息，也不会出现在 @ 列表里。它的话题和产出会保留，随时可以重新启用。</p><div class="foot"><span class="btn">取消</span><span class="btn danger-fill">停用</span></div></div></div>
      <div><div class="cap">工具提示（首次 500 ms，相邻立即）</div><div class="row" style="gap:12px"><span class="tip">侧边面板 ${kbd("⌥", "⌘", "0")}</span><span class="tip">新话题 ${kbd("⌘", "N")}</span></div></div>
    </div>
    <div class="scol">
      <div><div class="cap">消息菜单（ContextMenu）</div><div class="menu">
        <div class="menu-item">${icon("text-quote", 14)}引用<span class="kbd">R</span></div>
        <div class="menu-item active">${icon("at-sign", 14)}提及作者<span class="kbd">@</span></div>
        <div class="menu-item">${icon("copy", 14)}复制<span class="kbd">⌘C</span></div>
        <div class="menu-item">${icon("pencil", 14)}编辑<span class="kbd">E</span></div>
        <div class="msep"></div>
        <div class="menu-item danger">${icon("trash", 14)}删除…<span class="kbd">⌫</span></div>
      </div></div>
      <div><div class="cap">邀请码弹窗（Dialog，FadeSlide 180 ms）</div>${inviteSheet()}</div>
    </div>
  </div>
</div>`;
}

/* ---------- 产出 window ---------- */

export const ARTWIN_CSS = `
.artwin { left: 150px; top: 70px; width: 1140px; height: 760px; background: var(--content); flex-direction: column; }
.aw-bar { height: 52px; display: flex; align-items: center; gap: 12px; padding: 0 12px 0 20px; flex-shrink: 0; box-shadow: inset 0 -0.5px 0 var(--sep); }
.aw-bar .t { font-size: 15px; font-weight: 600; }
.aw-bar .sub { font-size: 11px; color: var(--text-2); }
.aw-bar .sel { display: inline-flex; align-items: center; gap: 6px; height: 26px; padding: 0 8px; border-radius: 7px; background: var(--control); box-shadow: var(--shadow-control); font-size: 12px; }
.aw-lock { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-2); }
.aw-body { flex: 1; display: flex; min-height: 0; }
.aw-page { flex: 1; background: #f5f5f7; display: flex; align-items: center; justify-content: center; }
[data-theme="dark"] .aw-page { background: #f5f5f7; }
.aw-vers { width: 260px; flex-shrink: 0; background: var(--panel); box-shadow: inset 0.5px 0 0 var(--sep); padding: 8px 12px; }
.aw-vers h4 { font-size: 11px; font-weight: 600; color: var(--text-3); padding: 8px 6px 6px; }
.ver { display: flex; gap: 10px; padding: 8px 6px; border-radius: 7px; align-items: flex-start; }
.ver.sel { background: var(--fill-selected); }
.ver b { font-weight: 500; display: block; }
.ver .m { font-size: 11px; line-height: 15px; color: var(--text-2); }
.ver .note { font-size: 12px; line-height: 17px; color: var(--text-2); margin-top: 2px; }
`;

export function artifactWindow(mode) {
  const vers = [
    { v: 3, who: "设计助手", t: "今天 14:30", note: "粘贴自动规整；错误提示移到输入框下方", sel: true },
    { v: 2, who: "设计助手", t: "今天 13:50", note: "输入框移到窗口中部" },
    { v: 1, who: "设计助手", t: "今天 10:31", note: "第一版" },
  ];
  return `<div class="desk"></div>
<div class="floatwin artwin">
  <div class="aw-bar">${trafficLights()}<div class="col grow" style="min-width:0;margin-left:12px"><span class="t ell">登录页改版方案</span><span class="sub ell">开发 · 登录页改版 · HTML 页面</span></div>
    <span class="aw-lock" title="这个页面不能访问网络">${icon("lock", 12)}离线页面</span>
    <span class="sel">第 3 版${icon("chevrons-up-down", 12)}</span>
    <span class="capsule"><span class="iconbtn">${icon("copy", 16)}</span><span class="iconbtn">${icon("download", 16)}</span><span class="iconbtn on">${icon("panel-right", 16)}</span></span>
  </div>
  <div class="aw-body">
    <div class="aw-page">${loginScreenshot("light", 720, 450)}</div>
    <aside class="aw-vers"><h4>版本</h4>${vers.map((x) => `<div class="ver${x.sel ? " sel" : ""}">${avatar(x.who, { size: 24, bot: true, palette: "teal", mode, ring: x.sel ? "var(--panel)" : "var(--panel)" })}<div class="grow" style="min-width:0"><b>第 ${x.v} 版${x.sel ? `<span class="t2" style="font-weight:400;font-size:11px;margin-left:6px">当前</span>` : ""}</b><div class="m">${x.who} · ${x.t}</div><div class="note">${x.note}</div></div></div>`).join("")}</aside>
  </div>
</div>`;
}
