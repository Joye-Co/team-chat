// AI 服务 (src/aiservice): owner-only config with the key encrypted at rest, 测试连接, and AI topic
// titles from the team's provider. Provider calls go to a fake fetch.
import { env } from "cloudflare:test";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { callModel, completionsUrl, takesEffort } from "../src/aiservice/providers";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const KEY = "sk-deepseek-test-9f3k";
const realFetch = globalThis.fetch;
type Seen = { url: string; auth: string | null; body: any };

/** Answers calls to the providers' hosts; everything else goes to the real fetch. */
function fakeProviders(answer: (s: Seen) => Response): Seen[] {
  const seen: Seen[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!/api\.deepseek\.com|api\.anthropic\.com|llm\.example\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
    const s: Seen = { url: req.url, auth: req.headers.get("authorization") ?? req.headers.get("x-api-key"), body: await req.json() };
    seen.push(s);
    return answer(s);
  });
  return seen;
}

const completion = (content: string) => Response.json({ choices: [{ message: { role: "assistant", content } }] });

afterEach(async () => {
  vi.restoreAllMocks();
});

describe("ai service", () => {
  it("is owner only and never returns the key", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    expect((await api("/v1/settings/ai", { token: member.token })).status).toBe(403);
    const empty = await api("/v1/settings/ai", { token: owner.token });
    expect(empty.body).toEqual({
      provider: null,
      model: "",
      baseUrl: null,
      keySet: false,
      keyHint: null,
      updatedAt: 0,
      switches: { frontDesk: true, titles: true, memory: true },
    });

    const noKey = await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "deepseek", model: "deepseek-chat" } });
    expect(noKey.status).toBe(400);
    expect((await api("/v1/settings/ai", { method: "PUT", token: member.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: KEY } })).status).toBe(403);

    const saved = await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: KEY } });
    expect(saved.status).toBe(200);
    expect(saved.body).toMatchObject({ provider: "deepseek", model: "deepseek-chat", baseUrl: null, keySet: true, keyHint: "9f3k" });
    expect(JSON.stringify(saved.body)).not.toContain(KEY);
    const read = await api("/v1/settings/ai", { token: owner.token });
    expect(read.body).toEqual(saved.body);

    const row = await env.DB.prepare("SELECT key_cipher FROM ai_service").first<{ key_cipher: string }>();
    expect(row?.key_cipher.startsWith("v1.")).toBe(true);
    expect(row?.key_cipher).not.toContain(KEY);
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'settings.ai' ORDER BY id DESC LIMIT 1").first<{ details: string }>();
    expect(audit?.details).not.toContain(KEY);
    expect(JSON.parse(audit!.details)).toMatchObject({ after: { provider: "deepseek", model: "deepseek-chat" }, keyChanged: true });

    // The saved key stays for the same provider, not for another one.
    const kept = await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "deepseek", model: "deepseek-reasoner" } });
    expect(kept.body).toMatchObject({ model: "deepseek-reasoner", keyHint: "9f3k" });
    const switched = await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "anthropic", model: "claude-haiku-5-5" } });
    expect(switched.status).toBe(400);
    const noBase = await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "openai-compatible", model: "m", apiKey: KEY } });
    expect(noBase.status).toBe(400);

    const cleared = await api("/v1/settings/ai", { method: "DELETE", token: owner.token });
    expect(cleared.body.keySet).toBe(false);
    expect((await api("/v1/settings/ai", { token: owner.token })).body.provider).toBeNull();
  });

  it("tests the connection with the saved key or a new one", async () => {
    const owner = await createMember({ level: "owner" });
    await api("/v1/settings/ai", { method: "PUT", token: owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: KEY } });
    let tools = true;
    const seen = fakeProviders((s) => {
      if (s.auth !== `Bearer ${KEY}`) return Response.json({ error: { message: "Authentication Fails" } }, { status: 401 });
      if (!s.body.tools || !tools) return completion("OK");
      return Response.json({ choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: "answer", arguments: '{"ok":true}' } }] } }] });
    });
    const ok = await api("/v1/settings/ai/test", { token: owner.token, body: { provider: "deepseek", model: "deepseek-chat" } });
    expect(ok.body).toMatchObject({ ok: true, text: { ok: true }, tool: { ok: true } });
    expect(ok.body.error).toBeUndefined();
    expect(seen[0].url).toBe("https://api.deepseek.com/chat/completions");
    expect(seen[0].body.model).toBe("deepseek-chat");
    // 模 1.7: the second check is a forced tool call, as the front desk makes.
    expect(seen[1].body.tool_choice).toEqual({ type: "function", function: { name: "answer" } });

    // A model that answers text but cannot be made to call a tool: the test says which check fails.
    tools = false;
    const noTools = await api("/v1/settings/ai/test", { token: owner.token, body: { provider: "deepseek", model: "deepseek-chat" } });
    expect(noTools.body).toMatchObject({ ok: false, text: { ok: true }, tool: { ok: false } });
    expect(noTools.body.error).toMatch(/^forced tool call: no tool call/);

    const bad = await api("/v1/settings/ai/test", { token: owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-wrong-key-0000" } });
    expect(bad.body).toMatchObject({ ok: false, error: "401 Authentication Fails", text: { ok: false, error: "401 Authentication Fails" } });
    expect(bad.body.tool).toBeUndefined();
    // Testing does not save.
    expect((await api("/v1/settings/ai", { token: owner.token })).body.keyHint).toBe("9f3k");
    await api("/v1/settings/ai", { method: "DELETE", token: owner.token });
  });

  it("names a new topic with the team's provider", async () => {
    const owner = await createMember({ level: "owner", name: "AiOwner" });
    await api("/v1/settings/ai", {
      method: "PUT",
      token: owner.token,
      body: { provider: "openai-compatible", model: "qwen-turbo", baseUrl: "https://llm.example.com/v1/", apiKey: KEY },
    });
    const seen = fakeProviders(() => completion("「登录页报错排查」"));
    const gid = await createGroup(owner, [], "全员");
    const { c } = await Client.connect(owner.token);
    const sent = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "登录页一直报 500，帮忙看下" });
    const topicId = sent.topic.id;
    const titled = () => c.events(gid).find((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.title);
    await waitUntil(() => Boolean(titled()));
    expect(titled().data.title).toBe("登录页报错排查");
    expect(seen[0].url).toBe("https://llm.example.com/v1/chat/completions");
    expect(seen[0].body.messages[1].content).toContain("登录页一直报 500");
    // 模 1.4: the call is recorded under its use.
    const cfg = await api("/v1/settings/ai", { token: owner.token });
    expect(cfg.body.uses).toEqual([expect.objectContaining({ use: "title", ok: true, failures: 0 })]);
    await api("/v1/settings/ai", { method: "DELETE", token: owner.token });
  });

  it("calls Anthropic through the SDK", async () => {
    const seen = fakeProviders(() =>
      Response.json({
        id: "msg_1",
        type: "message",
        role: "assistant",
        model: "claude-haiku-5-5",
        content: [{ type: "text", text: "周五发版" }],
        stop_reason: "end_turn",
        stop_sequence: null,
        usage: { input_tokens: 10, output_tokens: 3, cache_read_input_tokens: 2000 },
      }),
    );
    const r = await callModel({ provider: "anthropic", model: "claude-haiku-5-5", baseUrl: null, apiKey: "sk-ant-test-0000" }, { system: "s", prompt: "p", maxTokens: 64 });
    expect(r).toEqual({ ok: true, text: "周五发版", usage: { inputTokens: 10, outputTokens: 3, cacheReadTokens: 2000 } });
    expect(seen[0].url).toBe("https://api.anthropic.com/v1/messages");
    expect(seen[0].body.output_config).toEqual({ effort: "low" });
    // 模 1.11: the system prompt is the stable prefix, marked for the prompt cache.
    expect(seen[0].body.system).toEqual([{ type: "text", text: "s", cache_control: { type: "ephemeral" } }]);
  });

  it("builds URLs and knows which Claude models take effort", () => {
    expect(completionsUrl("deepseek", null)).toBe("https://api.deepseek.com/chat/completions");
    expect(completionsUrl("openai-compatible", "https://x.test/v1/chat/completions")).toBe("https://x.test/v1/chat/completions");
    expect(takesEffort("claude-haiku-5-5")).toBe(true);
    expect(takesEffort("claude-haiku-4-5")).toBe(false);
    expect(takesEffort("claude-opus-4-5")).toBe(true);
    expect(takesEffort("claude-sonnet-4-5")).toBe(false);
  });
});
