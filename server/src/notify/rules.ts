// The server's notification decision (docs/protocol.md, "Notifications"). Pure: no I/O, so the
// rules are unit-tested directly. The room calls it once per live event and recipient.
import type { Event, NotifyEvent, NotifyLevel, NotifyReason } from "@chat/protocol";

export interface NotifyInput {
  event: Event;
  /** The receiving member. */
  memberId: string;
  muted: boolean;
  level: NotifyLevel;
  /** The receiver follows the event's topic. */
  following: boolean;
  /** Members just added to the topic's waitingOn (topic.updated only). */
  waitingAdded: readonly string[];
}

/** The topic an event belongs to, if any. */
export function eventTopicId(event: Event): string | undefined {
  switch (event.type) {
    case "message.created":
      return event.data.message.topicId;
    case "topic.updated":
      return event.data.topicId;
    case "topic.created":
      return event.data.topic.id;
    case "artifact.created":
    case "artifact.versionAdded":
      return event.data.artifact.topicId;
    default:
      return undefined;
  }
}

/**
 * The first matching rule wins: mention, waiting, result, message. Nothing for my own events or
 * when my level is `none`. A muted group notifies only mentions.
 */
export function decideNotify(i: NotifyInput): NotifyReason | undefined {
  const { event, memberId } = i;
  if (event.actorId === memberId || i.level === "none") return undefined;

  const isMessage = event.type === "message.created" && (event.data.message.kind ?? "text") !== "system";
  if (isMessage && event.data.message.mentions.includes(memberId)) return "mention";
  if (i.muted) return undefined;

  if (event.type === "topic.updated" && event.data.waitingOn?.includes(memberId) && i.waitingAdded.includes(memberId)) {
    return "waiting";
  }
  if (i.following) {
    if (event.type === "topic.updated" && event.data.status === "done") return "result";
    if (event.type === "artifact.created" || event.type === "artifact.versionAdded") return "result";
  }
  if (isMessage && i.level === "all") return "message";
  return undefined;
}

/**
 * Settings: the notification and its switchable event (`notifyEvent`). The reasons `mention`,
 * `waiting` and `result` are their own events (a plain `message` has none). One more event
 * notifies with reason `message`, so older clients show it as a plain notification: `blocked`
 * (a topic I follow becomes blocked, 成员卡住或出错). The member's gateway then drops what the
 * member's settings switch off (settings/notify.ts).
 */
export function decideNotifyEvent(i: NotifyInput): { notify: NotifyReason; notifyEvent?: NotifyEvent } | undefined {
  const reason = decideNotify(i);
  if (reason === "mention" || reason === "waiting" || reason === "result") return { notify: reason, notifyEvent: reason };
  if (reason) return { notify: reason };
  const { event, memberId } = i;
  if (event.actorId === memberId || i.level === "none" || i.muted) return undefined;
  if (i.following && event.type === "topic.updated" && event.data.status === "blocked") return { notify: "message", notifyEvent: "blocked" };
  return undefined;
}

/** Event types that can notify; the room skips the follow and prefs lookups for the rest. */
export const NOTIFY_EVENT_TYPES: ReadonlySet<string> = new Set([
  "message.created",
  "topic.updated",
  "artifact.created",
  "artifact.versionAdded",
]);
