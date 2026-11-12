// AI 服务 fixtures (usable work line 4): switches, health and the failing alert, the status every
// member reads, the two checks of 测试连接, the 用量 line and the `ai.failing` transient. Called by
// gen-fixtures.ts. The keys and errors are made up.
export type AiServiceCtx = {
  ok: (
    path: string,
    kind: string,
    value: unknown,
    description?: string,
  ) => void;
  bad: (
    path: string,
    kind: string,
    value: unknown,
    description: string,
    reason?: string,
  ) => void;
};

export function addAiServiceFixtures({ ok, bad }: AiServiceCtx) {
  ok(
    "http/ai-service-health.json",
    "AiServiceConfig",
    {
      provider: "deepseek",
      model: "deepseek-chat",
      baseUrl: null,
      keySet: true,
      keyHint: "a1b2",
      updatedAt: 1762128000000,
      switches: { frontDesk: false, titles: true, memory: true },
      uses: [
        {
          use: "front_desk",
          at: 1762128600000,
          ok: false,
          error: "401 Authentication Fails",
          failures: 3,
          okAt: 1762128000000,
        },
        {
          use: "title",
          at: 1762128300000,
          ok: true,
          failures: 0,
          okAt: 1762128300000,
        },
      ],
      failing: {
        since: 1762128600000,
        use: "front_desk",
        error: "401 Authentication Fails",
      },
    },
    "usable: switches, the last call of each use, and failing after several failures in a row",
  );
  ok(
    "http/ai-service-status.json",
    "AiServiceStatus",
    { configured: true, frontDesk: false, titles: true, memory: true },
    "usable: any member reads whether the team's model is set up and which uses are on",
  );
  bad(
    "http/ai-service-status-missing.json",
    "AiServiceStatus",
    { configured: false },
    "every field is required",
  );
  ok(
    "http/ai-service-test-tool-failed.json",
    "AiServiceTestResult",
    {
      ok: false,
      error: "forced tool call: no tool call",
      latencyMs: 900,
      text: { ok: true },
      tool: { ok: false, error: "no tool call" },
    },
    "usable: the text call works, the forced tool call does not",
  );
  ok(
    "http/update-ai-service-switches.json",
    "UpdateAiServiceSwitchesRequest",
    { frontDesk: false },
    "absent switches keep",
  );
  bad(
    "http/update-ai-service-switches-empty.json",
    "UpdateAiServiceSwitchesRequest",
    {},
    "at least one switch",
  );
  ok(
    "http/team-usage-service.json",
    "TeamUsage",
    {
      from: 1761523200000,
      to: 1762128000000,
      total: {
        costUsd: 1.5,
        inputTokens: 10000,
        outputTokens: 1000,
        runs: 1,
        topics: 1,
      },
      previous: { costUsd: 2.5, topics: 1 },
      bots: [],
      initiators: [],
      machines: [],
      runtimes: [],
      daily: [],
      service: {
        calls: 42,
        failures: 2,
        costUsd: 0.12,
        inputTokens: 120000,
        outputTokens: 9000,
        cacheReadTokens: 40000,
        uses: [
          {
            use: "front_desk",
            calls: 30,
            inputTokens: 100000,
            outputTokens: 6000,
          },
          { use: "title", calls: 12, inputTokens: 20000, outputTokens: 3000 },
        ],
      },
    },
    "usable: the AI 服务's own calls as one line",
  );
  ok(
    "transient/ai.failing.json",
    "TransientEvent",
    {
      type: "ai.failing",
      failing: {
        since: 1762214400000,
        use: "front_desk",
        error: "401 Authentication Fails",
      },
    },
    "only to admins' connections with feature settings/1, once when failing is raised",
  );
  bad(
    "transient/ai.failing-missing.json",
    "TransientEvent",
    { type: "ai.failing" },
    "failing is required",
  );
}
