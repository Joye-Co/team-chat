// Posts alerts and the daily summary into the ops group with the HTTP send endpoint
// (POST /v1/groups/:id/messages) of a healthy env. Messages are cards with a text fallback.
import type { Alert } from "./alerts";
import type { TargetSummary } from "./state";

export const ALERT_TOPIC_TITLE = "监测告警";

export interface Card {
  v: 1;
  title: string;
  tone?: "neutral" | "info" | "success" | "warning" | "danger";
  status?: { label: string };
  blocks: Record<string, unknown>[];
}

export interface Post {
  title: string;
  body: string;
  card: Card;
  /** Members to notify: the group's default notify level is "mentions". */
  mentions?: string[];
}

function minutes(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60000));
  return m < 120 ? `${m} 分钟` : `${(m / 60).toFixed(1)} 小时`;
}

function time(ms: number): string {
  return `${new Date(ms).toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

/** A 不可用 / 已恢复 card (or 备份过期 / 备份已恢复 for the archive check), mentioning `oncall`. */
export function alertPost(a: Alert, oncall: string[] = []): Post {
  const post = a.check === "backup" ? backupPost(a) : servicePost(a);
  return oncall.length > 0 ? { ...post, mentions: [...new Set(oncall)] } : post;
}

function backupPost(a: Alert): Post {
  if (a.kind === "down") {
    const title = `${a.target} 备份过期`;
    return {
      title,
      body: `【告警】${a.target} 每日归档没有按时完成：${a.error}（自 ${time(a.since)}）`,
      card: {
        v: 1,
        title,
        tone: "warning",
        status: { label: "告警" },
        blocks: [
          {
            type: "fields",
            fields: [
              { label: "环境", value: a.target },
              { label: "原因", value: a.error },
              { label: "开始", value: time(a.since) },
            ],
          },
          { type: "text", text: "处理步骤见 docs/RUNBOOK.md 的每日归档一节。" },
        ],
      },
    };
  }
  const title = `${a.target} 备份已恢复`;
  return {
    title,
    body: `【恢复】${a.target} 每日归档已完成。`,
    card: {
      v: 1,
      title,
      tone: "success",
      status: { label: "恢复" },
      blocks: [
        {
          type: "fields",
          fields: [
            { label: "环境", value: a.target },
            { label: "开始", value: time(a.downSince) },
            { label: "恢复", value: time(a.at) },
          ],
        },
      ],
    },
  };
}

function servicePost(a: Alert): Post {
  if (a.kind === "down") {
    const title = `${a.target} 不可用`;
    return {
      title,
      body: `【告警】${a.target} 连续 ${a.fails} 次检查失败：${a.error}（自 ${time(a.since)}）`,
      card: {
        v: 1,
        title,
        tone: "danger",
        status: { label: "告警" },
        blocks: [
          {
            type: "fields",
            fields: [
              { label: "环境", value: a.target },
              { label: "原因", value: a.error },
              { label: "开始", value: time(a.since) },
              { label: "连续失败", value: String(a.fails) },
            ],
          },
          { type: "text", text: "处理步骤见 docs/RUNBOOK.md 的监测告警一节。" },
        ],
      },
    };
  }
  const title = `${a.target} 已恢复`;
  return {
    title,
    body: `【恢复】${a.target} 已恢复，中断约 ${minutes(a.at - a.downSince)}。`,
    card: {
      v: 1,
      title,
      tone: "success",
      status: { label: "恢复" },
      blocks: [
        {
          type: "fields",
          fields: [
            { label: "环境", value: a.target },
            { label: "中断", value: minutes(a.at - a.downSince) },
            { label: "开始", value: time(a.downSince) },
            { label: "恢复", value: time(a.at) },
            { label: "最后的错误", value: a.error ?? "-" },
          ],
        },
      ],
    },
  };
}

export function summaryPost(date: string, rows: TargetSummary[]): Post {
  const pct = (x: number | null) => (x === null ? "-" : `${(x * 100).toFixed(2)}%`);
  const ms = (x: number | null) => (x === null ? "-" : `${x} ms`);
  const label = { ok: "正常", failing: "故障", unknown: "未知" } as const;
  const bad = rows.some((r) => r.status === "failing" || r.failures > 0);
  const title = `每日运行摘要 ${date}`;
  return {
    title,
    body: `${title}：${rows.map((r) => `${r.target} ${label[r.status]} 可用率 ${pct(r.uptime)}`).join("；")}`,
    card: {
      v: 1,
      title,
      tone: bad ? "warning" : "success",
      blocks: [
        {
          type: "table",
          columns: ["环境", "状态", "检查", "失败", "可用率", "往返 p50", "往返 p95"],
          rows: rows.map((r) => [
            r.target,
            label[r.status],
            String(r.checks),
            String(r.failures),
            pct(r.uptime),
            ms(r.p50),
            ms(r.p95),
          ]),
        },
        { type: "text", text: "过去 24 小时，每 5 分钟一次健康检查和一次消息往返。" },
      ],
    },
  };
}

export interface SendOutcome {
  ok: boolean;
  status: number;
  topicId: string | null;
  /** The topic is gone; retry without it. */
  topicMissing: boolean;
}

export async function sendPost(
  fetcher: typeof fetch,
  baseUrl: string,
  token: string,
  groupId: string,
  topicId: string | null,
  clientMsgId: string,
  post: Post,
): Promise<SendOutcome> {
  const body: Record<string, unknown> = { clientMsgId, body: post.body, card: post.card };
  if (post.mentions && post.mentions.length > 0) body.mentions = post.mentions;
  if (topicId) body.topicId = topicId;
  else body.title = ALERT_TOPIC_TITLE;
  try {
    const res = await fetcher(`${baseUrl}/v1/groups/${encodeURIComponent(groupId)}/messages`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const json = (await res.json().catch(() => null)) as any;
    if (!res.ok) {
      return { ok: false, status: res.status, topicId, topicMissing: res.status === 404 && !!topicId && /topic/i.test(json?.error?.message ?? "") };
    }
    return { ok: true, status: res.status, topicId: json?.message?.topicId ?? json?.topic?.id ?? topicId, topicMissing: false };
  } catch {
    return { ok: false, status: 0, topicId, topicMissing: false };
  }
}
