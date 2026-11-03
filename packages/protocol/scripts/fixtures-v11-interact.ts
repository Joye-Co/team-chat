// 1.1 fixtures of the interaction loop (Card v2.1 composition, events, patches, answers and
// card state), 引用提问 (part refs), diagrams, the 产出 bridge (page events, data, replies)
// and media metadata. Called by gen-fixtures.ts. Names follow V11-INTERACT §8.4, V11-REFS §C3
// and V11-CARD21 §9, with the reconciliation of V11-INC R1-R5 (wire name `decision`, one
// `card.event` frame, `PartRef`, `table.rowKeys`, page events not stored).
import { cardV2ToV1, MAX_CARD_V2_BLOCKS } from "../src/index.ts";
import type { CardV2 } from "../src/index.ts";

type Obj = Record<string, unknown>;
export type InteractCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string, reason?: string) => void;
  T0: number;
  ID: Record<string, string>;
  base: Obj;
  message: Obj;
};

export function addInteractFixtures({ ok, bad, T0, ID, base, message }: InteractCtx) {
  const G = ID.general as string;
  const T = ID.topic as string;
  const M = ID.msg as string;
  const LEDGER = ID.bot as string;
  const JOYE = ID.owner as string;
  const ART = "01JC0ART00000000000000OCT3";
  const FILE = "01JC0FIL0000000000000INDEX";
  const EV = (n: number) => `01JC0EV00000000000000000${String(n).padStart(2, "0")}`;
  const CEV = (n: number) => `01JC0CEV0000000000000000${String(n).padStart(2, "0")}`;

  // -------------------------------------------------------------------------
  // Card v2.1: composition (V11-INTERACT §7.1, §7.2)
  // -------------------------------------------------------------------------
  const report: CardV2 = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "done",
    title: "10 月数据月报",
    summary: "新增群 38 个，比 9 月多 12%；日活 2,340。",
    fallback: "Ledger 发了 10 月数据月报：新增群 38，日活 2,340。请看一眼数据对不对。",
    data: { weeks: ["10-06", "10-13", "10-20", "10-27"] },
    ask: { id: "verify", from: ["requester"], blocking: false, promise: "你确认之前，我不会把月报发到全员群。" },
    blocks: [
      { id: "kpis", type: "row", children: ["m_groups", "m_dau", "m_reply"] },
      { id: "m_groups", type: "metric", label: "新增群", value: 38, delta: 4, better: "up", note: "比 9 月" },
      { id: "m_dau", type: "metric", label: "日活", value: 2340, delta: 90, better: "up", note: "比 9 月" },
      { id: "m_reply", type: "metric", label: "平均回复", value: 1.2, unit: "秒", delta: -0.3, better: "down", tone: "good", note: "比 9 月" },
      { id: "main", type: "row", weights: [2, 1], children: ["trend", "check_col"] },
      {
        id: "trend",
        type: "chart",
        kind: "line",
        label: "日活 · 近 4 周",
        emphasis: "high",
        x: ["10-06", "10-13", "10-20", "10-27"],
        series: [
          { id: "oct", name: "10 月", values: [2210, 2268, 2301, 2340] },
          { id: "sep", name: "9 月", values: [2150, 2190, 2236, 2250] },
        ],
        summary: "10 月日活从 2,210 涨到 2,340，每周都比 9 月高。",
        events: ["chart.select"],
      },
      { id: "check_col", type: "stack", children: ["check", "note"] },
      {
        id: "check",
        type: "decision",
        label: "这个数据对不对？",
        required: true,
        options: [
          { id: "ok", label: "对" },
          { id: "wrong", label: "有问题" },
        ],
        deliver: "onSubmit",
        events: ["choice.change"],
      },
      { id: "note", type: "input", kind: "textarea", label: "补充一句（可选）", required: false, maxLength: 300 },
      { id: "full", type: "ref", emphasis: "low", label: "10 月数据月报 v3", target: { kind: "artifact", artifactId: ART, version: 3 } },
    ],
    actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "note"] }],
  };
  ok("entities/card-v2-report-compose.json", "CardV2", report, "V11-INTERACT 7.1 rev 1: rows, a stack, a live chart and a non-blocking ask");

  const drill: CardV2 = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 2,
    intent: "info",
    title: "10 月新增群从哪来",
    fallback: "Ledger：10 月新增群 38 个，小红书最多（14 个）。",
    blocks: [
      {
        id: "bars",
        type: "chart",
        kind: "bar",
        label: "新增群 · 按渠道",
        x: ["小红书", "群内邀请", "B 站", "抖音"],
        series: [{ id: "groups", name: "新增群", values: [14, 10, 9, 5] }],
        summary: "小红书 14、群内邀请 10、B 站 9、抖音 5。",
        events: ["chart.select", "chart.clear"],
        emitBy: "members",
      },
      { id: "detail", type: "group", title: "小红书 · 14 个新增群", children: ["detail_row", "d_posts"] },
      { id: "detail_row", type: "row", weights: [1, 2], children: ["d_metric", "d_weeks"] },
      { id: "d_metric", type: "metric", label: "小红书新增群", value: 14, delta: 6, better: "up" },
      { id: "d_weeks", type: "chart", kind: "bar", label: "每周", x: ["10-06", "10-13", "10-20", "10-27"], series: [{ name: "新增群", values: [2, 3, 4, 5] }], summary: "每周都在涨，10-27 这周 5 个。" },
      {
        id: "d_posts",
        type: "table",
        columns: ["笔记", "带来的群"],
        rows: [
          ["群相册一键分享回群", "6"],
          ["周末徒步怎么建群", "5"],
          ["生日会照片整理", "3"],
        ],
        rowKeys: ["p1", "p2", "p3"],
        select: "multiple",
        sortable: true,
        events: ["table.rowSelect"],
      },
    ],
  };
  ok("entities/card-v2-drilldown.json", "CardV2", drill, "V11-INTERACT 7.2 rev 2: group > row > leaves (depth 2)");

  const tabsCompare: CardV2 = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "ask",
    title: "夹夹 4.0 首页用哪个方案？",
    fallback: "Pixel 做了两个首页方案，请选一个。",
    ask: { from: [JOYE], blocking: true },
    blocks: [
      {
        id: "views",
        type: "tabs",
        tabs: [
          { id: "pick", label: "选方案", children: ["cmp"] },
          { id: "data", label: "数据", children: ["usage"] },
        ],
        selected: "pick",
        events: ["tab.open"],
      },
      {
        id: "cmp",
        type: "compare",
        question: "首页用哪个？",
        options: [
          { id: "A", label: "按活动整理", pros: ["找照片最快"], cons: ["每个活动要先归类"], preview: { fileId: FILE }, recommended: true, children: ["a_note"] },
          { id: "B", label: "按时间排", pros: ["做起来最快"], cons: ["照片一多就难找"], preview: { artifactId: ART, version: 2 } },
        ],
        note: { label: "说说为什么", maxLength: 300 },
        deliver: "immediate",
      },
      { id: "a_note", type: "text", text: "5 位群主都提到要按活动找照片。" },
      { id: "usage", type: "metric", label: "按活动找照片的人", value: 62, unit: "%", events: ["metric.press"] },
    ],
  };
  ok("entities/card-v2-tabs-compare.json", "CardV2", tabsCompare, "compare with option children inside tabs; compare commits on pick");

  const variants: CardV2 = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "info",
    title: "2.1 的块",
    fallback: "每种 2.1 块一个。",
    blocks: [
      { id: "rank", type: "rank", label: "先做哪个？", items: [{ id: "photos", label: "群相册" }, { id: "share", label: "分享回群" }, { id: "birthday", label: "生日提醒", meta: "3 票" }], cut: { after: 2, label: "前 2 个进 4.0" }, fallbackText: "排序题：群相册、分享回群、生日提醒" },
      { id: "rate", type: "rating", label: "首页清楚吗？", max: 5, labels: { "1": "很乱", "5": "很清楚" }, fallbackText: "1 到 5 分" },
      { id: "nps", type: "scale", label: "会推荐给朋友吗？", min: 0, max: 10, ends: ["不会", "一定会"], fallbackText: "0 到 10 分" },
      { id: "go", type: "confirm", label: "今晚发布？", yes: "可以", no: "不行", detail: "所有群立刻用上新版本。", fallbackText: "可以 / 不行" },
      { id: "img", type: "imageChoice", label: "用哪张封面？", options: [{ id: "a", label: "封面 A", fileId: FILE }, { id: "b", label: "封面 B", fileId: FILE, recommended: true }], fallbackText: "两张封面选一张" },
      { id: "mx", type: "matrix", label: "每项进哪个版本？", rows: [{ id: "photos", label: "群相册", required: true }, { id: "birthday", label: "生日提醒" }], columns: [{ id: "v40", label: "4.0" }, { id: "v41", label: "4.1" }], fallbackText: "两项各选一个版本" },
      { id: "when", type: "slots", label: "什么时候评审？", tz: "Asia/Shanghai", multiple: true, slots: [{ id: "t1", start: T0, end: T0 + 1_800_000 }, { id: "t2", start: T0 + 3_600_000, end: T0 + 5_400_000, conflicts: [ID.alice as string] }], fallbackText: "两个时间段" },
      { id: "vote", type: "poll", label: "团建去哪？", options: [{ id: "A", label: "爬山" }, { id: "B", label: "密室" }], voters: "group", closesAt: T0 + 86_400_000, public: true, changeable: true, fallbackText: "投票：爬山 / 密室" },
      { id: "more", type: "buttons", buttons: [{ id: "by_channel", label: "按渠道拆开" }, { id: "weekly", label: "换成按周看" }, { id: "reset", label: "清空", style: "danger", confirm: { title: "清空筛选？" } }], events: ["button.press"], fallbackText: "按渠道拆开 · 换成按周看" },
      { id: "page", type: "embed", artifactId: ART, version: 1, height: 360, title: "渠道预算分配", events: ["budget.submit"], fallbackText: "Ledger 做了一个预算分配页面，请在 TeamChat 1.1 里打开。" },
      { id: "flow", type: "diagram", syntax: "mermaid", source: "flowchart LR\n  a[提问] --> b[回答]", title: "流程", alt: "提问之后回答", fallbackText: "图：提问、回答" },
      { id: "pct", type: "input", kind: "slider", label: "灰度比例", min: 0, max: 100, step: 5, unit: "%", default: 10, events: ["input.change"] },
    ],
  };
  ok("entities/card-v2-21-variants.json", "CardV2", variants, "every 2.1 leaf: rank rating scale confirm imageChoice matrix slots poll buttons embed diagram and the slider input");

  const questionnaire: CardV2 = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "ask",
    title: "实习反馈",
    fallback: "三道题，一起提交。",
    ask: { from: ["owners"] },
    blocks: [
      { id: "qs", type: "questionnaire", children: ["q1", "q2", "q3"], submitLabel: "一起提交", anonymous: true },
      { id: "q1", type: "rating", label: "带教清楚吗？", max: 5, required: true, fallbackText: "1 到 5 分" },
      { id: "q2", type: "decision", label: "想留下吗？", options: [{ id: "y", label: "想" }, { id: "n", label: "不想" }], allowOther: { label: "其他", placeholder: "说说看" } },
      { id: "q3", type: "input", kind: "textarea", label: "还想说什么", maxLength: 500 },
    ],
  };
  ok("entities/card-v2-questionnaire.json", "CardV2", questionnaire, "a questionnaire with three input children");
  ok("entities/card-v2-group-children.json", "CardV2", {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "working",
    title: "本周进展",
    fallback: "开发 1 个做完、1 个在做。",
    blocks: [
      { id: "dev", type: "group", title: "开发", summary: "1 个做完 · 1 个在做", collapsible: true, collapsed: false, children: ["l1", "pct"] },
      { id: "l1", type: "list", items: [{ text: "登录页", status: "done" }, { text: "相册", status: "in_progress" }] },
      { id: "pct", type: "progress", value: 0.5 },
    ],
  }, "a 2.1 group names its children by id");
  ok("entities/card-v2-unknown-event-name.json", "CardV2", {
    ...drill,
    blocks: drill.blocks.map((b) => (b.id === "bars" ? { ...b, events: ["chart.select", "chart.zoom"] } : b)),
  }, "an unknown event name is kept and ignored by readers");
  ok("entities/card-v2-table-row-keys.json", "CardV2", {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "info",
    title: "10 月数据月报",
    fallback: "渠道表",
    blocks: [
      { id: "channels", type: "table", title: "渠道表", columns: ["渠道", "新增群"], rows: [["小红书", "14"], ["B 站", "9"]], rowKeys: ["xhs", "bili"], select: "single" },
    ],
  }, "R4: table rowKeys (one key per row) is the row-key mechanism");
  ok("entities/card-v2-diagram.json", "CardV2", {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "info",
    title: "发布流程",
    fallback: "发布流程图",
    blocks: [
      {
        id: "flow",
        type: "diagram",
        syntax: "mermaid",
        source: "flowchart TD\n  build[构建] --> test[测试]\n  test --> ship[发布]",
        title: "发布流程",
        caption: "测试通过才发布",
        alt: "构建，然后测试，然后发布",
        fallbackText: "图：构建、测试、发布",
      },
    ],
  });

  // Invalid cards
  const leaf = (id: string) => ({ id, type: "text", text: id });
  const card21 = (blocks: Obj[], extra: Obj = {}): Obj => ({ v: 2, catalog: "tc.card/2.1", rev: 1, intent: "info", title: "t", fallback: "f", blocks, ...extra });
  bad("entities/card-v2-depth-4.json", "CardV2", card21([
    { id: "g0", type: "group", title: "0", children: ["s1"] },
    { id: "s1", type: "stack", children: ["s2"] },
    { id: "s2", type: "stack", children: ["s3"] },
    { id: "s3", type: "stack", children: ["x"] },
    leaf("x"),
  ]), "card.depth: a container at depth 3", "card.depth");
  bad("entities/card-v2-row-4-children.json", "CardV2", card21([{ id: "r", type: "row", children: ["a", "b", "c", "d"] }, leaf("a"), leaf("b"), leaf("c"), leaf("d")]), "a row has 2..3 children", "card.limits");
  bad("entities/card-v2-row-in-row.json", "CardV2", card21([{ id: "r1", type: "row", children: ["r2", "c"] }, { id: "r2", type: "row", children: ["a", "b"] }, leaf("a"), leaf("b"), leaf("c")]), "row inside a row", "card.limits");
  bad("entities/card-v2-tabs-in-row.json", "CardV2", card21([
    { id: "r", type: "row", children: ["tb", "c"] },
    { id: "tb", type: "tabs", tabs: [{ id: "t1", label: "一", children: ["a"] }, { id: "t2", label: "二", children: ["b"] }] },
    leaf("a"),
    leaf("b"),
    leaf("c"),
  ]), "tabs inside a row", "card.limits");
  bad("entities/card-v2-shared-child.json", "CardV2", card21([{ id: "s1", type: "stack", children: ["a"] }, { id: "s2", type: "stack", children: ["a"] }, leaf("a")]), "a child has two parents", "card.limits");
  bad("entities/card-v2-cycle.json", "CardV2", card21([leaf("x"), { id: "s1", type: "stack", children: ["s2"] }, { id: "s2", type: "stack", children: ["s1"] }]), "containers in a cycle", "card.depth");
  bad("entities/card-v2-style-key.json", "CardV2", card21([{ id: "a", type: "text", text: "红字", color: "red" }]), "card.style_property: color", "card.style_property");
  bad("entities/card-v2-two-primary.json", "CardV2", card21([leaf("a")], { actions: [{ id: "x", label: "X", style: "primary" }, { id: "y", label: "Y", style: "primary" }] }), "two primary actions", "card.limits");
  bad("entities/card-v2-two-decision-surfaces.json", "CardV2", card21([
    { id: "cmp", type: "compare", options: [{ id: "A", label: "A" }, { id: "B", label: "B" }] },
    { id: "qs", type: "questionnaire", children: ["q1", "q2"] },
    { id: "q1", type: "input", kind: "text", label: "一" },
    { id: "q2", type: "input", kind: "text", label: "二" },
  ], { intent: "ask", ask: { from: ["owners"] } }), "card.decision_surfaces: a compare and a questionnaire for one ask", "card.decision_surfaces");
  bad("entities/card-v2-immediate-on-multiple.json", "CardV2", card21([{ id: "c", type: "decision", multiple: true, deliver: "immediate", options: [{ id: "a", label: "A" }, { id: "b", label: "B" }] }]), "immediate on a multiple choice", "card.limits");
  bad("entities/card-v2-81-blocks.json", "CardV2", card21(Array.from({ length: MAX_CARD_V2_BLOCKS + 1 }, (_, i) => ({ type: "divider", id: `d${i}` }))), "card.limits: 81 blocks", "card.limits");
  bad("entities/card-v2-secret-input.json", "CardV2", card21([{ id: "pw", type: "input", kind: "secret", label: "密码" }]), "card.secret_deferred: no secret inputs in 1.1.0", "card.secret_deferred");
  bad("entities/card-v2-unknown-catalog.json", "CardV2", card21([leaf("a")], { catalog: "tc.card/2.2" }), "card.unknown_catalog", "card.unknown_catalog");
  bad("entities/card-v2-21-block-in-20.json", "CardV2", { v: 2, rev: 1, intent: "info", title: "t", fallback: "f", blocks: [{ id: "s", type: "stack", children: ["a"] }, leaf("a")] }, "card.unknown_block: stack needs tc.card/2.1", "card.unknown_block");
  bad("entities/card-v2-unknown-block.json", "CardV2", card21([{ id: "m", type: "map", fallbackText: "地图" }]), "card.unknown_block: a type no catalog has", "card.unknown_block");
  bad("entities/card-v2-rank-no-fallback.json", "CardV2", card21([{ id: "r", type: "rank", label: "排", items: [{ id: "a", label: "A" }, { id: "b", label: "B" }] }]), "2.1 leaves need fallbackText", "card.limits");
  bad("entities/card-v2-decision-unknown-default.json", "CardV2", card21([{ id: "c", type: "decision", options: [{ id: "a", label: "A" }, { id: "b", label: "B" }], default: ["z"] }]), "default names an unknown option", "card.limits");
  bad("entities/card-v2-decision-min-max.json", "CardV2", card21([{ id: "c", type: "decision", multiple: true, minSelect: 3, maxSelect: 2, options: [{ id: "a", label: "A" }, { id: "b", label: "B" }, { id: "c", label: "C" }] }]), "minSelect > maxSelect", "card.limits");
  bad("entities/card-v2-rating-max-4.json", "CardV2", card21([{ id: "r", type: "rating", label: "分", max: 4, fallbackText: "分" }]), "rating max is always 5", "card.limits");
  bad("entities/card-v2-scale-12-steps.json", "CardV2", card21([{ id: "s", type: "scale", label: "分", min: 0, max: 11, fallbackText: "分" }]), "scale spans at most 11 steps", "card.limits");
  bad("entities/card-v2-matrix-one-column.json", "CardV2", card21([{ id: "m", type: "matrix", label: "选", rows: [{ id: "a", label: "A" }], columns: [{ id: "x", label: "X" }], fallbackText: "选" }]), "matrix has 2..5 columns", "card.limits");
  bad("entities/card-v2-poll-7-options.json", "CardV2", card21([{ id: "p", type: "poll", label: "投", voters: "group", closesAt: T0, options: Array.from({ length: 7 }, (_, i) => ({ id: `o${i}`, label: `${i}` })), fallbackText: "投" }]), "poll has 2..6 options", "card.limits");
  bad("entities/card-v2-compare-5-options.json", "CardV2", card21([{ id: "c", type: "compare", options: Array.from({ length: 5 }, (_, i) => ({ id: `o${i}`, label: `${i}` })) }]), "compare has 2..4 options", "card.limits");
  bad("entities/card-v2-group-13-children.json", "CardV2", card21([{ id: "g", type: "group", title: "多", children: Array.from({ length: 13 }, (_, i) => `x${i}`) }, ...Array.from({ length: 13 }, (_, i) => leaf(`x${i}`))]), "group has 1..12 children", "card.limits");
  bad("entities/card-v2-questionnaire-container-child.json", "CardV2", card21([
    { id: "qs", type: "questionnaire", children: ["s", "q"] },
    { id: "s", type: "stack", children: ["a"] },
    { id: "q", type: "input", kind: "text", label: "一" },
    leaf("a"),
  ]), "questionnaire children are leaves", "card.limits");
  bad("entities/card-v2-chart-6-series.json", "CardV2", card21([{ id: "c", type: "chart", kind: "line", series: Array.from({ length: 6 }, (_, i) => ({ name: `s${i}`, values: [i] })) }]), "a chart has 1..5 series (V11-INTERACT §2.5)", "card.limits");
  bad("entities/card-v2-embed-too-tall.json", "CardV2", card21([{ id: "e", type: "embed", artifactId: ART, version: 1, height: 600, fallbackText: "页面" }]), "embed height is 160..480", "card.limits");
  bad("entities/card-v2-table-row-keys-mismatch.json", "CardV2", card21([{ id: "t", type: "table", columns: ["a"], rows: [["1"], ["2"]], rowKeys: ["only"] }]), "one row key per row", "card.limits");
  bad("entities/card-v2-diagram-too-long.json", "CardV2", card21([{ id: "d", type: "diagram", syntax: "mermaid", source: `flowchart LR\n${"  a --> b\n".repeat(1700)}`, fallbackText: "图" }]), "diagram source over 16384 bytes", "card.limits");
  bad("entities/card-v2-diagram-no-fallback.json", "CardV2", card21([{ id: "d", type: "diagram", syntax: "mermaid", source: "flowchart LR\n  a --> b" }]), "a diagram needs fallbackText", "card.limits");
  bad("entities/card-v2-diagram-click.json", "CardV2", card21([{ id: "d", type: "diagram", syntax: "mermaid", source: "flowchart LR\n  a --> b\n  click a callback", fallbackText: "图" }]), "click and callback are refused", "card.limits");
  bad("entities/card-v2-three-diagrams.json", "CardV2", card21(["d1", "d2", "d3"].map((id) => ({ id, type: "diagram", syntax: "mermaid", source: "gantt\n  title x", fallbackText: "图" }))), "at most 2 diagrams per card", "card.limits");

  // Derived v1 cards (CARD21 §7): the derivation of 2.1 cards is a valid v1 card.
  ok("entities/card-v1-derived-compose.json", "Card", cardV2ToV1(report), "v1 card derived from the 7.1 report");
  ok("entities/card-v1-derived-compare.json", "Card", cardV2ToV1(tabsCompare), "v1 card derived from tabs with a compare");
  ok("entities/card-v1-derived-variants.json", "Card", cardV2ToV1(variants), "v1 card derived from the 2.1 leaves (confirm, matrix, diagram as code)");

  // Card state, parts, refs
  const cause = { eventId: EV(1), actorId: JOYE, at: T0 + 1000, event: "choice.change", blockId: "check" };
  const answered = {
    ask: { id: "verify", status: "answered", from: [JOYE], blocking: false, answeredBy: JOYE, answeredAt: T0 + 5000 },
    answers: [
      {
        answerId: "01JC0AN0000000000000000001",
        askId: "verify",
        eventId: EV(3),
        actorId: JOYE,
        at: T0 + 5000,
        rev: 3,
        event: "action.press",
        actionId: "send",
        inputs: { check: { value: "wrong" }, what: { value: "后台是 7 个，差 2 个" } },
      },
    ],
    lastCause: cause,
  };
  ok("entities/card-state-answered.json", "CardState", answered);
  ok("entities/card-state-poll-public.json", "CardState", {
    ask: { id: "a1", status: "open", from: [JOYE, ID.alice], blocking: false, expiresAt: T0 + 86_400_000 },
    answers: [{ answerId: "01JC0AN0000000000000000002", askId: "a1", eventId: EV(4), actorId: ID.alice, at: T0, rev: 1, event: "choice.submit", blockId: "vote", value: { value: "B" } }],
    tally: { A: 0, B: 1 },
    tallyBy: { A: [], B: [ID.alice] },
  }, "a public poll: counts and voters");
  ok("entities/card-state-poll-private.json", "CardState", { ask: { id: "a1", status: "open", from: [JOYE], blocking: false }, answers: [], tally: { A: 2, B: 1 } }, "a private poll: counts only");
  ok("entities/part-ref-cell.json", "PartRef", { kind: "cell", keys: { row: "bili", col: 1 } });
  ok("entities/part-ref-unknown-kind.json", "PartRef", { kind: "frame", keys: { t: 12 } }, "open set: readers treat it as the whole block");
  bad("entities/part-ref-five-keys.json", "PartRef", { kind: "cell", keys: { a: 1, b: 2, c: 3, d: 4, e: 5 } }, "at most 4 keys");
  bad("entities/part-ref-upper-key.json", "PartRef", { kind: "cell", keys: { Row: 1 } }, "keys are [a-z]");

  const cellRef = {
    messageId: M,
    cardRev: 1,
    blockId: "channels",
    part: { kind: "cell", keys: { row: "bili", col: 1 } },
    label: "10 月数据月报 · 渠道表 · B 站 · 新增群 9",
    value: { row: "B 站", column: "新增群", display: "9", raw: 9 },
  };
  const nodeRef = { messageId: M, blockId: "flow", part: { kind: "node", keys: { node: "test" } }, label: "发布流程 · 测试", value: { node: "test", label: "测试", diagram: "flowchart" } };
  ok("entities/block-ref-input.json", "BlockRefInput", cellRef);
  ok("entities/block-ref.json", "BlockRef", { ...cellRef, sourceAuthorId: LEDGER, sourceTopicId: T });
  ok("entities/message-block-refs.json", "Message", {
    ...message,
    body: "> ↳ 10 月数据月报 · 渠道表 · B 站 · 新增群 9\n> ↳ 发布流程 · 测试\n\n@Ledger 为什么比上月少了 1 个？",
    blockRefs: [
      { ...cellRef, sourceAuthorId: LEDGER, sourceTopicId: T },
      { ...nodeRef, sourceAuthorId: LEDGER, sourceTopicId: T },
    ],
  }, "body starts with one quote line per ref");
  ok("entities/message-block-refs-deleted.json", "Message", {
    ...message,
    body: "> ↳ 原消息已删除\n\n为什么？",
    blockRefs: [{ messageId: M, blockId: "channels", part: { kind: "cell", keys: { row: "bili", col: 1 } }, label: "", sourceTopicId: T, deleted: true }],
  }, "the source was deleted: label empty, no value");
  ok("entities/message-cardv2.json", "Message", {
    ...message,
    authorId: LEDGER,
    kind: "card",
    body: report.fallback,
    card: cardV2ToV1(report),
    cardV2: { ...report, rev: 3 },
    cardState: answered,
    cause: { eventId: EV(2), actorId: JOYE, at: T0 + 3000, event: "table.rowSelect", blockId: "by_channel" },
  }, "a card message with its server-owned state and cause");
  ok("entities/card-v2-diagram-unknown-syntax.json", "Message", {
    ...message,
    authorId: LEDGER,
    kind: "card",
    body: "图",
    card: { v: 1, title: "图", blocks: [{ type: "code", lang: "plantuml", text: "@startuml\n@enduml" }] },
    cardV2: card21([{ id: "d", type: "diagram", syntax: "plantuml", source: "@startuml\n@enduml", fallbackText: "图" }]),
  }, "readers keep an unknown diagram syntax (shown as code)");

  // Media
  const video = {
    kind: "video",
    mime: "video/mp4",
    bytes: 48_000_000,
    width: 1920,
    height: 1080,
    duration: 312.5,
    posterFileId: FILE,
    frames: [{ t: 0, fileId: FILE }, { t: 60, fileId: FILE, label: "登录" }],
    chapters: [{ t: 0, title: "开头" }, { t: 120, title: "演示" }],
    transcriptFileId: FILE,
    preview: "ready",
  };
  ok("entities/media-video.json", "MediaInfo", video);
  ok("entities/media-code.json", "MediaInfo", { kind: "code", mime: "text/x-rust", bytes: 4200, lines: 180, language: "rust", focus: { fromLine: 10, toLine: 24 }, preview: "pending" });
  ok("entities/artifact-version-media-events.json", "ArtifactVersion", {
    artifactId: ART,
    version: 1,
    entry: "index.html",
    files: [{ path: "index.html", fileId: FILE, size: 12_000, mime: "text/html" }],
    createdBy: LEDGER,
    createdAt: T0,
    events: [
      { name: "breakdown", kind: "ask", label: "按渠道筛选", schema: { type: "object", required: ["channel"], properties: { channel: { enum: ["all", "xhs", "douyin"] } } } },
      { name: "report.problem", kind: "emit", label: "数据有问题" },
    ],
    media: { kind: "html", mime: "text/html", bytes: 12_000, posterFileId: FILE, preview: "ready" },
    dataRev: 2,
  }, "declared events with kind and schema, media metadata and the data rev");
  ok("entities/file-ref-media.json", "FileRef", {
    fileId: FILE,
    name: "demo.mp4",
    size: 48_000_000,
    mime: "video/mp4",
    sha256: "a".repeat(64),
    media: video,
  }, "a message attachment with media metadata (R9)");
  ok("entities/audit-card-event.json", "AuditEntry", {
    id: "01JC0AUD0000000000000000A1",
    at: T0,
    actorId: JOYE,
    credential: { kind: "token" },
    action: "card.event",
    target: { kind: "message", id: M },
    groupId: G,
    details: { eventId: EV(1), blockId: "check", event: "choice.change", rev: 1, value: { value: "wrong" } },
  }, "live card events are audited, not stored");

  // -------------------------------------------------------------------------
  // Client frames
  // -------------------------------------------------------------------------
  const ce = { t: "card.event", rid: "r101", groupId: G, messageId: M, rev: 1 };
  ok("client/card.event-choice-change.json", "ClientFrame", { ...ce, clientEventId: CEV(1), blockId: "check", event: "choice.change", value: { value: "wrong" } });
  ok("client/card.event-chart-select.json", "ClientFrame", {
    ...ce,
    clientEventId: CEV(2),
    blockId: "bars",
    event: "chart.select",
    value: { series: "groups", x: "小红书", y: 14, index: 0 },
    part: { kind: "bar", keys: { series: "groups", x: "小红书" } },
  }, "R3: a chart point event carries the part");
  ok("client/card.event-submit-inputs.json", "ClientFrame", {
    ...ce,
    rev: 3,
    clientEventId: CEV(4),
    actionId: "send",
    event: "action.press",
    inputs: { check: { value: "wrong" }, by_channel: { values: ["douyin"] }, what: { value: "后台是 7 个，差 2 个" } },
  });
  ok("client/card.event-questionnaire.json", "ClientFrame", {
    ...ce,
    clientEventId: CEV(5),
    actionId: "submit",
    event: "action.press",
    inputs: { q1: { value: 4 }, q2: { value: "other", other: "看情况" }, q3: { value: "很好" } },
  }, "a questionnaire answer holds every child");
  ok("client/card.event-retract.json", "ClientFrame", { ...ce, clientEventId: CEV(6), event: "answer.retract", value: { answerId: "01JC0AN0000000000000000001" } });
  bad("client/card.event-both-block-and-action.json", "ClientFrame", { ...ce, clientEventId: CEV(7), blockId: "check", actionId: "send", event: "action.press" }, "exactly one of blockId and actionId");
  bad("client/card.event-value-too-big.json", "ClientFrame", { ...ce, clientEventId: CEV(8), blockId: "check", event: "input.change", value: { value: "x".repeat(4100) } }, "value over 4 KiB");
  ok("client/card.event-metric-press.json", "ClientFrame", { ...ce, clientEventId: CEV(11), blockId: "m_dau", event: "metric.press", part: { kind: "metric" } });
  bad("client/card.event-part-on-button.json", "ClientFrame", { ...ce, clientEventId: CEV(12), blockId: "act", event: "button.press", value: { id: "x" }, part: { kind: "cell", keys: { row: "bili" } } }, "part only with chart.select and metric.press (V11-INC R3)");
  bad("client/card.event-chart-select-metric-part.json", "ClientFrame", { ...ce, clientEventId: CEV(13), blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-13", y: 2268, index: 1 }, part: { kind: "metric" } }, "chart.select names a point or a bar");
  bad("client/card.event-inputs-on-change.json", "ClientFrame", { ...ce, clientEventId: CEV(9), blockId: "check", event: "choice.change", inputs: { check: { value: "ok" } } }, "inputs only with action.press");
  ok("client/card.answer-short.json", "ClientFrame", { t: "card.answer", rid: "r102", groupId: G, messageId: M, rev: 3, actionId: "send", inputs: { check: { value: "ok" } }, clientEventId: CEV(10) }, "short form of card.event action.press");
  ok("client/card.answer.retract.json", "ClientFrame", { t: "card.answer.retract", rid: "r103", groupId: G, messageId: M, answerId: "01JC0AN0000000000000000001" }, "short form of card.event answer.retract");
  ok("client/card.patch-ops.json", "ClientFrame", {
    t: "card.patch",
    rid: "b7",
    groupId: G,
    messageId: M,
    baseRev: 1,
    cause: EV(1),
    clientPatchId: "01JC0PAT000000000000000001",
    ops: [
      { op: "insert", after: "main", block: { id: "drill", type: "group", title: "按渠道拆开", collapsible: false, children: [] } },
      { op: "insert", parent: "drill", block: { id: "by_channel", type: "table", columns: ["渠道", "新增群"], rows: [["小红书", "14"], ["抖音", "5"]], rowKeys: ["xhs", "douyin"], select: "multiple", events: ["table.rowSelect"] } },
      { op: "set", id: "m_groups", props: { value: 40, note: "已改正" } },
      { op: "replace", id: "note", block: { id: "note", type: "input", kind: "textarea", label: "哪一项不对？", maxLength: 300 } },
      { op: "remove", id: "full" },
      { op: "data", path: "/weeks/0", value: "10-07" },
      { op: "data", path: "/old" },
      { op: "card", props: { summary: "新增群 40 个。", ask: { id: "verify", from: ["requester"], blocking: false }, actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "by_channel", "note"] }] } },
      { op: "card", props: { ask: null } },
    ],
  }, "all six ops; data without value deletes; ask null closes the ask");
  bad("client/card.patch-empty-ops.json", "ClientFrame", { t: "card.patch", rid: "b8", groupId: G, messageId: M, baseRev: 1, ops: [] }, "1..50 ops");
  bad("client/card.patch-set-type.json", "ClientFrame", { t: "card.patch", rid: "b9", groupId: G, messageId: M, baseRev: 1, ops: [{ op: "set", id: "a", props: { type: "chart" } }] }, "set cannot change the type");
  bad("client/card.patch-two-placements.json", "ClientFrame", { t: "card.patch", rid: "b10", groupId: G, messageId: M, baseRev: 1, ops: [{ op: "insert", after: "a", index: 0, block: leaf("z") }] }, "at most one placement");
  ok("client/interaction.resolve.json", "ClientFrame", { t: "interaction.resolve", rid: "b11", groupId: G, eventId: EV(1), result: "ignored", note: "没有 10-14 之前的数" });
  bad("client/interaction.resolve-patched.json", "ClientFrame", { t: "interaction.resolve", rid: "b12", groupId: G, eventId: EV(1), result: "patched" }, "members resolve only as ignored or failed");
  ok("client/artifact.event-ask.json", "ClientFrame", { t: "artifact.event", rid: "r40", groupId: G, artifactId: ART, version: 1, clientEventId: "01JC0AEV000000000000000001", name: "breakdown", kind: "ask", payload: { channel: "douyin" } });
  ok("client/artifact.event-emit.json", "ClientFrame", { t: "artifact.event", rid: "r41", groupId: G, artifactId: ART, version: 1, clientEventId: "01JC0AEV000000000000000002", name: "report.problem", kind: "emit", payload: { channel: "douyin", note: "后台是 7 个" } });
  ok("client/artifact.data.json", "ClientFrame", { t: "artifact.data", rid: "b41", groupId: G, artifactId: ART, version: 1, baseDataRev: 1, cause: EV(21), clientPushId: "01JC0PSH000000000000000001", data: { asOf: "2026-10-31", channels: { all: 40, douyin: 7 } } });
  bad("client/artifact.data-too-big.json", "ClientFrame", { t: "artifact.data", rid: "b42", groupId: G, artifactId: ART, version: 1, data: { s: "x".repeat(262_200) } }, "data over 256 KiB");
  ok("client/artifact.reply-ok.json", "ClientFrame", { t: "artifact.reply", rid: "b40", groupId: G, eventId: EV(20), ok: true, payload: { series: [1, 1, 1, 2] } });
  ok("client/artifact.reply-declined.json", "ClientFrame", { t: "artifact.reply", rid: "b43", groupId: G, eventId: EV(20), ok: false, error: { code: "declined", message: "没有这个渠道的数" } });
  bad("client/artifact.reply-ok-with-error.json", "ClientFrame", { t: "artifact.reply", rid: "b44", groupId: G, eventId: EV(20), ok: true, error: { code: "failed", message: "x" } }, "ok: true takes no error");
  ok("client/hello-features.json", "ClientFrame", {
    t: "hello",
    protocol: 1,
    client: { name: "desktop", version: "1.1.0" },
    cursors: { [G]: 42 },
    features: ["interact/1", "refs/1"],
    supportedCatalogs: ["tc.card/2.0", "tc.card/2.1"],
  });
  ok("client/send-cardv2-cause.json", "ClientFrame", {
    t: "send",
    rid: "b50",
    groupId: G,
    topicId: T,
    clientMsgId: "01JC0CMI000000000000000001",
    body: report.fallback,
    cardV2: report,
    causeEventId: EV(21),
  }, "a new card that answers an event");
  ok("client/edit-cardv2-base-rev.json", "ClientFrame", {
    t: "edit",
    rid: "b51",
    groupId: G,
    messageId: M,
    body: report.fallback,
    cardV2: { ...report, rev: 2 },
    baseRev: 1,
    causeEventId: EV(1),
  }, "a full replace checked against baseRev");
  bad("client/send-cardv2-style-key.json", "ClientFrame", {
    t: "send",
    rid: "b52",
    groupId: G,
    clientMsgId: "01JC0CMI000000000000000002",
    body: "x",
    cardV2: card21([{ id: "a", type: "metric", label: "x", value: 1, fontSize: 20 }]),
  }, "card.style_property: fontSize");

  // References to a part (REFS §C3)
  const send = { t: "send", rid: "r60", groupId: G, topicId: T, clientMsgId: "01JC0CMI000000000000000010", body: "@Ledger 为什么比上月少了 1 个？", mentions: [LEDGER] };
  ok("client/send-block-refs.json", "ClientFrame", { ...send, blockRefs: [cellRef] }, "a cell of a v2 table");
  ok("client/send-block-refs-artifact.json", "ClientFrame", {
    ...send,
    blockRefs: [{ artifact: { artifactId: ART, version: 1 }, blockId: "page", part: { kind: "element", keys: { selector: "#chart > g:nth-of-type(2)" } }, label: "10 月数据看板 v1 · 抖音", value: { channel: "douyin", count: 5 } }],
  }, "an element of a 产出 page");
  ok("client/send-block-refs-card.json", "ClientFrame", { ...send, blockRefs: [{ messageId: M, cardRev: 1, blockId: "card", label: "10 月数据月报" }] }, "the whole card");
  ok("client/send-block-refs-md.json", "ClientFrame", { ...send, blockRefs: [{ messageId: M, blockId: "md:3", part: { kind: "row", keys: { row: 2 } }, label: "Ledger · 表格 · 抖音 · 整行" }, { messageId: M, blockId: "v1:0", label: "部署 · 第 1 行" }] }, "a Markdown table row and a v1 card block");
  ok("client/send-block-refs-unknown-kind.json", "ClientFrame", { ...send, blockRefs: [{ messageId: M, blockId: "clip", part: { kind: "frame", keys: { t: 12 } }, label: "演示 · 0:12" }] }, "an unknown part kind is kept");
  bad("client/send-block-refs-too-many.json", "ClientFrame", { ...send, blockRefs: Array.from({ length: 6 }, () => cellRef) }, "at most 5 refs");
  bad("client/send-block-refs-both-sources.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, artifact: { artifactId: ART, version: 1 } }] }, "exactly one of messageId and artifact");
  bad("client/send-block-refs-no-source.json", "ClientFrame", { ...send, blockRefs: [{ blockId: "channels", label: "x" }] }, "a ref needs a source");
  bad("client/send-block-refs-label-too-long.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, label: "长".repeat(121) }] }, "label over 120 code points");
  bad("client/send-block-refs-label-newline.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, label: "第一行\n第二行" }] }, "label with a newline");
  bad("client/send-block-refs-value-too-big.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, value: { s: "x".repeat(2100) } }] }, "value over 2048 bytes");
  bad("client/send-block-refs-bad-block-id.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, blockId: "md:x" }] }, "md: needs an index");
  bad("client/send-block-refs-bad-v1-index.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, blockId: "v1:-1" }] }, "v1: needs an index 0..49");
  bad("client/send-block-refs-bad-keys.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, part: { kind: "cell", keys: { a: 1, b: 2, c: 3, d: 4, e: 5 } } }] }, "at most 4 keys");
  bad("client/send-block-refs-upper-key.json", "ClientFrame", { ...send, blockRefs: [{ ...cellRef, part: { kind: "cell", keys: { Row: 1 } } }] }, "keys are [a-z]");
  ok("http/post-message-block-refs.json", "PostMessageRequest", { clientMsgId: "01JC0CMI000000000000000011", topicId: T, body: "为什么？", blockRefs: [cellRef] });

  // -------------------------------------------------------------------------
  // Server frames, acks and stored events
  // -------------------------------------------------------------------------
  ok("http/card-event-ack.json", "CardEventAck", { eventId: EV(1), stored: false, expect: { by: LEDGER, mode: "live", etaMs: 2000 } });
  ok("http/card-event-ack-answer.json", "CardEventAck", { eventId: EV(3), stored: true, answerId: "01JC0AN0000000000000000001", duplicate: true, expect: { by: LEDGER, mode: "run", etaMs: 10_000 } });
  ok("http/card-patch-ack.json", "CardPatchAck", { message: { ...message, authorId: LEDGER, kind: "card", body: report.fallback, cardV2: { ...report, rev: 2 }, cause }, rev: 2 });
  ok("http/artifact-event-ack.json", "ArtifactEventAck", { eventId: EV(20), expect: { by: LEDGER, mode: "live" } });
  ok("http/artifact-data-ack.json", "ArtifactDataAck", { dataRev: 2 });
  ok("server/welcome-features.json", "ServerFrame", {
    t: "welcome",
    protocol: 1,
    me: { id: JOYE, kind: "human", name: "Joye", level: "owner" },
    members: [],
    groups: [],
    serverTime: T0,
    features: ["interact/1", "refs/1", "diagram/1"],
  });
  ok("server/ack-error-reason.json", "ServerFrame", { t: "ack", rid: "r104", ok: false, error: { code: "conflict", message: "the ask was answered", reason: "card.already_answered" } }, "R8: the reason, no new ErrorCode");
  ok("server/ack-card-event.json", "ServerFrame", { t: "ack", rid: "r101", ok: true, result: { eventId: EV(1), stored: false, expect: { by: LEDGER, mode: "live", etaMs: 2000 } } });

  const ev = (seq: number, actorId: string) => ({ ...base, seq, actorId });
  ok("events/card.answered-2.1.json", "Event", {
    ...ev(60, JOYE),
    type: "card.answered",
    data: {
      messageId: M,
      answerId: "01JC0AN0000000000000000001",
      askId: "verify",
      eventId: EV(3),
      rev: 3,
      event: "action.press",
      actionId: "send",
      inputs: { check: { value: "wrong" }, what: { value: "后台是 7 个，差 2 个" } },
    },
  });
  ok("events/card.answered-immediate.json", "Event", {
    ...ev(61, JOYE),
    type: "card.answered",
    data: { messageId: M, answerId: "01JC0AN0000000000000000003", askId: "a1", eventId: EV(5), rev: 1, event: "compare.pick", blockId: "cmp", value: { value: "A" } },
  }, "the commit of an immediate block: no actionId");
  ok("events/card.retracted.json", "Event", { ...ev(62, JOYE), type: "card.retracted", data: { messageId: M, answerId: "01JC0AN0000000000000000001", askId: "verify", eventId: EV(6) } });
  ok("events/message.updated-cardv2-cause.json", "Event", {
    ...ev(63, LEDGER),
    type: "message.updated",
    data: {
      messageId: M,
      body: report.fallback,
      editedAt: T0 + 2000,
      streaming: false,
      card: cardV2ToV1(report),
      cardV2: { ...report, rev: 2 },
      cardState: { ask: { id: "verify", status: "open", from: [JOYE], blocking: false }, answers: [], lastCause: cause },
      cause,
    },
  }, "a patch result: the full card, its state and the cause");
  ok("events/message.updated-block-refs.json", "Event", {
    ...ev(64, JOYE),
    type: "message.updated",
    data: { messageId: ID.msg2, body: "> ↳ 原消息已删除\n\n为什么？", editedAt: T0 + 9000, streaming: false, blockRefs: [{ messageId: M, blockId: "channels", label: "", sourceTopicId: T, deleted: true }] },
  }, "the source was deleted: the ref and the quote line change");
  ok("events/artifact.previewReady.json", "Event", { ...ev(65, LEDGER), type: "artifact.previewReady", data: { artifactId: ART, version: 1, media: video } });

  // -------------------------------------------------------------------------
  // Transient events (only to "interact/1" connections)
  // -------------------------------------------------------------------------
  ok("transient/card.event.json", "TransientEvent", { type: "card.event", groupId: G, topicId: T, messageId: M, eventId: EV(1), rev: 1, blockId: "check", event: "choice.change", value: { value: "wrong" }, actorId: JOYE, at: T0 + 1000 });
  ok("transient/card.event-part.json", "TransientEvent", { type: "card.event", groupId: G, topicId: T, messageId: M, eventId: EV(10), rev: 1, blockId: "bars", event: "chart.select", value: { series: "groups", x: "小红书", y: 14, index: 0 }, part: { kind: "bar", keys: { series: "groups", x: "小红书" } }, actorId: ID.alice, at: T0 });
  ok("transient/interaction.resolved.json", "TransientEvent", { type: "interaction.resolved", groupId: G, eventId: EV(21), result: "replied", note: "看板里抖音的数改好了" });
  ok("transient/artifact.event.json", "TransientEvent", { type: "artifact.event", groupId: G, topicId: T, artifactId: ART, version: 1, eventId: EV(20), name: "breakdown", kind: "ask", payload: { channel: "douyin" }, actorId: JOYE, at: T0 });
  ok("transient/artifact.data-inline.json", "TransientEvent", { type: "artifact.data", groupId: G, artifactId: ART, version: 1, dataRev: 2, data: { channels: { douyin: 7 } }, cause: EV(21) });
  ok("transient/artifact.data-ref.json", "TransientEvent", { type: "artifact.data", groupId: G, artifactId: ART, version: 1, dataRev: 3 }, "over 64 KiB: GET the data");
  ok("transient/artifact.reply.json", "TransientEvent", { type: "artifact.reply", groupId: G, eventId: EV(20), ok: true, payload: { series: [1, 1, 1, 2] } });
  ok("transient/artifact.reply-failed.json", "TransientEvent", { type: "artifact.reply", groupId: G, eventId: EV(22), ok: false, error: { code: "failed", message: "没算出来" } });

  // -------------------------------------------------------------------------
  // HTTP: 产出 data and versions
  // -------------------------------------------------------------------------
  const createArt = {
    groupId: G,
    topicId: T,
    title: "10 月数据看板",
    kind: "html",
    entry: "index.html",
    files: [{ path: "index.html", fileId: FILE }],
    note: "按渠道筛选；发现数不对可以直接告诉 Ledger。",
    events: [
      { name: "breakdown", kind: "ask", label: "按渠道筛选", schema: { type: "object", required: ["channel"], properties: { channel: { enum: ["all", "xhs", "invite", "bili", "douyin"] } } } },
      { name: "report.problem", kind: "emit", label: "数据有问题", schema: { type: "object", properties: { channel: { type: "string", maxLength: 32 }, note: { type: "string", maxLength: 300 } } } },
    ],
    media: { kind: "html", mime: "text/html", bytes: 12_000, preview: "pending" },
    data: { asOf: "2026-10-31", channels: { all: 38, xhs: 14, invite: 10, bili: 9, douyin: 5 } },
  };
  ok("http/create-artifact-media-data.json", "CreateArtifactRequest", createArt, "V11-INTERACT 7.3 (declarations on `events`, R5)");
  bad("entities/artifact-version-bad-event-name.json", "CreateArtifactRequest", { ...createArt, events: [{ name: "Breakdown!", kind: "ask" }] }, "event names are [a-z][A-Za-z0-9_.-]{0,63}");
  bad("http/create-artifact-schema-ref.json", "CreateArtifactRequest", { ...createArt, events: [{ name: "x", schema: { $ref: "#/data/x" } }] }, "no $ref in a page event schema");
  bad("http/create-artifact-reserved-path.json", "CreateArtifactRequest", { ...createArt, files: [{ path: "__teamchat/mermaid.min.js", fileId: FILE }] }, "paths under __teamchat/ are reserved");
  bad("http/create-artifact-media-too-big.json", "CreateArtifactRequest", { ...createArt, media: { kind: "html", mime: "text/html", bytes: 30_000_000, preview: "pending" } }, "an html version is at most 20 MB");
  ok("http/add-artifact-version-media.json", "AddArtifactVersionRequest", { entry: "index.html", files: [{ path: "index.html", fileId: FILE }], events: [{ name: "pick" }], media: { kind: "html", mime: "text/html", bytes: 1000, preview: "ready" }, data: null });
  ok("http/artifact-data-response.json", "ArtifactDataResponse", { data: { channels: { douyin: 7 } }, dataRev: 2, updatedAt: T0 });
  ok("http/artifact-data-response-empty.json", "ArtifactDataResponse", { data: null, dataRev: 0, updatedAt: T0 });
  ok("http/put-artifact-data-request.json", "PutArtifactDataRequest", { data: { channels: { douyin: 7 } }, baseDataRev: 1, cause: EV(21) });
  bad("http/put-artifact-data-request-missing.json", "PutArtifactDataRequest", { baseDataRev: 1 }, "data is required");

  // -------------------------------------------------------------------------
  // Machine: answers, interactions and chat-run output
  // -------------------------------------------------------------------------
  const RUN = "01JBX0RUN000000000000000AU";
  const actor = { id: JOYE, name: "Joye" };
  ok("machine/run.answer.json", "MachineDownFrame", {
    t: "run.answer",
    runId: RUN,
    inputId: "in-7",
    source: { kind: "card", messageId: M, askId: "verify", answerId: "01JC0AN0000000000000000001", rev: 3 },
    actionId: "send",
    event: "action.press",
    inputs: { check: { value: "wrong" } },
    actor,
    eventId: EV(2),
    block: { id: "check", type: "decision", label: "这个数据对不对？", options: [{ id: "right", label: "对" }, { id: "wrong", label: "不对" }] },
  });
  ok("machine/run.interaction-card.json", "MachineDownFrame", {
    t: "run.interaction",
    runId: RUN,
    inputId: "in-8",
    source: { kind: "card", messageId: M, blockId: "check", rev: 1 },
    event: "choice.change",
    value: { value: "wrong" },
    actor,
    eventId: EV(1),
    block: { id: "check", type: "decision", options: [{ id: "right", label: "对" }, { id: "wrong", label: "不对" }], events: ["choice.change"] },
  });
  ok("machine/run.interaction-artifact.json", "MachineDownFrame", {
    t: "run.interaction",
    runId: RUN,
    inputId: "in-9",
    source: { kind: "artifact", artifactId: ART, version: 1, eventId: EV(20), ask: true },
    event: "breakdown",
    value: { channel: "douyin" },
    actor,
  });
  ok("machine/run.output.result.json", "MachineDownFrame", { t: "run.output.result", runId: RUN, outputId: "o1", ok: true, result: { rev: 2 } });
  ok("machine/run.output.result-error.json", "MachineDownFrame", { t: "run.output.result", runId: RUN, outputId: "o2", ok: false, error: { code: "conflict", reason: "card.stale_rev", message: "the card changed" } });
  ok("machine/run.event-output-patch.json", "MachineUpFrame", {
    t: "run.event",
    runId: RUN,
    seq: 12,
    at: T0,
    event: { type: "output", outputId: "o1", op: "card.patch", body: { messageId: M, baseRev: 1, cause: EV(1), ops: [{ op: "remove", id: "note" }] } },
  });
  ok("machine/run.input-answer-2.1.json", "MachineDownFrame", {
    t: "run.input",
    runId: RUN,
    inputId: "in-10",
    text: "<teamchat-event v=\"1\" id=\"in-10\" kind=\"card.answer\">{}</teamchat-event>",
    answer: { messageId: M, rev: 3, actionId: "send", actorId: JOYE, answerId: "01JC0AN0000000000000000001", askId: "verify", eventId: EV(3), event: "action.press" },
  }, "run.input.answer with the 2.1 answer fields (old daemons get the envelope text)");
}
