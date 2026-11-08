// Memory (沉淀) fixtures: items, requests, versions, the daily 记忆更新. Called by gen-fixtures.ts.
export type MemoryCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  T0: number;
  ID: Record<string, string>;
};

export function addMemoryFixtures({ ok, bad, T0, ID }: MemoryCtx) {
  const mem = "01JBX0MEM000000000000000AH";
  const run = "01JBX0RUN000000000000000AJ";
  const item = {
    id: mem,
    scope: "team",
    kind: "convention",
    text: "PR 标题用英文，一行说清改了什么",
    when: "开 PR 的时候",
    source: { kind: "message", ids: [ID.msg] },
    authorId: ID.owner,
    human: true,
    status: "active",
    confidence: 0.9,
    uses: 3,
    lastUsedAt: T0 + 5000,
    version: 2,
    createdAt: T0,
    updatedAt: T0 + 1000,
  };
  const { when: _when, ...noWhen } = item;
  const lesson = {
    ...noWhen,
    id: "01JBX0MEM000000000000000AK",
    scope: `member:${ID.bot}`,
    kind: "lesson",
    text: "跑 cargo test 之前先设 CARGO_TARGET_DIR",
    source: { kind: "run", ids: [run] },
    note: "上次没设，编译占满了磁盘",
    authorId: ID.bot,
    human: false,
    status: "pending",
    confidence: 0.5,
    uses: 0,
    lastUsedAt: null,
    version: 1,
  };
  ok("entities/memory-item.json", "MemoryItem", item);
  ok("entities/memory-item-pending-lesson.json", "MemoryItem", lesson, "an AI member's lesson from a run, waiting for the daily consolidation");
  bad("entities/memory-item-bad-scope.json", "MemoryItem", { ...item, scope: "everyone" }, "scope is team, group:<id>, member:<id> or person:<id>");
  bad("entities/memory-item-long-text.json", "MemoryItem", { ...item, text: "记".repeat(501) }, "text is at most 500 characters");

  ok("http/create-memory-request.json", "CreateMemoryRequest", { scope: `group:${ID.general}`, kind: "convention", text: "周五不上线", when: "要发布的时候", source: { kind: "message", ids: [ID.msg] } });
  ok("http/create-memory-request-minimal.json", "CreateMemoryRequest", { scope: `person:${ID.alice}`, kind: "preference", text: "回复要短" });
  bad("http/create-memory-request-empty.json", "CreateMemoryRequest", { scope: "team", kind: "fact", text: "" }, "text is required");
  bad("http/create-memory-request-bad-kind.json", "CreateMemoryRequest", { scope: "team", kind: "rumor", text: "x" }, "unknown kind");
  bad("http/create-memory-request-bad-scope.json", "CreateMemoryRequest", { scope: "group:", kind: "fact", text: "x" }, "a group scope needs the group id");

  ok("http/update-memory-request.json", "UpdateMemoryRequest", { text: "PR 标题用英文", when: null });
  ok("http/update-memory-request-retire.json", "UpdateMemoryRequest", { status: "retired" }, "停用");
  bad("http/update-memory-request-pending.json", "UpdateMemoryRequest", { status: "pending" }, "a person cannot make a memory pending");

  ok("http/memories-response.json", "MemoriesResponse", { memories: [item, lesson] });
  ok("http/memory-versions-response.json", "MemoryVersionsResponse", {
    versions: [
      { memoryId: mem, version: 2, item, by: ID.owner, at: T0 + 1000 },
      { memoryId: mem, version: 1, item: { ...item, text: "PR 标题用英文", version: 1, updatedAt: T0 }, by: null, at: T0 },
    ],
  });
  const added = {
    id: "01JBX0MEMUPD00000000000AAA",
    day: "2026-11-09",
    scope: `member:${ID.bot}`,
    memoryId: lesson.id,
    change: "added",
    before: null,
    after: { ...lesson, status: "active", version: 2 },
    undoneAt: null,
    at: T0 + 9000,
  };
  ok("http/memory-updates-response.json", "MemoryUpdatesResponse", {
    day: "2026-11-09",
    updates: [
      added,
      { ...added, id: "01JBX0MEMUPD00000000000AAB", scope: "team", memoryId: mem, change: "flagged", before: item, after: item, note: "可能过时了：团队已经改用中文标题" },
      { ...added, id: "01JBX0MEMUPD00000000000AAC", change: "retired", before: { ...lesson, status: "active" }, after: { ...lesson, status: "retired", version: 3 }, note: "60 天没用到", undoneAt: T0 + 20000 },
    ],
  });
  ok("http/memory-updates-response-empty.json", "MemoryUpdatesResponse", { day: null, updates: [] }, "no consolidation yet");
  bad("http/memory-updates-response-bad-day.json", "MemoryUpdatesResponse", { day: "9 Nov", updates: [] }, "day is YYYY-MM-DD");

  ok("machine/memory-note-request.json", "MachineMemoryNoteRequest", { scope: "member", kind: "lesson", text: "这个仓库的测试要先 bun install", when: "跑测试之前" });
  bad("machine/memory-note-request-team.json", "MachineMemoryNoteRequest", { scope: "team", kind: "convention", text: "x" }, "a run never writes team memory");
}
