import type { Event } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { type NotifyInput, decideNotify, eventTopicId } from "../src/notify/rules";

const ME = "01ME";
const OTHER = "01OTHER";

function message(opts: { mentions?: string[]; actor?: string; kind?: "text" | "card" | "system" } = {}): Event {
  return {
    seq: 2,
    groupId: "g",
    at: 1,
    actorId: opts.actor ?? OTHER,
    type: "message.created",
    data: {
      message: {
        id: "m",
        clientMsgId: "c",
        groupId: "g",
        topicId: "t",
        authorId: opts.actor ?? OTHER,
        body: "hi",
        mentions: opts.mentions ?? [],
        createdSeq: 2,
        createdAt: 1,
        kind: opts.kind,
      },
    },
  } as Event;
}

function topicUpdated(data: Record<string, unknown>, actor = OTHER): Event {
  return { seq: 3, groupId: "g", at: 1, actorId: actor, type: "topic.updated", data: { topicId: "t", ...data } } as Event;
}

const artifactEvent = {
  seq: 4,
  groupId: "g",
  at: 1,
  actorId: OTHER,
  type: "artifact.created",
  data: {
    artifact: {
      id: "a",
      groupId: "g",
      topicId: "t",
      title: "x",
      kind: "file",
      currentVersion: 1,
      createdBy: OTHER,
      createdAt: 1,
      updatedAt: 1,
    },
    version: { artifactId: "a", version: 1, entry: "x", files: [], createdBy: OTHER, createdAt: 1 },
  },
} as unknown as Event;

function d(event: Event, over: Partial<NotifyInput> = {}) {
  return decideNotify({ event, memberId: ME, muted: false, level: "mentions", following: false, waitingAdded: [], ...over });
}

describe("notification rules", () => {
  it("never notify my own events or level none", () => {
    expect(d(message({ actor: ME, mentions: [ME] }))).toBeUndefined();
    expect(d(message({ mentions: [ME] }), { level: "none" })).toBeUndefined();
  });

  it("mention wins over every other rule, also in a muted group", () => {
    expect(d(message({ mentions: [ME] }), { level: "all", following: true })).toBe("mention");
    expect(d(message({ mentions: [ME] }), { muted: true })).toBe("mention");
    expect(d(message({ mentions: [OTHER] }))).toBeUndefined();
  });

  it("waiting only when I was just added to waitingOn", () => {
    expect(d(topicUpdated({ waitingOn: [ME] }), { waitingAdded: [ME] })).toBe("waiting");
    expect(d(topicUpdated({ waitingOn: [ME] }), { waitingAdded: [] })).toBeUndefined();
    expect(d(topicUpdated({ waitingOn: [OTHER] }), { waitingAdded: [OTHER] })).toBeUndefined();
    expect(d(topicUpdated({ waitingOn: [ME] }), { waitingAdded: [ME], muted: true })).toBeUndefined();
  });

  it("result for followers on status done and on new 产出", () => {
    expect(d(topicUpdated({ status: "done" }), { following: true })).toBe("result");
    expect(d(topicUpdated({ status: "done" }), { following: false })).toBeUndefined();
    expect(d(topicUpdated({ status: "blocked" }), { following: true })).toBeUndefined();
    expect(d(artifactEvent, { following: true })).toBe("result");
    expect(d({ ...artifactEvent, type: "artifact.versionAdded" } as Event, { following: true })).toBe("result");
    expect(d(artifactEvent, { following: true, muted: true })).toBeUndefined();
    expect(d(artifactEvent, { following: false })).toBeUndefined();
  });

  it("message only with level all, never for system messages", () => {
    expect(d(message(), { level: "all" })).toBe("message");
    expect(d(message({ kind: "card" }), { level: "all" })).toBe("message");
    expect(d(message(), { level: "mentions" })).toBeUndefined();
    expect(d(message({ kind: "system" }), { level: "all" })).toBeUndefined();
    expect(d(message({ kind: "system", mentions: [ME] }), { level: "all" })).toBeUndefined();
    expect(d(message(), { level: "all", muted: true })).toBeUndefined();
  });

  it("find the topic of an event", () => {
    expect(eventTopicId(message())).toBe("t");
    expect(eventTopicId(topicUpdated({}))).toBe("t");
    expect(eventTopicId(artifactEvent)).toBe("t");
    expect(
      eventTopicId({ seq: 1, groupId: "g", at: 1, actorId: OTHER, type: "group.updated", data: { name: "x" } } as Event),
    ).toBeUndefined();
  });
});
