// 1.1 fixtures: Card v2, card answers, side chat context, run fields, artifact events and
// topic links. Called by gen-fixtures.ts. The machine frames of 1.1 are daemon golden lines
// (apps/daemon/tests/fixtures/machine/*.jsonl), added by fixtures-bots.ts.
import { cardV2ToV1, MAX_CARD_V2_ACTIONS } from "../src/index.ts";
import type { CardV2 } from "../src/index.ts";

type Obj = Record<string, unknown>;
export type V11Ctx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  add: (path: string, f: { kind: string; valid: boolean; description?: string; value: unknown; normalized?: unknown }) => void;
  T0: number;
  ID: Record<string, string>;
  base: Obj;
  message: Obj;
  topic: Obj;
};

export function addV11Fixtures({ ok, bad, add, T0, ID, base, message, topic }: V11Ctx) {
  const X = {
    art: "01JBX0ART000000000000000AM",
    file: "01JBX0FILE00000000000000AH",
    run: "01JBX0RUN000000000000000AU",
    machine: "01JBX0MACHINE00000000000AV",
    side: "01JBX0SIDE00000000000000AW",
    dm: ID.dm,
  };

  // -------------------------------------------------------------------------
  // Card v2
  // -------------------------------------------------------------------------
  const ask: CardV2 = {
    v: 2,
    rev: 1,
    intent: "ask",
    title: "发布 qqbot 1.4 到正式环境？",
    fallback: "Scout 在 staging 验收通过。请选择发布时间。",
    ask: {
      from: ["requester", "owners", ID.alice],
      expiresAt: T0 + 86_400_000,
      record: true,
      irreversible: { verb: "发布", target: "qqbot 1.4 到正式环境", consequence: "所有群立刻用上新版本", typeToConfirm: "发布" },
    },
    blocks: [
      { type: "text", id: "why", text: "**验收通过**，没有新的报错。" },
      { type: "decision", id: "when", options: [{ id: "now", label: "现在发" }, { id: "tonight", label: "今晚 22:00", detail: "人少" }], default: ["tonight"] },
      { type: "input", id: "note", kind: "textarea", label: "给大家的话", placeholder: "可不填", maxLength: 500 },
      { type: "input", id: "pct", kind: "number", label: "灰度比例", min: 0, max: 100, default: 10 },
      { type: "input", id: "tags", kind: "chips", label: "通知", options: [{ id: "all", label: "全员" }, { id: "ops", label: "值班" }] },
      { type: "input", id: "notify", kind: "switch", label: "发通知", default: true },
      { type: "draft", id: "notes", label: "发布说明", text: "- 修好了重复消息\n- 更快的启动", editable: true },
      { type: "evidence", id: "e2e", label: "E2E 42/42", state: "pass", url: "https://github.com/Joye-Co/qqbot/actions/runs/1" },
    ],
    actions: [
      { id: "ship", label: "发布", style: "primary", confirm: { title: "确定发布？", body: "所有群立刻用上新版本。" } },
      { id: "later", label: "再等等", kind: "cancel" },
    ],
    data: { releaseId: 14 },
  };
  const done: CardV2 = {
    v: 2,
    rev: 3,
    intent: "done",
    title: "修好了 sync 测试",
    fallback: "测试 128/128 通过，PR #42 已开。",
    blocks: [
      { type: "metric", id: "tests", label: "测试", value: 128, unit: "个", delta: 3, better: "up" },
      { type: "metric", id: "time", label: "用时", value: 4.5, unit: "分钟", better: "down", note: "比上次快" },
      { type: "chart", id: "trend", kind: "line", label: "构建时间", unit: "s", x: ["周一", "周二", "周三"], series: [{ name: "main", values: [312, 298.5, 270] }] },
      { type: "chart", id: "spark", kind: "spark", series: [{ name: "失败", values: [] }] },
      { type: "diff", id: "patch", path: "src/sync.rs", text: "@@ -1 +1 @@\n-a\n+b\n", additions: 1, deletions: 1 },
      { type: "plan", id: "plan", runId: X.run, items: [{ text: "跑测试", state: "done" }, { text: "开 PR", state: "current" }, { text: "等审", state: "waiting" }, { text: "合并", state: "pending" }] },
      {
        type: "group",
        id: "refs",
        label: "相关",
        blocks: [
          { type: "ref", id: "pr", label: "PR #42", target: { kind: "pr", url: "https://github.com/Joye-Co/team-chat/pull/42", repo: "Joye-Co/team-chat", number: 42, state: "open" } },
          { type: "ref", id: "page", label: "报告", target: { kind: "artifact", artifactId: X.art, version: 2 } },
          { type: "ref", id: "topic", label: "原话题", target: { kind: "topic", groupId: ID.general, topicId: ID.topic } },
          { type: "ref", id: "who", label: "Alice", target: { kind: "member", memberId: ID.alice } },
          { type: "ref", id: "docs", label: "文档", target: { kind: "url", url: "https://example.com/docs" } },
          { type: "evidence", id: "shot", label: "截图", state: "info", fileId: X.file, artifact: { artifactId: X.art, version: 1 } },
        ],
      },
      { type: "fields", id: "f", fields: [{ label: "分支", value: "bot/run" }] },
      { type: "table", id: "tbl", columns: ["文件", "改动"], rows: [["src/a.rs", "+3"]] },
      { type: "list", id: "lst", items: [{ text: "a", status: "done" }] },
      { type: "progress", id: "prog", value: 1, label: "完成" },
      { type: "code", id: "log", lang: "text", text: "ok" },
      { type: "image", id: "img", fileId: X.file, alt: "结果" },
      { type: "divider", id: "div" },
      { type: "link", id: "lnk", label: "打开", target: { kind: "url", url: "https://example.com" } },
    ],
    actions: [{ id: "open", label: "看 PR", kind: "link", target: { kind: "pr", url: "https://github.com/Joye-Co/team-chat/pull/42" } }],
  };
  const working: CardV2 = { v: 2, rev: 1, intent: "working", title: "正在做", fallback: "", blocks: [] };
  const info: CardV2 = { v: 2, rev: 1, intent: "info", title: "提示", fallback: "只是告诉你", blocks: [{ type: "text", id: "t", text: "x" }], actions: [{ id: "ok", label: "知道了" }] };
  ok("entities/card2-ask.json", "CardV2", ask, "ask with every input kind used here, irreversible, expiry and record");
  ok("entities/card2-done.json", "CardV2", done, "done with metric, chart, diff, plan, group of refs and the v1 block types");
  ok("entities/card2-working.json", "CardV2", working, "minimal");
  ok("entities/card2-info.json", "CardV2", info);
  bad("entities/card2-v1.json", "CardV2", { ...working, v: 1 }, "v must be 2");
  bad("entities/card2-bad-intent.json", "CardV2", { ...working, intent: "question" }, "unknown intent");
  bad("entities/card2-missing-id.json", "CardV2", { ...working, blocks: [{ type: "text", text: "x" }] }, "blocks need an id");
  bad("entities/card2-duplicate-block-ids.json", "CardV2", { ...working, blocks: [{ type: "divider", id: "a" }, { type: "group", id: "g", blocks: [{ type: "divider", id: "a" }] }] }, "block ids are unique, nested ones too");
  bad("entities/card2-nested-group.json", "CardV2", { ...working, blocks: [{ type: "group", id: "g", blocks: [{ type: "group", id: "h", blocks: [{ type: "divider", id: "d" }] }] }] }, "no group inside a group");
  bad("entities/card2-decision-one-option.json", "CardV2", { ...working, blocks: [{ type: "decision", id: "d", options: [{ id: "a", label: "A" }] }] }, "a decision has 2..10 options");
  bad("entities/card2-bad-input-kind.json", "CardV2", { ...working, blocks: [{ type: "input", id: "i", kind: "color", label: "颜色" }] }, "unknown input kind");
  bad("entities/card2-bad-chart-kind.json", "CardV2", { ...working, blocks: [{ type: "chart", id: "c", kind: "pie", series: [{ name: "a", values: [1] }] }] }, "unknown chart kind");
  bad("entities/card2-link-without-target.json", "CardV2", { ...working, actions: [{ id: "a", label: "A", kind: "link" }] }, "a link action needs a target");
  bad("entities/card2-submit-with-target.json", "CardV2", { ...working, actions: [{ id: "a", label: "A", target: { kind: "url", url: "https://example.com" } }] }, "only link actions have a target");
  bad("entities/card2-bad-ref-kind.json", "CardV2", { ...working, blocks: [{ type: "ref", id: "r", label: "x", target: { kind: "issue", url: "https://example.com" } }] }, "unknown ref kind");
  bad("entities/card2-ask-from-empty.json", "CardV2", { ...ask, ask: { from: [] } }, "ask.from needs a member");
  bad("entities/card2-too-many-actions.json", "CardV2", { ...working, actions: Array.from({ length: MAX_CARD_V2_ACTIONS + 1 }, (_, i) => ({ id: `a${i}`, label: "x" })) }, "limit: 11 actions");
  bad("entities/card2-too-many-blocks.json", "CardV2", { ...working, blocks: Array.from({ length: 51 }, (_, i) => ({ type: "divider", id: `d${i}` })) }, "limit: 51 blocks");
  bad("entities/card2-too-big.json", "CardV2", { ...working, blocks: Array.from({ length: 9 }, (_, i) => ({ type: "code", id: `c${i}`, text: "x".repeat(8000) })) }, "card exceeds 64 KiB");
  bad("entities/card2-unknown-block.json", "CardV2", { ...working, blocks: [{ type: "map", id: "m" }] }, "the server rejects unknown block types");

  // The v1 card the server derives (docs/protocol.md, "Cards v2"); old readers see this.
  ok("entities/card2-ask-as-v1.json", "Card", cardV2ToV1(ask), "cardV2ToV1 of card2-ask: no v1 actions, the card has inputs");
  ok("entities/card2-done-as-v1.json", "Card", cardV2ToV1(done), "cardV2ToV1 of card2-done");
  ok("entities/card2-info-as-v1.json", "Card", cardV2ToV1(info), "cardV2ToV1 of card2-info: the action stays pressable");

  // Messages carry cardV2 next to the derived v1 card.
  const cardMessage = { ...message, authorId: ID.bot, body: ask.fallback, mentions: [], kind: "card", card: cardV2ToV1(ask), cardV2: ask };
  ok("entities/message-card2.json", "Message", cardMessage, "Card v2 message with its derived v1 card");
  add("entities/message-card2-unreadable.json", {
    kind: "Message",
    valid: true,
    description: "a cardV2 this version cannot read is dropped; the v1 card stays",
    value: { ...cardMessage, cardV2: { ...working, v: 3 } },
    normalized: { ...cardMessage, cardV2: undefined },
  });
  ok("client/send-card2.json", "ClientFrame", { t: "send", rid: "r90", groupId: ID.general, topicId: ID.topic, clientMsgId: "c90", body: "", cardV2: done }, "the server derives card");
  ok("client/edit-card2.json", "ClientFrame", { t: "edit", rid: "r91", groupId: ID.general, messageId: ID.msg, body: "", cardV2: { ...ask, rev: 2 } });
  bad("client/send-card2-invalid.json", "ClientFrame", { t: "send", rid: "r92", groupId: ID.general, clientMsgId: "c92", body: "", cardV2: { ...working, intent: "x" } }, "a send with an invalid cardV2 is rejected");
  ok("events/message.updated-card2.json", "Event", { ...base, actorId: ID.bot, type: "message.updated", data: { messageId: ID.msg, body: "", editedAt: T0 + 9000, streaming: false, card: cardV2ToV1(done), cardV2: done } });

  // Answers
  ok("client/card.answer.json", "ClientFrame", {
    t: "card.answer",
    rid: "r93",
    groupId: ID.general,
    messageId: ID.msg,
    rev: 1,
    actionId: "ship",
    inputs: { when: "tonight", note: "辛苦了", pct: 20, tags: ["all"], notify: true },
    editedValues: { notes: "- 修好了重复消息" },
  });
  ok("client/card.answer-minimal.json", "ClientFrame", { t: "card.answer", rid: "r94", groupId: ID.general, messageId: ID.msg, rev: 1, actionId: "later" });
  bad("client/card.answer-bad-block-id.json", "ClientFrame", { t: "card.answer", rid: "r95", groupId: ID.general, messageId: ID.msg, rev: 1, actionId: "ship", inputs: { "a b": 1 } }, "input keys are block ids");
  bad("client/card.answer-rev-zero.json", "ClientFrame", { t: "card.answer", rid: "r96", groupId: ID.general, messageId: ID.msg, rev: 0, actionId: "ship" }, "rev is positive");
  ok("events/card.answered.json", "Event", { ...base, seq: 43, type: "card.answered", data: { messageId: ID.msg, rev: 1, actionId: "ship", inputs: { when: "tonight" }, editedValues: { notes: "x" } } });
  ok("events/card.answered-minimal.json", "Event", { ...base, seq: 44, type: "card.answered", data: { messageId: ID.msg, rev: 2, actionId: "later" } });

  // -------------------------------------------------------------------------
  // Side chats: send.context and Topic.contextOf
  // -------------------------------------------------------------------------
  const context = { groupId: ID.general, topicId: ID.topic, uptoSeq: 41 };
  ok("entities/topic-context.json", "TopicContext", context);
  bad("entities/topic-context-missing-seq.json", "TopicContext", { groupId: ID.general, topicId: ID.topic }, "missing uptoSeq");
  ok("entities/topic-side-chat.json", "Topic", { ...topic, id: X.side, groupId: X.dm, contextOf: context }, "a side chat topic in a DM with a bot");
  ok("client/send-context.json", "ClientFrame", { t: "send", rid: "r97", groupId: X.dm, clientMsgId: "c97", body: "这个话题现在卡在哪？", context }, "start a side chat about a topic");
  ok("events/topic.updated-context.json", "Event", { ...base, groupId: X.dm, type: "topic.updated", data: { topicId: X.side, contextOf: { ...context, uptoSeq: 60 } } });

  // -------------------------------------------------------------------------
  // Runs: usage, duration, result, acceptance, pause, kind
  // -------------------------------------------------------------------------
  const run = {
    id: X.run,
    botId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    machineId: X.machine,
    status: "running",
    requestedBy: ID.owner,
    createdAt: T0,
    startedAt: T0 + 1000,
    updatedAt: T0 + 9000,
    usage: { inputTokens: 12_000, outputTokens: 3400, costUsd: 0.42 },
    durationMs: 271_000,
    resultMessageId: ID.msg,
    acceptance: [
      { text: "staging 上能登录", state: "pass", evidence: { text: "截图", fileId: X.file, url: "https://staging.example.com", artifact: { artifactId: X.art, version: 1 } } },
      { text: "E2E 全过", state: "pending" },
      { text: "没有新报错", state: "fail" },
    ],
    pausedBy: ID.owner,
  };
  ok("entities/bot-run-v11.json", "BotRun", run, "usage, durationMs, resultMessageId, acceptance and pausedBy");
  ok("entities/bot-run-aside.json", "BotRun", { ...run, id: "01JBX0RUN000000000000000AX", groupId: X.dm, topicId: X.side, kind: "aside", acceptance: [], usage: { inputTokens: 1, outputTokens: 2 } }, "a side chat run");
  bad("entities/bot-run-bad-acceptance.json", "BotRun", { ...run, acceptance: [{ text: "x", state: "met" }] }, "unknown acceptance state");
  ok("events/run.updated-v11.json", "Event", { ...base, actorId: ID.bot, type: "run.updated", data: { run } });

  // -------------------------------------------------------------------------
  // 产出: version runId and declared events, artifact.event, artifact.evented
  // -------------------------------------------------------------------------
  const version = {
    artifactId: X.art,
    version: 2,
    entry: "index.html",
    files: [{ path: "index.html", fileId: X.file, size: 2048, mime: "text/html" }],
    createdBy: ID.bot,
    createdAt: T0 + 9000,
    runId: X.run,
    events: [{ name: "pick", label: "选了一个方案" }, { name: "submit" }],
  };
  ok("entities/artifact-version-v11.json", "ArtifactVersion", version, "made by a run, declares events");
  ok("http/create-artifact-request-events.json", "CreateArtifactRequest", { groupId: ID.general, topicId: ID.topic, title: "方案对比", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: X.file }], events: [{ name: "pick" }], runId: X.run });
  ok("client/artifact.event.json", "ClientFrame", { t: "artifact.event", rid: "r98", groupId: ID.general, artifactId: X.art, version: 2, name: "pick", payload: { option: "B" } });
  ok("client/artifact.event-minimal.json", "ClientFrame", { t: "artifact.event", rid: "r99", groupId: ID.general, artifactId: X.art, version: 2, name: "submit" });
  bad("client/artifact.event-too-big.json", "ClientFrame", { t: "artifact.event", rid: "r100", groupId: ID.general, artifactId: X.art, version: 2, name: "pick", payload: "x".repeat(16_385) }, "payload exceeds 16 KiB");
  bad("client/artifact.event-name-too-long.json", "ClientFrame", { t: "artifact.event", rid: "r101", groupId: ID.general, artifactId: X.art, version: 2, name: "x".repeat(129) }, "name is 1..128");
  ok("events/artifact.evented.json", "Event", { ...base, seq: 45, actorId: ID.alice, type: "artifact.evented", data: { artifactId: X.art, version: 2, name: "pick", payload: { option: "B" } } });

  // -------------------------------------------------------------------------
  // Topic links
  // -------------------------------------------------------------------------
  const pr = {
    kind: "pr",
    repo: "Joye-Co/team-chat",
    number: 42,
    url: "https://github.com/Joye-Co/team-chat/pull/42",
    title: "fix: sync test",
    state: "open",
    checks: [{ name: "CI", conclusion: "success" }, { name: "e2e", conclusion: "pending" }],
    additions: 120,
    deletions: 30,
    changedFiles: 4,
  };
  ok("entities/topic-link-pr.json", "TopicLink", pr);
  ok("entities/topic-link-pr-minimal.json", "TopicLink", { kind: "pr", repo: "a/b", number: 1, url: "https://github.com/a/b/pull/1", title: "", state: "draft", checks: [] });
  bad("entities/topic-link-unknown-kind.json", "TopicLink", { ...pr, kind: "issue" }, "the server writes only known kinds");
  ok("events/topic.linked.json", "Event", { ...base, seq: 46, actorId: ID.bot, type: "topic.linked", data: { topicId: ID.topic, link: pr } });

  // Machines expose the daemon's features to owners.
  ok("entities/machine-v11.json", "Machine", {
    id: X.machine,
    name: "mac-mini",
    labels: ["mac"],
    createdBy: ID.owner,
    createdAt: T0,
    connected: true,
    healthy: true,
    runningRuns: [X.run],
    features: ["pause", "answer", "aside", "acceptance"],
    takeoverUrl: "vnc://mac-mini.tailnet",
  });
}
