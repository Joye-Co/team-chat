// Hide and delete fixtures (docs/protocol.md, "Hide and delete"). Called by gen-fixtures.ts.
type Obj = Record<string, unknown>;
export type HideDeleteCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  T0: number;
  ID: Record<string, string>;
  base: Obj;
  generalSummary: Obj;
  botMember: Obj;
};

export function addHideDeleteFixtures({ ok, bad, T0, ID, base, generalSummary, botMember }: HideDeleteCtx) {
  ok("entities/group-summary-hidden.json", "GroupSummary", { ...generalSummary, hidden: true }, "I hid this group");
  ok("http/update-group-prefs-request-hidden.json", "UpdateGroupPrefsRequest", { hidden: true });
  bad("http/update-group-prefs-request-bad-hidden.json", "UpdateGroupPrefsRequest", { hidden: "yes" }, "hidden is a boolean");

  const state = { topicId: ID.topic, readSeq: 40, unread: 0, mentions: 0, hidden: true };
  ok("entities/topic-state-hidden.json", "TopicState", state, "I hid this topic");
  ok("client/topic.hide.json", "ClientFrame", { t: "topic.hide", rid: "r90", groupId: ID.general, topicId: ID.topic, hidden: true });
  ok("client/topic.hide-show.json", "ClientFrame", { t: "topic.hide", rid: "r91", groupId: ID.general, topicId: ID.topic, hidden: false });
  bad("client/topic.hide-missing-hidden.json", "ClientFrame", { t: "topic.hide", rid: "r92", groupId: ID.general, topicId: ID.topic }, "missing hidden");
  ok("server/ack-ok-topic-hide.json", "ServerFrame", { t: "ack", rid: "r90", ok: true, result: state }, "ack of topic.hide: result is a TopicState");

  ok("client/topic.delete.json", "ClientFrame", { t: "topic.delete", rid: "r93", groupId: ID.general, topicId: ID.topic });
  bad("client/topic.delete-missing-topic.json", "ClientFrame", { t: "topic.delete", rid: "r94", groupId: ID.general }, "missing topicId");
  ok("http/topic-delete-result.json", "TopicDeleteResult", { topicId: ID.topic, deletedAt: T0 + 9000 });
  ok("events/topic.deleted.json", "Event", { ...base, type: "topic.deleted", data: { topicId: ID.topic, deletedAt: T0 + 9000 } });
  bad("events/topic.deleted-missing-at.json", "Event", { ...base, type: "topic.deleted", data: { topicId: ID.topic } }, "missing deletedAt");

  ok("entities/member-deleted.json", "Member", { ...botMember, deactivated: true, deleted: true }, "a deleted AI member");
}
