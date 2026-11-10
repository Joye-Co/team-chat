//! 团队管理 data for the screenshot self-test: each machine's monitor, 用量, the roster, 活动记录
//! and a machine's API keys, for the sample team of `fake.rs`. Only the self-test reads it.

use chat_core::proto::{
    ActivityResponse, MachineMonitor, TeamRoster, TeamUsage, monitor::MachineSecretsResponse,
};
use serde_json::{Value, json};

use super::{ME, now_ms};

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn now() -> u64 {
    now_ms().max(0) as u64
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).expect("fake 团队管理 sample matches the protocol")
}

/// `n` samples of a gentle wave around `base`.
fn wave(n: usize, base: f64, swing: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let x = i as f64 / 3.;
            ((base + swing * x.sin() + swing * 0.4 * (x * 2.3).cos()) * 10.).round() / 10.
        })
        .collect()
}

pub fn machine_monitor(machine_id: &str, range: &str) -> MachineMonitor {
    let now = now();
    let (n, step) = match range {
        "1h" => (60, MIN),
        "24h" => (96, 15 * MIN),
        _ => (30, 30_000),
    };
    if machine_id != "mc-mini" {
        return parse(json!({
            "machine": {
                "id": machine_id, "name": "旧笔记本", "connected": false,
                "lastHeartbeatAt": now - 3 * DAY, "os": "macos", "arch": "aarch64",
                "daemonVersion": "0.3.2"
            },
            "series": { "range": range, "from": now - n as u64 * step, "stepMs": step,
                        "cpuPct": [], "memPct": [], "diskPct": [], "netRxBps": [], "netTxBps": [] },
            "capacity": { "slots": 1, "running": 0, "canTake": 0 },
            "queue": { "count": 0 },
            "today": { "finished": 0, "succeeded": 0, "failed": 0, "finishedYesterday": 0, "tokens": 0 },
            "clis": [], "runs": [], "members": []
        }));
    }
    let gib = 1024_u64 * 1024 * 1024;
    parse(json!({
        "machine": {
            "id": machine_id, "name": "办公室 Mac mini", "connected": true,
            "lastHeartbeatAt": now - 8_000, "connectedAt": now - 3 * HOUR,
            "daemonVersion": "0.4.0", "os": "macos", "arch": "aarch64"
        },
        "sampledAt": now - 4_000,
        "host": {
            "cpuPct": 46.5, "cpuThreads": 10, "load1": 3.2,
            "memTotalBytes": 32 * gib, "memAvailableBytes": 13 * gib, "memVmBytes": 4 * gib,
            "diskTotalBytes": 494 * gib, "diskFreeBytes": 212 * gib,
            "netRxBps": 1_250_000.0, "netTxBps": 310_000.0, "uptimeSecs": 3 * 86_400
        },
        "series": {
            "range": range, "from": now - n as u64 * step, "stepMs": step,
            "cpuPct": wave(n, 42., 14.), "memPct": wave(n, 58., 6.),
            "diskPct": wave(n, 57., 0.5), "netRxBps": wave(n, 900_000., 400_000.),
            "netTxBps": wave(n, 250_000., 90_000.)
        },
        "capacity": { "slots": 4, "running": 2, "canTake": 2 },
        "queue": { "count": 1, "oldestAt": now - 3 * MIN },
        "today": { "finished": 38, "succeeded": 35, "failed": 3, "finishedYesterday": 31, "tokens": 4_200_000 },
        "clis": [
            { "runtime": "claude", "kind": "claude", "available": true, "version": "2.1.0",
              "login": "logged_in", "failedToday": 0, "method": "subscription",
              "email": "team@example.com", "plan": "Max" },
            { "runtime": "codex", "kind": "codex", "available": true, "version": "0.46.0",
              "login": "logged_out", "failedToday": 2 }
        ],
        "runs": [
            { "runId": "r-1", "memberId": "m-coder", "state": "running", "startedAt": now - 12 * MIN,
              "cpuCores": 1.4, "memBytes": 2 * gib },
            { "runId": "r-2", "memberId": "m-reviewer", "state": "waiting", "startedAt": now - 6 * MIN },
            { "runId": "r-3", "memberId": "m-docs", "state": "queued" }
        ],
        "members": [
            { "memberId": "m-coder", "running": 1, "waiting": 0, "queued": 0, "cpuCores": 1.4,
              "memBytes": 2 * gib, "tokensToday": 2_100_000, "finishedToday": 17 },
            { "memberId": "m-reviewer", "running": 0, "waiting": 1, "queued": 0, "cpuCores": 0.2,
              "memBytes": gib / 2, "tokensToday": 1_300_000, "finishedToday": 12 },
            { "memberId": "m-docs", "running": 0, "waiting": 0, "queued": 1, "cpuCores": 0.0,
              "memBytes": 0, "tokensToday": 800_000, "finishedToday": 9 }
        ],
        "lastFailure": { "runId": "r-9", "memberId": "m-taro", "at": now - 2 * HOUR,
                         "title": "把这周的微博排期发出来" }
    }))
}

fn spend(cost: f64, topics: u32) -> Value {
    json!({
        "costUsd": cost, "inputTokens": (cost * 9_000.) as u64, "outputTokens": (cost * 900.) as u64,
        "runs": topics + topics / 2, "topics": topics
    })
}

fn with(id_key: &str, id: &str, cost: f64, topics: u32, extra: Value) -> Value {
    let mut v = spend(cost, topics);
    v[id_key] = json!(id);
    if let Value::Object(m) = extra {
        for (k, x) in m {
            v[k] = x;
        }
    }
    v
}

fn pair(initiator: &str, member: &str, cost: f64, topics: u32) -> Value {
    let mut v = with("memberId", member, cost, topics, json!({}));
    v["initiatorId"] = json!(initiator);
    v
}

pub fn team_usage(from: u64, to: u64) -> TeamUsage {
    let bots = [
        ("m-coder", 86.4, 31_u32, Some(10_000_u64)),
        ("m-reviewer", 41.2, 18, Some(6_000)),
        ("m-docs", 24.8, 12, None),
        ("m-tata", 12.1, 6, Some(5_000)),
        ("m-taro", 6.3, 4, None),
    ];
    let days = ((to - from) / DAY).clamp(1, 31) as usize;
    let shape = [0.18, 0.22, 0.16, 0.2, 0.14, 0.06, 0.04];
    let elapsed = ((now().min(to) - from) / DAY + 1).min(days as u64) as usize;
    let daily: Vec<Value> = (0..days)
        .map(|d| {
            let share = if d < elapsed { shape[d % 7] } else { 0. };
            json!({
                "start": from + d as u64 * DAY,
                "costUsd": (bots.iter().map(|b| b.1).sum::<f64>() * share * 100.).round() / 100.,
                "bots": bots.iter().map(|b| json!({ "memberId": b.0, "costUsd": (b.1 * share * 100.).round() / 100. })).collect::<Vec<_>>()
            })
        })
        .collect();
    let total: f64 = bots.iter().map(|b| b.1).sum();
    parse(json!({
        "from": from, "to": to,
        "total": spend(total, 71),
        "previous": { "costUsd": 151.0, "topics": 63 },
        "teamBudgetCents": 21_000,
        "bots": bots.iter().map(|b| with("memberId", b.0, b.1, b.2,
            b.3.map(|c| json!({ "budgetCents": c })).unwrap_or(json!({})))).collect::<Vec<_>>(),
        "initiators": [
            with("memberId", ME, 74.1, 26, json!({})),
            with("memberId", "m-xiaowang", 41.8, 17, json!({})),
            with("memberId", "m-lixue", 33.2, 14, json!({})),
            with("memberId", "m-alex", 21.7, 14, json!({})),
        ],
        "machines": [ with("machineId", "mc-mini", total, 71, json!({})) ],
        "pairs": [
            pair(ME, "m-coder", 38.2, 11), pair(ME, "m-reviewer", 16.0, 7), pair(ME, "m-docs", 12.4, 5),
            pair("m-xiaowang", "m-coder", 30.1, 9), pair("m-xiaowang", "m-reviewer", 11.7, 6),
            pair("m-lixue", "m-tata", 12.1, 6), pair("m-lixue", "m-docs", 12.4, 5), pair("m-lixue", "m-reviewer", 8.7, 3),
            pair("m-alex", "m-coder", 18.1, 11), pair("m-alex", "m-taro", 3.6, 3),
        ],
        "runtimes": [
            with("runtime", "claude", 152.6, 58, json!({ "machineId": "mc-mini" })),
            with("runtime", "codex", 18.2, 13, json!({ "machineId": "mc-mini" })),
        ],
        "daily": daily
    }))
}

pub fn team_roster() -> TeamRoster {
    let now = now();
    let joined = |id: &str, days: u64, by: Option<&str>, seen: u64, devices: Value| {
        let mut v = json!({ "memberId": id, "joinedAt": now - days * DAY, "lastSeenAt": now - seen, "devices": devices });
        if let Some(b) = by {
            v["addedBy"] = json!(b);
        }
        v
    };
    parse(json!({
        "members": [
            joined(ME, 60, None, 0, json!([
                { "label": "Joye 的 MacBook Pro", "createdAt": now - 60 * DAY, "lastUsedAt": now },
                { "label": "办公室 iMac", "createdAt": now - 20 * DAY, "lastUsedAt": now - 2 * DAY }
            ])),
            joined("m-xiaowang", 30, Some(ME), 5 * MIN, json!([
                { "label": "小王的 MacBook Air", "createdAt": now - 30 * DAY, "lastUsedAt": now - 5 * MIN }
            ])),
            joined("m-lixue", 24, Some(ME), 3 * HOUR, json!([])),
            joined("m-chenchen", 12, Some(ME), DAY, json!([])),
            joined("m-alex", 9, Some(ME), 20 * MIN, json!([])),
            joined("m-coder", 45, Some(ME), 0, json!([])),
            joined("m-reviewer", 40, Some(ME), 0, json!([])),
            joined("m-docs", 50, Some(ME), 0, json!([])),
        ],
        "invites": [
            { "id": "inv-1", "name": "Lina", "kind": "human", "level": "member", "role": "运营",
              "createdBy": ME, "createdAt": now - 5 * DAY, "expiresAt": now + DAY + 4 * HOUR },
            { "id": "inv-2", "name": "周周", "kind": "human", "level": "member",
              "createdBy": ME, "createdAt": now - DAY, "expiresAt": now + 6 * DAY }
        ]
    }))
}

pub fn team_activity() -> ActivityResponse {
    let now = now();
    let e = |id: u64,
             ago: u64,
             kind: &str,
             action: &str,
             actor: &str,
             target: &str,
             extra: Value| {
        let mut v = json!({ "id": id, "at": now - ago, "kind": kind, "action": action, "actorId": actor, "target": target });
        if let Value::Object(m) = extra {
            for (k, x) in m {
                v[k] = x;
            }
        }
        v
    };
    parse(json!({
        "entries": [
            e(9, 4 * MIN, "approval", "run.approval", ME, "m-reviewer", json!({ "details": { "decision": "approved" } })),
            e(8, 50 * MIN, "key", "machine.secret.set", ME, "mc-mini",
              json!({ "machineId": "mc-mini", "details": { "name": "OPENAI_API_KEY", "members": ["m-coder"] } })),
            e(7, 2 * HOUR, "login", "machine.login.start", ME, "mc-mini",
              json!({ "machineId": "mc-mini", "details": { "runtime": "claude" } })),
            e(6, 5 * HOUR, "budget", "member.update", ME, "m-coder",
              json!({ "details": { "weeklyBudgetCents": [8000, 10000] } })),
            e(5, DAY + 2 * HOUR, "invite", "invite.create", ME, "inv-1", json!({ "details": { "name": "Lina" } })),
            e(4, DAY + 5 * HOUR, "level", "member.update", ME, "m-tata",
              json!({ "details": { "level": ["member", "owner"] } })),
            e(3, 2 * DAY, "member", "invite.redeem", "m-alex", "m-alex", json!({})),
            e(2, 3 * DAY, "machine", "machine.create", ME, "mc-mini",
              json!({ "machineId": "mc-mini", "details": { "name": "办公室 Mac mini" } })),
        ],
        "hasMore": false
    }))
}

pub fn machine_secrets() -> MachineSecretsResponse {
    let now = now();
    parse(json!({
        "publicKey": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        "secrets": [
            { "name": "OPENAI_API_KEY", "last4": "x9Qz", "members": ["m-coder"], "updatedAt": now - 50 * MIN },
            { "name": "ANTHROPIC_API_KEY", "last4": "7fKe", "members": [], "updatedAt": now - 6 * DAY }
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_parse() {
        let _ = machine_monitor("mc-mini", "15m");
        let _ = machine_monitor("mc-old", "1h");
        let n = now();
        let u = team_usage(n - 3 * DAY, n + 4 * DAY);
        assert_eq!(u.daily.len(), 7);
        assert_eq!(team_roster().invites.len(), 2);
        assert!(!team_activity().entries.is_empty());
        assert_eq!(machine_secrets().secrets.len(), 2);
    }
}
