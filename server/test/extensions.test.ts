import { describe, expect, it } from "vitest";
import { z } from "zod";
import { ApiError, unwrap } from "../src/core/errors";
import {
  registerAfterCommit,
  registerFrameHandler,
  registerFrameValidator,
  registerRoomRpc,
  registerRoute,
} from "../src/core/registry";
import { roomStub } from "../src/core/env";
import { env } from "cloudflare:test";
import { Client, api, clientMsgId, createGroup, createMember, waitUntil } from "./helpers";
import { protocolViolations } from "./helpers";
import { afterAll as afterAllCheck } from "vitest";

afterAllCheck(() => {
  expect(protocolViolations).toEqual([]);
});

const hooked: { type: string; seq: number; groupId: string }[] = [];

registerRoute({
  method: "POST",
  path: "/v1/test-ext/:name",
  auth: "member",
  bodyLimit: 16,
  async handler({ params, body, auth }) {
    const b = await body(z.object({ n: z.int() }));
    return { name: params.name, n: b.n, me: auth?.memberId };
  },
});

registerFrameHandler<{ t: "test.echo"; rid: string; groupId: string; text: string }>("test.echo", {
  schema: z.object({ t: z.literal("test.echo"), rid: z.string(), groupId: z.string(), text: z.string() }),
  rate: "read",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const r = await roomStub(c.env, frame.groupId).rpc(frame.groupId, "test.note", c.actor, { text: frame.text });
    return unwrap(r);
  },
});

registerRoomRpc("test.note", (room, actor, args) => {
  const { text } = args as { text: string };
  const event = room.commit((tx) => tx.append({ type: "group.updated", actorId: actor!.id, data: { name: text } }));
  return { seq: event.seq, members: room.memberIds().length };
});

registerFrameValidator("send", (frame) => {
  if ("body" in frame && frame.body === "forbidden-word") throw new ApiError("invalid", "blocked by validator");
});

registerAfterCommit((event) => {
  hooked.push({ type: event.type, seq: event.seq, groupId: event.groupId });
});

describe("extension points", () => {
  it("serve feature routes with their own body limit", async () => {
    const m = await createMember();
    const r = await api("/v1/test-ext/abc", { token: m.token, body: { n: 1 } });
    expect(r.body).toEqual({ name: "abc", n: 1, me: m.id });
    const big = await api("/v1/test-ext/abc", { token: m.token, body: { n: 1, pad: "xxxxxxxxxxxxxxxxxxxx" } });
    expect(big.status).toBe(413);
    expect((await api("/v1/test-ext/abc", { body: { n: 1 } })).status).toBe(401);
  });

  it("route feature frames to room RPCs, run validators and after-commit hooks", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    const { c } = await Client.connect(a.token);
    const r = await c.ok({ t: "test.echo", groupId: gid, text: "renamed" });
    expect(r).toEqual({ seq: 1, members: 2 });
    await c.waitEvent((e) => e.type === "group.updated" && e.data.name === "renamed");
    const bad = await c.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "forbidden-word" });
    expect(bad).toMatchObject({ ok: false, error: { code: "invalid", message: "blocked by validator" } });
    await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "fine" });
    await waitUntil(() => hooked.filter((h) => h.groupId === gid).length === 3);
    expect(hooked.filter((h) => h.groupId === gid).map((h) => h.type)).toEqual([
      "group.updated",
      "topic.created",
      "message.created",
    ]);
    const outsider = await createMember();
    const { c: o } = await Client.connect(outsider.token);
    expect(await o.request({ t: "test.echo", groupId: gid, text: "x" })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    c.close();
    o.close();
  });

  it("export pages events for the archive", async () => {
    const a = await createMember();
    const gid = await createGroup(a, []);
    const { c } = await Client.connect(a.token);
    await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "one" });
    const r = await roomStub(env, gid).exportEvents(gid, 0, 1);
    expect(r).toMatchObject({ ok: true, value: { groupId: gid, hasMore: true, lastSeq: 2 } });
    if (r.ok) expect(r.value.events.map((e) => e.seq)).toEqual([1]);
    c.close();
  });
});
