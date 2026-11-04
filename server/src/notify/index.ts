// Notifications, topic follow, group prefs and card presses; members admin, audit and HTTP send
// live in the sibling files. Registered at import time from src/features.ts.
import {
  type CardActionFrame,
  type Event,
  type GroupSummary,
  type TopicFollowFrame,
  type TopicState,
  UpdateGroupPrefsRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { getGroup, isGroupMember } from "../core/db";
import { gatewayStub, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import {
  registerEventDecorator,
  registerFrameHandler,
  registerRoomRpc,
  registerRoute,
} from "../core/registry";
import { summaryFor } from "../core/routes";
import {
  type ReadTotals,
  getPrefs,
  groupReadState,
  isFollowing,
  setFollowing,
  setPrefs,
  topicState,
} from "../core/topicstate";
import { NOTIFY_EVENT_TYPES, decideNotifyEvent, eventTopicId } from "./rules";
import "./members";
import "./messages";

// ---------------------------------------------------------------------------
// The notify field on live event frames
// ---------------------------------------------------------------------------

registerEventDecorator((event, memberId, c) => {
  if (event.actorId === memberId || !NOTIFY_EVENT_TYPES.has(event.type)) return undefined;
  const prefs = getPrefs(c.sql, memberId, c.groupKind);
  const topicId = eventTopicId(event as Event);
  return decideNotifyEvent({
    event: event as Event,
    memberId,
    muted: prefs.muted,
    level: prefs.notify,
    following: topicId ? isFollowing(c.sql, memberId, topicId) : false,
    waitingAdded: (c.hints?.waitingAdded as string[] | undefined) ?? [],
  });
});

// ---------------------------------------------------------------------------
// topic.follow
// ---------------------------------------------------------------------------

function lastSeq(sql: SqlStorage): number {
  return sql.exec<{ s: number | null }>("SELECT max(seq) AS s FROM events").one().s ?? 0;
}

registerRoomRpc("topic.follow", (room, actor, args) => {
  const { topicId, follow, groupRead } = args as { topicId: string; follow: boolean; groupRead: number };
  if (!actor) throw new ApiError("internal", "actor required");
  if (room.sql.exec("SELECT 1 FROM topics WHERE id = ?", topicId).toArray().length === 0) {
    throw new ApiError("not_found", "topic not found");
  }
  const read = Math.min(groupRead, lastSeq(room.sql));
  setFollowing(room.sql, actor.id, topicId, follow);
  const { topics: _all, ...totals } = groupReadState(room.sql, actor.id, read);
  return { ...totals, state: topicState(room.sql, actor.id, topicId, read) };
});

/** The member's group read cursor, from the gateway's own SQLite (core table `cursors`). */
function groupCursor(sql: SqlStorage, groupId: string): number {
  return sql.exec<{ seq: number }>("SELECT seq FROM cursors WHERE group_id = ?", groupId).toArray()[0]?.seq ?? 0;
}

registerFrameHandler<TopicFollowFrame>("topic.follow", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const r = unwrap(
      await roomStub(c.env, frame.groupId).rpc(frame.groupId, "topic.follow", c.actor, {
        topicId: frame.topicId,
        follow: frame.follow,
        groupRead: groupCursor(c.sql, frame.groupId),
      }),
    ) as ReadTotals & { state: TopicState };
    c.broadcast({
      t: "reads",
      groupId: frame.groupId,
      readSeq: r.readSeq,
      unread: r.unread,
      mentions: r.mentions,
      topics: [r.state],
    });
    return r.state;
  },
});

// ---------------------------------------------------------------------------
// card.action
// ---------------------------------------------------------------------------

registerFrameHandler<CardActionFrame>("card.action", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    unwrap(await roomStub(c.env, frame.groupId).cardAction(frame.groupId, c.actor, frame.messageId, frame.actionId));
    return undefined;
  },
});

// ---------------------------------------------------------------------------
// PATCH /v1/groups/:id/prefs
// ---------------------------------------------------------------------------

registerRoomRpc("prefs.set", (room, actor, args) => {
  if (!actor) throw new ApiError("internal", "actor required");
  const change = args as { muted?: boolean; notify?: "all" | "mentions" | "none" };
  const before = getPrefs(room.sql, actor.id, room.groupKind);
  setPrefs(room.sql, actor.id, change);
  return { before, after: getPrefs(room.sql, actor.id, room.groupKind) };
});

registerRoute({
  method: "PATCH",
  path: "/v1/groups/:id/prefs",
  auth: "member",
  async handler({ env, auth, params, body }) {
    const a = auth as AuthContext;
    const req = await body(UpdateGroupPrefsRequest);
    const group = await getGroup(env.DB, params.id);
    if (!group) throw new ApiError("not_found", "group not found");
    if (!(await isGroupMember(env.DB, group.id, a.memberId))) throw new ApiError("forbidden", "not a member of this group");
    const actor = { id: a.memberId, level: a.member.level, kind: a.member.kind, tokenId: a.tokenId };
    const r = unwrap(await roomStub(env, group.id).rpc(group.id, "prefs.set", actor, req)) as {
      before: { muted: boolean; notify: string };
      after: { muted: boolean; notify: string };
    };
    const details: Record<string, unknown> = { groupId: group.id };
    if (r.before.muted !== r.after.muted) details.muted = [r.before.muted, r.after.muted];
    if (r.before.notify !== r.after.notify) details.notify = [r.before.notify, r.after.notify];
    await auditStatement(env.DB, a, { action: "prefs.update", target: group.id, details }).run();
    const summary: GroupSummary = await summaryFor(env, a.memberId, group.id);
    // Every connection of mine gets the new summary (an HTTP caller has none of its own).
    await gatewayStub(env, a.memberId).deliver([{ t: "directory", groups: [summary] }]);
    return summary;
  },
});
