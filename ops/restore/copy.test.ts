// The id map of a copy into another group (restore.ts group --from-group).
import { expect, test } from "bun:test";
import { copiedId, copyIdMap, remapValue } from "./restore";

const A = "01M6F00000AAAAAAAAAAAAAAAA";
const B = "01M6F11111BBBBBBBBBBBBBBBB";
const T = "01M6F22222TTTTTTTTTTTTTTTT";
const M = "01M6F33333MMMMMMMMMMMMMMMM";
const F = "01M6F44444FFFFFFFFFFFFFFFF";

const events = [
  { seq: 1, type: "topic.created", actorId: "u", at: 1, topicId: T, data: { topic: { id: T, groupId: A, rootMessageId: M } } },
  { seq: 2, type: "message.created", actorId: "u", at: 1, topicId: T, messageId: M, data: { message: { id: M, groupId: A, topicId: T, body: `see ${A}`, attachments: [{ fileId: F }] } } },
].map((e) => JSON.stringify(e)).join("\n");

test("a copy maps the group, topic and message ids, keeps file ids and text", () => {
  const map = copyIdMap(A, B, [events]);
  expect(map.get(A)).toBe(B);
  expect(map.has(F)).toBe(false);
  const out = events.split("\n").map((l) => remapValue(JSON.parse(l), map)) as any[];
  const t2 = out[0].data.topic.id;
  const m2 = out[1].data.message.id;
  expect(t2).not.toBe(T);
  expect(m2).not.toBe(M);
  expect(out[0]).toMatchObject({ topicId: t2, data: { topic: { groupId: B, rootMessageId: m2 } } });
  expect(out[1]).toMatchObject({ topicId: t2, messageId: m2, data: { message: { groupId: B, topicId: t2, attachments: [{ fileId: F }] } } });
  // Only whole values are ids; text that mentions an id stays as written.
  expect(out[1].data.message.body).toBe(`see ${A}`);
});

test("copied ids are stable per target group and keep the ULID time part", () => {
  expect(copiedId(B, T)).toBe(copiedId(B, T));
  expect(copiedId(B, T)).not.toBe(copiedId("01M6F55555CCCCCCCCCCCCCCCC", T));
  expect(copiedId(B, T)).toMatch(/^01M6F22222[0-9A-HJKMNP-TV-Z]{16}$/);
  expect(copiedId(B, "not-a-ulid")).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
});
