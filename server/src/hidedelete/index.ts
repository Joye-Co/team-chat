// Hide and delete (docs/protocol.md, "Hide and delete"): showing a hidden 群, 私聊 or 话题 again
// when a new message arrives, `topic.delete`, `DELETE /v1/groups/:id` and
// `DELETE /v1/members/:id`. Hiding itself is a per-member flag next to `muted`
// (notify/index.ts: `PATCH /v1/groups/:id/prefs` and `topic.hide`).
import { type TopicDeleteFrame, type TopicDeleteResult, type TopicState, mainTopicId } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { type MemberRow, getGroup, getMember, groupMemberIds, toMember } from "../core/db";
import { type Env, gatewayStub, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { logger } from "../core/logger";
import {
  registerFrameHandler,
  registerGatewayRpc,
  registerMessageWriteHook,
  registerRoomRpc,
  registerRoute,
} from "../core/registry";
import { notifyGroupChanged, notifyMembersChanged, removeMemberFromGroup } from "../core/routes";
import { type ReadTotals, groupReadState, topicState, unhideGroupExcept, unhideTopicFor } from "../core/topicstate";
import { deskStub } from "../bots/env";
import { fileKey } from "../files/routes";

const log = logger.child({ feature: "hidedelete" });

type Wait = (p: Promise<unknown>) => void;

// ---------------------------------------------------------------------------
// A new message shows what was hidden again
// ---------------------------------------------------------------------------

// In the send's commit: every member but the author sees the 群 or 私聊 again; mentioned
// members see the 话题 again and get `reads` with its state (clients clear it the same way).
registerMessageWriteHook({
  send(_frame, room) {
    let cleared: string[] = [];
    let topicId = "";
    return {
      inTx(_tx, message) {
        if (message.kind === "system") return;
        unhideGroupExcept(room.sql, message.authorId);
        if (message.topicId === mainTopicId(room.groupId)) return;
        topicId = message.topicId;
        cleared = unhideTopicFor(
          room.sql,
          message.topicId,
          message.mentions.filter((id) => id !== message.authorId),
        );
      },
      after() {
        for (const memberId of cleared) {
          room.waitUntil(
            gatewayStub(room.env, memberId)
              .rpc("hidedelete.reads", { groupId: room.groupId, topicId })
              .catch((err) => log.warn("reads after unhide failed", { memberId, err })),
          );
        }
      },
    };
  },
});

/** One member's state of a topic and the group totals, for `reads`. Internal (actor null). */
registerRoomRpc("hidedelete.topicState", (room, actor, args) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const a = args as { memberId: string; topicId: string; groupRead: number };
  if (!room.isMember(a.memberId)) throw new ApiError("forbidden", "not a member of this group");
  const last = room.sql.exec<{ s: number | null }>("SELECT max(seq) AS s FROM events").one().s ?? 0;
  const read = Math.min(a.groupRead, last);
  const { topics: _all, ...totals } = groupReadState(room.sql, a.memberId, read);
  return { ...totals, state: topicState(room.sql, a.memberId, a.topicId, read) };
});

/** Sends `reads` for one topic to every connection of this member (its cursor lives here). */
registerGatewayRpc("hidedelete.reads", async (gw, raw) => {
  const { groupId, topicId } = raw as { groupId: string; topicId: string };
  if (!gw.memberId || !gw.online()) return 0;
  const groupRead = gw.sql.exec<{ seq: number }>("SELECT seq FROM cursors WHERE group_id = ?", groupId).toArray()[0]?.seq ?? 0;
  const r = unwrap(
    await roomStub(gw.env, groupId).rpc(groupId, "hidedelete.topicState", null, { memberId: gw.memberId, topicId, groupRead }),
  ) as ReadTotals & { state: TopicState };
  gw.broadcast({ t: "reads", groupId, readSeq: r.readSeq, unread: r.unread, mentions: r.mentions, topics: [r.state] });
  return 1;
});

// ---------------------------------------------------------------------------
// Stopping the runs of a deleted topic or group
// ---------------------------------------------------------------------------

async function stopRuns(env: Env, groupId: string, topicId?: string): Promise<void> {
  const bots = topicId
    ? await env.DB.prepare("SELECT DISTINCT bot_id FROM bot_runs WHERE group_id = ? AND topic_id = ?").bind(groupId, topicId).all<{ bot_id: string }>()
    : await env.DB.prepare("SELECT DISTINCT bot_id FROM bot_runs WHERE group_id = ?").bind(groupId).all<{ bot_id: string }>();
  for (const { bot_id } of bots.results) {
    try {
      await deskStub(env, bot_id).stopRunsIn(bot_id, groupId, topicId);
    } catch (err) {
      log.warn("stop runs failed", { botId: bot_id, groupId, topicId, err });
    }
  }
}

// ---------------------------------------------------------------------------
// topic.delete
// ---------------------------------------------------------------------------

registerFrameHandler<TopicDeleteFrame>("topic.delete", {
  rate: "write",
  async handle(frame, c): Promise<TopicDeleteResult> {
    await c.requireMember(frame.groupId);
    const room = roomStub(c.env, frame.groupId);
    // Checks first (creator or owner, not the 群聊, known), then the topic's runs stop, so their
    // last messages are scrubbed with the rest.
    const checked = unwrap(await room.topicDelete(frame.groupId, c.actor, frame.topicId, true));
    if (checked.deletedAt !== 0) return { topicId: checked.topicId, deletedAt: checked.deletedAt };
    await stopRuns(c.env, frame.groupId, frame.topicId);
    const r = unwrap(await room.topicDelete(frame.groupId, c.actor, frame.topicId));
    if (r.deletedNow) {
      await auditStatement(c.env.DB, { memberId: c.actor.id, tokenId: c.actor.tokenId }, {
        action: "topic.delete",
        target: frame.groupId,
        details: { groupId: frame.groupId, topicId: r.topicId, messages: r.messages },
      }).run();
    }
    return { topicId: r.topicId, deletedAt: r.deletedAt };
  },
});

// ---------------------------------------------------------------------------
// DELETE /v1/groups/:id
// ---------------------------------------------------------------------------

/** Removes a 群 for everyone: D1 rows, the room's storage, its files; members get `removedGroupIds`. */
export async function deleteGroup(env: Env, waitUntil: Wait, a: AuthContext, groupId: string): Promise<void> {
  if (a.member.level !== "owner") throw new ApiError("forbidden", "owner only");
  const group = await getGroup(env.DB, groupId);
  if (!group) throw new ApiError("not_found", "group not found");
  if (group.kind === "dm") throw new ApiError("forbidden", "a DM cannot be deleted");
  if (group.is_default) throw new ApiError("forbidden", "the default group cannot be deleted");
  const memberIds = await groupMemberIds(env.DB, group.id);
  // While the room still answers: the runs post their last messages there.
  await stopRuns(env, group.id);
  const now = Date.now();
  const r = await env.DB.batch([
    env.DB.prepare("UPDATE groups SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL").bind(now, group.id),
    env.DB.prepare(
      `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
       SELECT ?, ?, 'group.delete', ?, ?, ? WHERE changes() = 1`,
    ).bind(a.memberId, a.tokenId, group.id, JSON.stringify({ name: group.name, members: memberIds }), now),
    env.DB.prepare("DELETE FROM group_members WHERE group_id = ?").bind(group.id),
    env.DB.prepare("UPDATE files SET deleted_at = ? WHERE group_id = ? AND deleted_at IS NULL").bind(now, group.id),
  ]);
  if (!r[0].meta.changes) throw new ApiError("not_found", "group not found");
  unwrap(await roomStub(env, group.id).wipe(group.id));
  // Not found in D1 any more: each gateway drops the group and pushes `removedGroupIds`.
  notifyGroupChanged(env, waitUntil, group.id, memberIds);
  waitUntil(deleteGroupFiles(env, group.id));
}

async function deleteGroupFiles(env: Env, groupId: string): Promise<void> {
  try {
    let cursor: string | undefined;
    do {
      const page = await env.FILES.list({ prefix: fileKey(groupId, ""), cursor });
      if (page.objects.length > 0) await env.FILES.delete(page.objects.map((o) => o.key));
      cursor = page.truncated ? page.cursor : undefined;
    } while (cursor);
  } catch (err) {
    log.warn("group files delete failed", { groupId, err });
  }
}

registerRoute({
  method: "DELETE",
  path: "/v1/groups/:id",
  auth: "member",
  async handler({ env, auth, params, ctx }) {
    await deleteGroup(env, (p) => ctx.waitUntil(p), auth as AuthContext, params.id);
    return { ok: true };
  },
});

// ---------------------------------------------------------------------------
// DELETE /v1/members/:id
// ---------------------------------------------------------------------------

registerRoute({
  method: "DELETE",
  path: "/v1/members/:id",
  auth: "member",
  async handler({ env, auth, params, ctx }) {
    const a = auth as AuthContext;
    if (a.member.level !== "owner") throw new ApiError("forbidden", "owner only");
    const target = await getMember(env.DB, params.id);
    if (!target || target.deleted_at) throw new ApiError("not_found", "member not found");
    if (target.kind !== "bot") throw new ApiError("forbidden", "only an AI member can be deleted");
    if (!target.deactivated) throw new ApiError("conflict", "deactivate the member first");
    const now = Date.now();
    const r = await env.DB.batch([
      env.DB.prepare("UPDATE members SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL AND deactivated = 1").bind(now, target.id),
      env.DB.prepare(
        `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
         SELECT ?, ?, 'member.delete', ?, ?, ? WHERE changes() = 1`,
      ).bind(a.memberId, a.tokenId, target.id, JSON.stringify({ name: target.name }), now),
      // Its runner and machine placements; sign-in codes for it end now.
      env.DB.prepare("DELETE FROM bot_runners WHERE member_id = ?").bind(target.id),
      env.DB.prepare("DELETE FROM machine_bots WHERE bot_id = ?").bind(target.id),
      env.DB.prepare("UPDATE invites SET expires_at = ? WHERE member_id = ? AND used_at IS NULL AND expires_at > ?").bind(now, target.id, now),
    ]);
    if (!r[0].meta.changes) throw new ApiError("conflict", "deactivate the member first");
    const waitUntil: Wait = (p) => ctx.waitUntil(p);
    // Out of every 群 (not its 私聊): `group.memberRemoved` in each.
    const groups = await env.DB.prepare(
      `SELECT g.id FROM groups g JOIN group_members gm ON gm.group_id = g.id
       WHERE gm.member_id = ? AND g.kind = 'group' AND g.deleted_at IS NULL ORDER BY g.id`,
    )
      .bind(target.id)
      .all<{ id: string }>();
    for (const g of groups.results) {
      try {
        await removeMemberFromGroup(env, waitUntil, a, g.id, target.id);
      } catch (err) {
        log.warn("remove deleted member from group failed", { groupId: g.id, memberId: target.id, err });
      }
    }
    const row = (await getMember(env.DB, target.id)) as MemberRow;
    // Also runs the members-changed hooks (features drop what they cached about it).
    await notifyMembersChanged(env, waitUntil, [row]);
    return toMember(row);
  },
});
