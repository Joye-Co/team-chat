// Sample data for the v2 mockups. Bots are the real roster (assets/bots/roster.json).
export const M = {
  joye: { name: "Joye", online: true, role: "负责人" },
  li: { name: "李雪", online: true, role: "产品实习生" },
  chen: { name: "陈晨", online: false, role: "设计实习生" },
  wang: { name: "王一鸣", online: true, role: "前端实习生" },
  alex: { name: "Alex Chen", online: true, role: "后端实习生" },
  jojo: { name: "Jojo", char: "jojo", role: "主助手", state: "idle" },
  tata: { name: "Tata", char: "o03", role: "产品负责人", state: "idle" },
  pocket: { name: "Pocket", char: "o05", role: "幕僚长", state: "working" },
  forge: { name: "Forge", char: "o08", role: "开发", state: "success" },
  watch: { name: "Watch", char: "o11", role: "运维", state: "needs-input" },
  scout: { name: "Scout", char: "o07", role: "测试验收", state: "working" },
  ledger: { name: "Ledger", char: "h02", role: "数据", state: "idle" },
  taro: { name: "Taro", char: "h03", role: "社媒发布", state: "needs-input" },
};

export const sidebar = {
  todo: [
    { icon: "circle-question-mark", title: "夜间告警降噪：合并重复的 5xx", ctx: "qqbot 研发 · Watch 等你回答" },
    { icon: "eye", title: "周四小红书：夹夹 3.0 上线", ctx: "社媒 · Taro 等你审" },
    { icon: "at-sign", title: "登录页支持粘贴邀请码", ctx: "qqbot 研发 · 李雪 @你" },
  ],
  groups: [
    { name: "qqbot 研发", mention: 2, selected: true },
    { name: "运维告警", unread: true },
    { name: "社媒", unread: true },
    { name: "夹夹增长", palette: "sage" },
    { name: "产品周会", palette: "sand" },
    { name: "全员", muted: true },
  ],
  dms: [
    { m: "pocket", count: 0 },
    { m: "li", count: 1 },
    { m: "jojo" },
    { m: "chen" },
    { m: "alex" },
  ],
};

export const topics = [
  { title: "夜间告警降噪：合并重复的 5xx", owner: "forge", who: "Watch", text: "要在 staging 先打开合并规则吗？", time: "10:41", status: "waiting_answer", you: true, mention: 1, selected: true },
  { title: "登录页支持粘贴邀请码", owner: "wang", who: "李雪", text: "@Joye 粘贴时要不要自动去掉空格？", time: "10:20", unread: 2, mention: 1 },
  { title: "PR #128：话题列表虚拟滚动", owner: "forge", who: "Forge", text: "CI 通过，1 万条数据下滚动 60 帧", time: "09:58", status: "waiting_review", name: "Alex Chen", unread: 1 },
  { title: "staging 每日巡检", owner: "scout", who: "Scout", text: "正在检查 12 个页面…", time: "09:30", status: "in_progress" },
  { title: "WebSocket 断线重连后消息重复", owner: "alex", who: "Alex Chen", text: "复现了，是重放时没有按 seq 去重", time: "昨天", status: "in_progress" },
  { title: "DMG 签名：缺少开发者证书", owner: "watch", who: "Watch", text: "证书申请还在审核，先发未签名版本", time: "昨天", status: "blocked" },
  { title: "搜索支持中文两字词", owner: "ledger", who: "Ledger", text: "两字词召回率 96%，报告在产出里", time: "周一", status: "done" },
  { title: "新人入职手册", owner: "li", who: "陈晨", text: "我补了截图，大家看一下", time: "10/28" },
  { title: "周五 demo 安排", owner: "joye", who: "Joye", text: "每人 10 分钟，先讲结论", time: "10/25", status: "done" },
];
