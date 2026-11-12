// 机 5.1: a computer that needs its managers tells them (docs/protocol.md, "Machine alerts").
// MachineLink keeps which alerts are up (offline for 10 minutes while an AI member depends on it,
// a CLI that was signed in is signed out, free disk below the limit) and calls these when one
// starts or ends. The notice is a message of an AI member that uses the computer in its DM with
// each manager (a team computer: the admins; a personal one: its owner), mentioning them, so the
// usual notification and push carry it; when the alert ends the message says so and its 话题 is
// done, so nothing is left waiting.
import { MIN_FREE_DISK_BYTES } from "../bots/dispatch";
import type { BotsEnv } from "../bots/env";
import { roomStub } from "../core/env";
import { unwrap } from "../core/errors";
import { logger } from "../core/logger";
import type { Actor } from "../core/registry";
import { findOrCreateDm } from "../core/routes";

/** An offline computer an AI member depends on is reported after this long. */
export const ALERT_OFFLINE_MS = 10 * 60_000;
/** A disk alert ends only above this much free space, so a disk at the line does not flap. */
export const DISK_CLEAR_BYTES = MIN_FREE_DISK_BYTES + 1024 ** 3;
/** At most this many people hear of one alert. */
const MAX_RECIPIENTS = 20;

export type AlertKind = "offline" | "signed_out" | "disk";

/** One alert that is up. `key` is the kind, with the runtime for a sign-in. */
export interface MachineAlert {
  key: string;
  kind: AlertKind;
  since: number;
  /** signed_out: the runtime and the CLI's name. */
  runtime?: string;
  cli?: string;
  /** disk: free bytes when it started. */
  freeBytes?: number;
}

/** Where one person was told, so the notice can be withdrawn. */
export interface AlertNote {
  /** The AI member that said it. */
  botId: string;
  memberId: string;
  groupId: string;
  topicId: string;
  messageId: string;
  body: string;
}

const gb = (bytes: number) => String(Math.max(0, Math.floor((bytes / 1024 ** 3) * 10) / 10));

/** The words of the notices. */
export const ALERT_TEXT = {
  offlineTitle: (machine: string) => `电脑「${machine}」离线了`,
  offline: (machine: string, mins: number, bots: string[]) =>
    `电脑「${machine}」离线 ${mins} 分钟了，交给 ${bots.join("、")} 的活要等它回来才能开始。请看看它是不是关机、睡眠或者断网了。`,
  signedOutTitle: (machine: string, cli: string) => `电脑「${machine}」上的 ${cli} 退出登录了`,
  signedOut: (machine: string, cli: string, admin: boolean) =>
    `电脑「${machine}」上的 ${cli} 没有登录了，用它的活现在做不了。` +
    (admin ? `请到 团队管理 › 账号与密钥 › 订阅账号，选 ${machine}，点「重新登录」。` : `请在这台电脑上重新登录 ${cli}。`),
  diskTitle: (machine: string) => `电脑「${machine}」的磁盘快满了`,
  disk: (machine: string, free: number) =>
    `电脑「${machine}」只剩 ${gb(free)} GB 可用，少于 ${gb(MIN_FREE_DISK_BYTES)} GB，新的活不会派到这台电脑。请清理一些空间。`,
  recovered: "已经恢复，不用处理了。",
};

/** The alerts a heartbeat starts and ends, from the latest sample and what the link saw before. */
export function heartbeatAlerts(input: {
  up: Record<string, MachineAlert>;
  health: { diskFreeBytes: number; diskTotalBytes: number };
  /** The heartbeat's sign-ins; absent when it had no metrics the server could read. */
  logins?: { runtime: string; state: string; confirmed?: boolean }[];
  /** The last state each runtime was seen in ("logged_in" | "logged_out"). */
  seen: Record<string, string>;
  /**
   * The daemon has feature "sign-in-confirmed": only a `logged_in` with `confirmed` is certain; one
   * without may come from the credential files while the CLI is still signed out.
   */
  confirmsSignIn?: boolean;
  cliName: (runtime: string) => string;
  now: number;
}): { raise: MachineAlert[]; clear: string[]; seen: Record<string, string> } {
  const raise: MachineAlert[] = [];
  const clear: string[] = [];
  const h = input.health;
  if (h.diskTotalBytes > 0 && h.diskFreeBytes < MIN_FREE_DISK_BYTES) {
    if (!input.up.disk) raise.push({ key: "disk", kind: "disk", since: input.now, freeBytes: h.diskFreeBytes });
  } else if (input.up.disk && (h.diskTotalBytes === 0 || h.diskFreeBytes >= DISK_CLEAR_BYTES)) {
    clear.push("disk");
  }
  const seen = { ...input.seen };
  for (const l of input.logins ?? []) {
    const key = `signed_out:${l.runtime}`;
    if (l.state === "logged_in") {
      seen[l.runtime] = "logged_in";
      if (input.up[key] && (!input.confirmsSignIn || l.confirmed === true)) clear.push(key);
    } else if (l.state === "logged_out" && l.confirmed === true) {
      // Only a sign-in that was there and went: a CLI nobody ever signed in is not news.
      if (input.seen[l.runtime] === "logged_in" && !input.up[key]) {
        raise.push({ key, kind: "signed_out", since: input.now, runtime: l.runtime, cli: input.cliName(l.runtime) });
      }
      seen[l.runtime] = "logged_out";
    }
  }
  // A CLI the heartbeat no longer reports (removed from the config, uninstalled) has nothing left
  // to recover: its alert ends.
  if (input.logins) {
    const reported = new Set(input.logins.map((l) => l.runtime));
    for (const key of Object.keys(input.up)) {
      const a = input.up[key];
      if (a?.kind === "signed_out" && a.runtime !== undefined && !reported.has(a.runtime)) clear.push(key);
    }
  }
  return { raise, clear, seen };
}

interface Person {
  id: string;
  level: string;
}

/** Who manages the computer: a personal one's owner, else the team's active admins. */
async function managers(env: BotsEnv, ownerId: string | null): Promise<Person[]> {
  const rows = ownerId
    ? await env.DB.prepare("SELECT id, level FROM members WHERE id = ? AND kind = 'human' AND deactivated = 0").bind(ownerId).all<Person>()
    : await env.DB.prepare(`SELECT id, level FROM members WHERE kind = 'human' AND deactivated = 0 AND level = 'owner' ORDER BY id LIMIT ${MAX_RECIPIENTS}`).all<Person>();
  return rows.results;
}

/**
 * The AI members that depend on the computer: placed on it, or checked on it by its owner. For a
 * sign-in or the disk, a team computer also serves the AI members placed nowhere.
 */
async function dependents(env: BotsEnv, machineId: string, personal: boolean, kind: AlertKind): Promise<{ id: string; name: string }[]> {
  const rows = await env.DB.prepare(
    `SELECT m.id, m.name FROM members m
     WHERE m.kind = 'bot' AND m.deactivated = 0 AND (
       m.id IN (SELECT member_id FROM bot_runners WHERE machine_id = ? AND enabled = 1)
       OR m.id IN (SELECT bot_id FROM machine_bots WHERE machine_id = ?))
     ORDER BY m.created_at, m.id LIMIT 20`,
  )
    .bind(machineId, machineId)
    .all<{ id: string; name: string }>();
  if (rows.results.length > 0 || kind === "offline" || personal) return rows.results;
  const free = await env.DB.prepare(
    `SELECT m.id, m.name FROM members m JOIN bot_runners r ON r.member_id = m.id
     WHERE m.deactivated = 0 AND r.enabled = 1 AND r.machine_id IS NULL ORDER BY m.created_at, m.id LIMIT 20`,
  ).all<{ id: string; name: string }>();
  return free.results;
}

/** Tells the computer's managers about `alert`; returns where, empty when nobody depends on it. */
export async function notifyAlert(
  env: BotsEnv,
  machineId: string,
  alert: MachineAlert,
  now = Date.now(),
  waitUntil?: (p: Promise<unknown>) => void,
): Promise<AlertNote[]> {
  const machine = await env.DB.prepare("SELECT name, owner_id, revoked_at FROM machines WHERE id = ?")
    .bind(machineId)
    .first<{ name: string; owner_id: string | null; revoked_at: number | null }>();
  if (!machine || machine.revoked_at !== null) return [];
  const bots = await dependents(env, machineId, machine.owner_id !== null, alert.kind);
  if (bots.length === 0) return [];
  const speaker = bots[0];
  const actor: Actor = { id: speaker.id, level: "member", kind: "bot" };
  const notes: AlertNote[] = [];
  for (const p of await managers(env, machine.owner_id)) {
    const cli = alert.cli ?? alert.runtime ?? "";
    const [title, body] =
      alert.kind === "offline"
        ? [ALERT_TEXT.offlineTitle(machine.name), ALERT_TEXT.offline(machine.name, Math.max(1, Math.round((now - alert.since) / 60_000)), bots.map((b) => b.name))]
        : alert.kind === "signed_out"
          ? [ALERT_TEXT.signedOutTitle(machine.name, cli), ALERT_TEXT.signedOut(machine.name, cli, p.level === "owner")]
          : [ALERT_TEXT.diskTitle(machine.name), ALERT_TEXT.disk(machine.name, alert.freeBytes ?? 0)];
    try {
      const groupId = await findOrCreateDm(env, (w) => waitUntil?.(w), { memberId: speaker.id }, p.id);
      const sent = unwrap(
        await roomStub(env, groupId).send(groupId, actor, {
          t: "send",
          rid: "alert",
          groupId,
          title,
          // One notice per alert and person, also when a retry sends it again.
          clientMsgId: `alert:${machineId.slice(-12)}:${alert.key.slice(0, 32)}:${alert.since}`.slice(0, 64),
          body,
          mentions: [p.id],
        } as never),
      );
      notes.push({ botId: speaker.id, memberId: p.id, groupId, topicId: sent.message.topicId, messageId: sent.message.id, body });
    } catch (err) {
      // One person not told never keeps the others from hearing.
      logger.warn("machine alert not sent", { machineId, kind: alert.kind, memberId: p.id, err });
    }
  }
  return notes;
}

/** The alert ended: each notice says so and its 话题 is done. */
export async function withdrawAlert(env: BotsEnv, notes: AlertNote[]): Promise<void> {
  for (const n of notes) {
    const actor: Actor = { id: n.botId, level: "member", kind: "bot" };
    const room = roomStub(env, n.groupId);
    try {
      unwrap(await room.edit(n.groupId, actor, { t: "edit", rid: "alert", groupId: n.groupId, messageId: n.messageId, body: `${n.body}\n\n${ALERT_TEXT.recovered}` } as never));
      unwrap(await room.topicUpdate(n.groupId, actor, { t: "topic.update", rid: "alert", groupId: n.groupId, topicId: n.topicId, status: "done" } as never));
    } catch (err) {
      // Deleted meanwhile, or the AI member left: nothing more to withdraw there.
      logger.warn("machine alert not withdrawn", { groupId: n.groupId, err });
    }
  }
}
