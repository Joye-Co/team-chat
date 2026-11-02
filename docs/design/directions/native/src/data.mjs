// Mockup content: a realistic day of the team, and the break-ui worst case.
import { esc, icon } from "./base.mjs";

const m = (name, cls = "") => `<span class="mention ${cls}">@${esc(name)}</span>`;

/** A screenshot of the current login screen, drawn as an image the way a member would paste it. */
export function loginScreenshot(mode, w = 360, h = 225) {
  const bg = mode === "dark" ? "#2a2a2d" : "#f3f3f5";
  const win = mode === "dark" ? "#323236" : "#ffffff";
  const ink = mode === "dark" ? "rgba(255,255,255,0.8)" : "rgba(0,0,0,0.78)";
  const soft = mode === "dark" ? "rgba(255,255,255,0.14)" : "rgba(0,0,0,0.09)";
  const faint = mode === "dark" ? "rgba(255,255,255,0.35)" : "rgba(0,0,0,0.35)";
  return `<svg class="img" width="${w}" height="${h}" viewBox="0 0 360 225" xmlns="http://www.w3.org/2000/svg">
  <rect width="360" height="225" fill="${bg}"/>
  <rect x="80" y="14" width="200" height="197" rx="10" fill="${win}" stroke="${soft}"/>
  <circle cx="94" cy="26" r="3.5" fill="#ff5f57"/><circle cx="105" cy="26" r="3.5" fill="#febc2e"/><circle cx="116" cy="26" r="3.5" fill="#28c840"/>
  <rect x="164" y="48" width="32" height="32" rx="8" fill="#0a7aff"/>
  <text x="180" y="98" text-anchor="middle" font-size="10" font-weight="600" fill="${ink}" font-family="-apple-system, PingFang SC">欢迎使用</text>
  <rect x="100" y="112" width="160" height="5" rx="2.5" fill="${soft}"/>
  <rect x="120" y="123" width="120" height="5" rx="2.5" fill="${soft}"/>
  <rect x="96" y="160" width="168" height="18" rx="5" fill="none" stroke="${soft}"/>
  <text x="104" y="172" font-size="7.5" fill="${faint}" font-family="ui-monospace, Menlo">XXXX-XXXX-XXXX</text>
  <rect x="96" y="184" width="168" height="16" rx="5" fill="#0a7aff" opacity="0.9"/>
  <rect x="88" y="152" width="184" height="54" rx="7" fill="none" stroke="#ff3b30" stroke-width="1.5" stroke-dasharray="4 3"/>
</svg>`;
}

export function mainData(mode) {
  return {
    me: { name: "Joye", role: "产品负责人", palette: "graphite" },
    todo: 3,
    groups: [
      { name: "开发", palette: "sky", sel: true },
      { name: "林晓", dm: true, presence: "online", palette: "rose", unread: 2 },
      { name: "运维", palette: "teal", mention: 1 },
      { name: "构建助手", dm: true, bot: true, presence: "online", palette: "sage" },
      { name: "产品", palette: "sand", unread: 5 },
      { name: "全员", palette: "slate", unread: 12 },
      { name: "陈思远", dm: true, presence: "online", palette: "sky" },
      { name: "设计评审", palette: "clay", muted: true, mutedIcon: true },
      { name: "王磊", dm: true, palette: "clay" },
      { name: "周报助手", dm: true, bot: true, palette: "graphite" },
    ],
    group: { name: "开发", sub: "8 位成员 · 14 个话题" },
    topics: [
      { title: "登录页改版", time: "14:32", status: "waiting_review", chip: { you: true }, preview: "设计助手：v3 的变化：邀请码输入框移到…", sel: true },
      { title: "发布 v0.4.2", time: "14:05", status: "waiting_answer", chip: { you: true }, preview: "构建助手：预发环境检查都通过了，可以发布吗？", unread: true, mention: 1 },
      { title: "iOS 推送证书过期", time: "13:58", status: "blocked", preview: "王磊：Apple 后台需要账号持有人确认", unread: true, count: 3 },
      { title: "消息同步偶尔丢一条", time: "13:40", status: "in_progress", preview: "构建助手：已复现，正在写回归测试", unread: true, count: 1 },
      { title: "搜索支持两个字的词", time: "11:20", status: "waiting_answer", chip: { name: "陈思远" }, preview: "你：分词用 ngram 还是词典？" },
      { title: "图片超过 100 MB 上传失败", time: "昨天", status: "done", preview: "林晓：已修复，改成了分片上传" },
      { title: "周会纪要 11/2", time: "昨天", preview: "Joye：这周重点是稳定性和安装引导" },
      { title: "代码块在暗色模式下对比度不够", time: "周一", status: "done", preview: "设计助手：已按系统配色调整" },
      { title: "安装引导写给新同学", time: "周一", status: "waiting_review", chip: { name: "林晓" }, preview: "陈思远：初稿放在产出里了" },
    ],
    topic: {
      title: "登录页改版",
      status: "waiting_review",
      chip: { you: true },
      sub: "林晓 发起 · 设计助手 负责 · 今天 10:12",
      people: [
        { name: "林晓", palette: "rose" },
        { name: "设计助手", bot: true, palette: "teal" },
        { name: "陈思远", palette: "sky" },
      ],
    },
    messages: [
      { kind: "start", text: "林晓 在「开发」发起了这个话题 · 今天 10:12" },
      {
        author: "林晓", role: "前端", time: "10:12", palette: "rose", presence: "online",
        html: `<p>登录页要改一版：现在邀请码输入框太靠下，新同学第一次打开找不到。${m("设计助手")} 帮忙出一个方案，参考系统设置的样式。</p>`,
      },
      { kind: "ack", author: "设计助手", palette: "teal", text: "已接手，开始做方案，大约 20 分钟。", time: "10:12" },
      {
        author: "林晓", role: "前端", time: "11:40", palette: "rose", presence: "online",
        html: `<p>对比一下现在的样子，红框是新同学找不到的地方：</p><div class="blk">${loginScreenshot(mode, 320, 200)}</div>`,
      },
      {
        author: "设计助手", role: "界面与交互设计", time: "13:50", bot: true, palette: "teal",
        html: `<p>方案 v2 放在这里了，主要把输入框移到了窗口中部。</p><div class="blk card art"><span class="tile">${icon("file-code", 20)}</span><div class="grow" style="min-width:0"><b class="ell">登录页改版方案</b><div class="m ell">HTML 页面 · 第 2 版 · 13:50</div></div><span class="btn sm">打开</span></div>`,
      },
      {
        author: "陈思远", role: "后端", time: "14:02", palette: "sky", presence: "online", hover: true,
        html: `<div class="quote"><b>设计助手</b> 主要把输入框移到了窗口中部。</div><p>${m("设计助手")} 邀请码 12 位分三段，输入框要能直接粘贴带横线或空格的格式。服务端是这样规整的：</p>`,
      },
      {
        cont: true,
        html: `<div class="code"><div class="ch">TypeScript<span class="iconbtn">${icon("copy", 14)}</span></div><pre><span class="k">export function</span> <span class="fn">normalizeInviteCode</span>(raw: <span class="ty">string</span>): <span class="ty">string</span> {
  <span class="k">return</span> raw.<span class="fn">replace</span>(<span class="s">/[\\s-]/g</span>, <span class="s">""</span>).<span class="fn">toUpperCase</span>();
}</pre></div>`,
      },
      { kind: "ack", author: "设计助手", palette: "teal", text: "收到，改到 v3。", time: "14:03" },
      { kind: "divider" },
      {
        author: "构建助手", role: "构建与发布", time: "14:31", bot: true, palette: "sage",
        html: `<div class="card sc"><div class="sh">${icon("git-branch", 16)}<b class="ell">登录页改版 v3 的预览已就绪</b><span class="chip st-you">等你审</span></div>
<dl><dt>分支</dt><dd class="mono" style="font-size:12px">design/login-v3</dd><dt>改动</dt><dd><span class="num">6</span> 个文件</dd><dt>检查</dt><dd><span class="ok">${icon("circle-check", 14)}</span>全部通过</dd><dt>预览</dt><dd>预发环境</dd></dl>
<div class="acts"><span class="btn primary sm">批准合并</span><span class="btn sm">要求修改</span></div></div>`,
      },
      {
        author: "设计助手", role: "界面与交互设计", time: "14:32", bot: true, palette: "teal",
        html: `<p>v3 改好了：粘贴带横线或空格的邀请码会自动规整成三段；错误提示放在输入框下方，不再弹窗。</p><p>正在补充暗色截图<span class="stream-caret"></span></p><div class="streaming">${icon("loader", 13)}<span class="shimmer">正在输出</span></div>`,
      },
      { author: "林晓", role: "前端", time: "14:33", palette: "rose", presence: "online", html: `<p>截图出来后我来对一下暗色。</p>` },
    ],
    typing: "陈思远 正在输入…",
    draft: `看过了，整体可以。合并前再请 <span class="mention">@助</span><span class="caret"></span>`,
    mentionList: Object.assign(
      [
        { name: "设计助手", role: "界面与交互设计", bot: true, palette: "teal", hl: "助" },
        { name: "构建助手", role: "构建与发布", bot: true, palette: "sage", hl: "助" },
        { name: "周报助手", role: "整理周报", bot: true, palette: "graphite", hl: "助" },
      ],
      { left: 214 },
    ),
    artifacts: [
      { name: "登录页改版方案", meta: "HTML 页面 · 第 3 版 · 14:30", icon: "file-code", kind: "html" },
      { name: "login-v3-dark.png", meta: "图片 · 1.2 MB · 14:32", icon: "image" },
      { name: "邀请码规则.md", meta: "文件 · 4 KB · 10:31", icon: "file-text" },
    ],
    process: [
      { state: "done", title: "读了话题和 2 条引用", meta: "10:12" },
      { state: "done", title: "做出方案 v1", meta: "10:31 · 产出 第 1 版" },
      { state: "done", title: "按反馈改到 v3", meta: "14:30 · 产出 第 3 版" },
      { state: "run", title: "补充暗色模式截图", meta: "2 / 3 张", progress: 66 },
      { state: "wait", title: "等你审方案 v3", meta: "完成后通知你" },
    ],
  };
}

export function worstData(mode) {
  const d = mainData(mode);
  const longName = "Aleksandra Wiśniewska-Kowalczyk";
  d.me = { name: "Christopher Alexander Montgomery III", role: "高级产品设计工程师（平台基础设施与开发者体验）", palette: "graphite" };
  d.todo = 1284;
  d.groups = [
    { name: "2026 Q4 移动端与桌面端统一登录及账号体系重构专项小组", palette: "sky", sel: true, unread: 0 },
    { name: longName, dm: true, presence: "online", palette: "rose", unread: 1284 },
    { name: "J", dm: true, presence: "online", palette: "sand", mention: 150 },
    { name: "王", dm: true, palette: "clay", unread: 1 },
    { name: "超长名字的自动部署与回滚监控值守助手", dm: true, bot: true, presence: "online", palette: "sage", unread: 99 },
    { name: "🦊 Fox 狐狸", dm: true, palette: "teal" },
    { name: "Benachrichtigungseinstellungen", palette: "slate", mention: 7, unread: 999 },
    { name: "全员", palette: "slate", unread: 1000 },
    { name: "运维", palette: "teal", muted: true, mutedIcon: true },
    { name: "نور الهدى عبد الرحمن", dm: true, palette: "sky" },
  ];
  d.offline = true;
  d.group = { name: "2026 Q4 移动端与桌面端统一登录及账号体系重构专项小组", sub: "1,284 位成员 · 10,000 个话题" };
  d.topics = [
    { title: "fix/ios-push-cert-renewal-and-apns-p8-key-rotation-for-production-environment", time: "14:32", status: "waiting_review", chip: { name: longName }, preview: `${longName}：https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments`, sel: true, count: 1284 },
    { title: "J", time: "14:31", status: "waiting_answer", chip: { you: true }, preview: "J：?", unread: true, mention: 150 },
    { title: "（无标题）", time: "14:30", status: "in_progress", preview: "超长名字的自动部署与回滚监控值守助手：已接手", unread: true, count: 99 },
    { title: "把 2,000 字的需求说明贴进来之后标题会怎样显示，标题默认取第一行消息的全部内容", time: "11月2日", status: "blocked", preview: "王：👍👍👍", count: 1 },
    { title: "Paramètres de confidentialité et de sécurité 隐私与安全设置翻译", time: "2025年12月31日", status: "done", preview: "🦊 Fox 狐狸：<script>alert(1)</script> 原样显示了".replace(/</g, "&lt;") },
    { title: "等回答", time: "10月3日", status: "waiting_answer", preview: "（这个话题还没有消息）" },
  ];
  d.topic = {
    title: "fix/ios-push-cert-renewal-and-apns-p8-key-rotation-for-production-environment",
    status: "waiting_review",
    chip: { name: longName },
    sub: `${longName} 发起 · 超长名字的自动部署与回滚监控值守助手 负责 · 1,284 位成员`,
    people: [
      { name: longName, palette: "rose" },
      { name: "J", palette: "sand" },
      { name: "超长名字的自动部署与回滚监控值守助手", bot: true, palette: "sage" },
    ],
  };
  d.messages = [
    {
      author: "Christopher Alexander Montgomery III", role: "高级产品设计工程师（平台基础设施与开发者体验）", time: "14:20", palette: "graphite",
      html: `<p>${m(longName)} 证书在这里：https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments&amp;filter=unresolved 还有 bartholomew.fitzgerald@northwind-industries-holdings.example.com</p>`,
    },
    { kind: "ack", author: "超长名字的自动部署与回滚监控值守助手", palette: "sage", text: "已接手。这一行很长，在窄窗口里只显示一行并以省略号结束，完整内容在悬停提示里。", time: "14:21" },
    { author: "J", role: "", time: "14:22", palette: "sand", presence: "online", html: `<p>?</p>` },
    { author: "王", role: "iOS", time: "14:23", palette: "clay", html: `<p>👍👍👍</p>` },
    {
      author: longName, role: "Senior Product Design Engineer, Platform Infrastructure", time: "14:25", palette: "rose", presence: "online",
      html: `<div class="code wide"><div class="ch">Shell<span class="iconbtn">${icon("copy", 14)}</span></div><pre>openssl pkcs12 -in AuthKey_9F8E7D6C5B.p12 -out apns-production-2026-11-03-renewed.pem -nodes -clcerts -legacy</pre></div>
<div class="blk card art"><span class="tile">${icon("file", 20)}</span><div class="grow" style="min-width:0"><b class="ell" title="Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf">Q3 Board Deck — FINAL (revised) v…legal].pdf</b><div class="m ell">文件 · 98.6 MB · 第 12 版</div></div><span class="btn sm">打开</span></div>`,
    },
    { kind: "divider" },
    {
      author: "超长名字的自动部署与回滚监控值守助手", role: "构建与发布", time: "14:31", bot: true, palette: "sage",
      html: `<div class="card sc"><div class="sh">${icon("triangle-alert", 16)}<b class="ell">生产环境证书将在 0 天后过期，自动续期失败，需要账号持有人在 Apple 后台确认</b><span class="chip st-blocked">卡住了</span></div>
<dl><dt>证书</dt><dd class="mono" style="font-size:12px" title="9f8e7d6c-5b4a-4c3d-8e2f-1a0b9c8d7e6f">9f8e7d6c-5b4a-4c3d-8e2f-1a0b9c8d7e6f</dd><dt>失败</dt><dd><span class="num">1,284</span> 次</dd><dt>负责</dt><dd>${esc(longName)}</dd><dt>耗时</dt><dd><span class="num">1,284</span> 小时</dd></dl>
<div class="acts"><span class="btn primary sm">我来处理</span><span class="btn sm">稍后提醒</span><span class="btn sm">查看日志</span></div></div>`,
    },
  ];
  d.typing = `${longName}、J 和另外 1,282 位正在输入…`;
  d.draft = `<span style="display:block;max-height:154px;overflow:hidden">粘贴了一段很长的需求说明。输入框最多长到 7 行，再多就在框内滚动，发送按钮和工具栏始终可见。${"第二段：说明包含很多内容，用来测试换行与行高，以及中英文混排 mixed English text 和数字 1,284 的对齐。".repeat(4)}</span>`;
  d.mentionList = null;
  d.draftEmpty = false;
  d.artifacts = [
    { name: "Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf", meta: "文件 · 98.6 MB · 第 12 版 · 2025年12月31日", icon: "file" },
    { name: "IMG_20250914_183022_HDR_portrait_edited_edited.HEIC", meta: "图片 · 0 B · 上传失败", icon: "image" },
    { name: "J", meta: "HTML 页面 · 第 1 版", icon: "file-code", kind: "html" },
  ];
  return d;
}
