//! What the 概览 page shows, worked out from one `MachineMonitor` without drawing anything:
//! the words of every number, the 要注意 list and the sorted 按成员 rows. Kept apart from the
//! view so it is tested without a window.

use chat_core::proto::machine::login_state;
use chat_core::proto::monitor::{MachineMonitor, MonitorMember, run_state};

use crate::copy::{admin as m, fill};

/// The ranges of the segmented control, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Range {
    M15,
    #[default]
    H1,
    H24,
}

impl Range {
    pub const ALL: [Range; 3] = [Range::M15, Range::H1, Range::H24];

    pub fn wire(self) -> &'static str {
        match self {
            Range::M15 => "15m",
            Range::H1 => "1h",
            Range::H24 => "24h",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Range::M15 => m::RANGE_15M,
            Range::H1 => m::RANGE_1H,
            Range::H24 => m::RANGE_24H,
        }
    }

    pub fn index(self) -> usize {
        Range::ALL.iter().position(|r| *r == self).unwrap_or(1)
    }
}

const KIB: f64 = 1024.;
const MIB: f64 = KIB * 1024.;
const GIB: f64 = MIB * 1024.;
const TIB: f64 = GIB * 1024.;

/// "62.6 GiB", "835 GiB", "1.2 TiB": one decimal below 100.
pub fn bytes(n: u64) -> String {
    let n = n as f64;
    let (v, unit) = if n >= TIB {
        (n / TIB, "TiB")
    } else if n >= GIB {
        (n / GIB, "GiB")
    } else if n >= MIB {
        (n / MIB, "MiB")
    } else if n >= KIB {
        (n / KIB, "KiB")
    } else {
        (n, "B")
    };
    format!("{} {unit}", one_decimal(v))
}

/// A rate: "3.4 MiB/s".
pub fn rate(bps: f64) -> String {
    format!("{}/s", bytes(bps.max(0.).round() as u64))
}

/// The number and unit of a rate apart, for the KPI's big number: ("3.4", "MiB/s").
pub fn rate_parts(bps: f64) -> (String, String) {
    let s = rate(bps);
    match s.split_once(' ') {
        Some((v, u)) => (v.to_string(), u.to_string()),
        None => (s, String::new()),
    }
}

fn one_decimal(v: f64) -> String {
    if v >= 100. || (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as u64)
    } else {
        format!("{v:.1}")
    }
}

/// "6.8M", "14K", "950": token counts.
pub fn count_short(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{}M", one_decimal(n as f64 / 1_000_000.))
    } else if n >= 10_000 {
        format!("{}K", (n as f64 / 1_000.).round() as u64)
    } else if n >= 1_000 {
        format!("{}K", one_decimal(n as f64 / 1_000.))
    } else {
        n.to_string()
    }
}

/// "2.6 核".
pub fn cores(c: f64) -> String {
    fill(m::CORES, &[("n", &format!("{:.1}", c.max(0.)))])
}

/// A wait or an age: "2 分 10 秒", "45 秒", "3 小时", "2 天".
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        fill(m::SECONDS, &[("n", &s.max(1).to_string())])
    } else if s < 600 {
        fill(
            m::MINUTES_SECONDS,
            &[("m", &(s / 60).to_string()), ("s", &(s % 60).to_string())],
        )
    } else if s < 3600 {
        fill(m::MINUTES, &[("n", &(s / 60).to_string())])
    } else if s < 86_400 {
        fill(m::HOURS, &[("n", &(s / 3600).to_string())])
    } else {
        fill(m::DAYS, &[("n", &(s / 86_400).to_string())])
    }
}

/// "3 分钟前", or 刚刚 under 10 seconds.
pub fn ago(ms: u64) -> String {
    if ms < 10_000 {
        return m::JUST_NOW.to_string();
    }
    fill(m::AGO, &[("d", &duration(ms))])
}

/// A boot time: "1.3 秒".
pub fn seconds(ms: u64) -> String {
    fill(m::SECONDS, &[("n", &one_decimal(ms as f64 / 1000.))])
}

/// The share of `part` in `whole`, 0..=1.
pub fn share(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.
    } else {
        (part as f64 / whole as f64).clamp(0., 1.) as f32
    }
}

/// A runtime's name as people know it: "Claude Code", "Codex", else the configured name.
pub fn cli_name(kind: &str, runtime: &str) -> String {
    match kind {
        "claude" => "Claude Code".into(),
        "codex" => "Codex".into(),
        _ => runtime.to_string(),
    }
}

/// A runtime entry as the topic's sign-in note names it: the CLI's name, and the entry's own
/// name when it is not the kind's (`Codex（codexout）`), so two entries of one CLI differ.
pub fn cli_entry_name(kind: &str, runtime: &str) -> String {
    let cli = cli_name(kind, runtime);
    if matches!(kind, "claude" | "codex") && runtime != kind {
        fill(m::CLI_ENTRY, &[("cli", &cli), ("entry", runtime)])
    } else {
        cli
    }
}

/// The tone of a dot or an icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Quiet,
    /// Something to know, not to fix (an invite about to end).
    Info,
}

/// One cell of the status strip.
#[derive(Clone, Debug, PartialEq)]
pub struct StripCell {
    pub label: String,
    pub value: String,
    pub tone: Tone,
    pub cli: bool,
}

/// The status strip: the host, then each CLI with its login.
pub fn strip(d: &MachineMonitor, now: u64) -> Vec<StripCell> {
    let mut out = Vec::new();
    let host = if d.machine.connected {
        let since = d.machine.connected_at.unwrap_or(now);
        StripCell {
            label: m::STRIP_HOST.into(),
            value: fill(
                m::HOST_UP,
                &[("duration", &duration(now.saturating_sub(since)))],
            ),
            tone: Tone::Ok,
            cli: false,
        }
    } else {
        StripCell {
            label: m::STRIP_HOST.into(),
            value: match d.machine.last_heartbeat_at {
                Some(t) => fill(m::HOST_DOWN, &[("ago", &ago(now.saturating_sub(t)))]),
                None => m::HOST_NEVER.into(),
            },
            tone: if d.machine.last_heartbeat_at.is_some() {
                Tone::Bad
            } else {
                Tone::Quiet
            },
            cli: false,
        }
    };
    out.push(host);
    for c in &d.clis {
        let (value, tone) = if !c.available {
            (m::CLI_MISSING, Tone::Warn)
        } else {
            match c.login.as_str() {
                login_state::LOGGED_IN => (m::LOGIN_IN, Tone::Ok),
                login_state::LOGGED_OUT => (m::LOGIN_OUT, Tone::Warn),
                _ => (m::LOGIN_UNKNOWN, Tone::Quiet),
            }
        };
        out.push(StripCell {
            label: cli_name(&c.kind, &c.runtime),
            value: value.into(),
            tone,
            cli: true,
        });
    }
    out
}

/// What a 要注意 row is about (it picks the icon).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionKind {
    Waiting,
    CliLoggedOut,
    Queue,
    Failed,
    Disk,
    Offline,
    /// 总览: an AI member near or over its weekly budget.
    Budget,
    /// 总览: an invite about to end.
    Invite,
    /// 模 3.3: a CLI's subscription near or at its limit (the machine's sign-in, shared).
    Limit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attention {
    pub kind: AttentionKind,
    pub title: String,
    pub meta: String,
    pub tone: Tone,
}

/// A subscription window this full is near its limit (模 3.3).
pub const NEAR_LIMIT_PCT: f64 = 90.0;

/// 模 3.3: one 要注意 row for a signed-in CLI whose fullest usage window is near or at its limit;
/// nothing is held back by it (a run that hits the limit waits for the reset by itself).
fn near_limit(
    c: &chat_core::proto::monitor::MonitorCli,
    time: &dyn Fn(u64) -> String,
) -> Option<Attention> {
    if !c.available || c.login == login_state::LOGGED_OUT {
        return None;
    }
    let w = c
        .usage
        .as_deref()?
        .iter()
        .filter(|w| w.used_pct.is_finite())
        .max_by(|a, b| a.used_pct.total_cmp(&b.used_pct))?;
    if w.used_pct < NEAR_LIMIT_PCT {
        return None;
    }
    let cli = cli_name(&c.kind, &c.runtime);
    let full = w.used_pct >= 100.0;
    let title = if full {
        fill(m::ATT_LIMIT_FULL, &[("cli", &cli)])
    } else {
        let pct = (w.used_pct.floor() as u32).to_string();
        fill(m::ATT_LIMIT_NEAR, &[("cli", &cli), ("pct", &pct)])
    };
    let meta = match w.resets_at {
        Some(at) => fill(m::ATT_LIMIT_META, &[("time", &time(at))]),
        None => m::ATT_LIMIT_META_NONE.into(),
    };
    Some(Attention {
        kind: AttentionKind::Limit,
        title,
        meta,
        tone: if full { Tone::Bad } else { Tone::Warn },
    })
}

/// The 要注意 list, the most urgent first. `name` gives a member's name.
pub fn attention(
    d: &MachineMonitor,
    now: u64,
    name: &dyn Fn(&str) -> String,
    time: &dyn Fn(u64) -> String,
) -> Vec<Attention> {
    let mut out = Vec::new();
    if !d.machine.connected
        && let Some(t) = d.machine.last_heartbeat_at
    {
        out.push(Attention {
            kind: AttentionKind::Offline,
            title: m::ATT_OFFLINE.into(),
            meta: fill(m::ATT_OFFLINE_META, &[("ago", &ago(now.saturating_sub(t)))]),
            tone: Tone::Bad,
        });
    }
    let waiting: Vec<&MonitorMember> = d.members.iter().filter(|x| x.waiting > 0).collect();
    let n: u32 = waiting.iter().map(|x| x.waiting).sum();
    if n > 0 {
        let who: Vec<String> = waiting
            .iter()
            .map(|x| {
                fill(
                    m::ATT_COUNT,
                    &[("name", &name(&x.member_id)), ("n", &x.waiting.to_string())],
                )
            })
            .collect();
        out.push(Attention {
            kind: AttentionKind::Waiting,
            title: fill(m::ATT_WAITING, &[("n", &n.to_string())]),
            meta: who.join(" · "),
            tone: Tone::Warn,
        });
    }
    for c in &d.clis {
        if c.available && c.login == login_state::LOGGED_OUT {
            out.push(Attention {
                kind: AttentionKind::CliLoggedOut,
                title: fill(
                    m::ATT_CLI_OUT,
                    &[("cli", &cli_entry_name(&c.kind, &c.runtime))],
                ),
                meta: if c.failed_today > 0 {
                    fill(m::ATT_CLI_OUT_FAILED, &[("n", &c.failed_today.to_string())])
                } else {
                    m::ATT_CLI_OUT_HINT.into()
                },
                tone: Tone::Warn,
            });
        }
    }
    for c in &d.clis {
        if let Some(a) = near_limit(c, time) {
            out.push(a);
        }
    }
    if d.queue.count > 0 {
        out.push(Attention {
            kind: AttentionKind::Queue,
            title: fill(m::ATT_QUEUE, &[("n", &d.queue.count.to_string())]),
            meta: d
                .queue
                .oldest_at
                .map(|t| {
                    fill(
                        m::ATT_QUEUE_OLDEST,
                        &[("wait", &duration(now.saturating_sub(t)))],
                    )
                })
                .unwrap_or_default(),
            tone: Tone::Warn,
        });
    }
    if let Some(f) = &d.last_failure {
        let when = time(f.at);
        out.push(Attention {
            kind: AttentionKind::Failed,
            title: fill(m::ATT_FAILED, &[("name", &name(&f.member_id))]),
            meta: match f.title.as_deref().filter(|t| !t.trim().is_empty()) {
                Some(t) => fill(m::ATT_FAILED_META, &[("title", t), ("time", &when)]),
                None => when,
            },
            tone: Tone::Bad,
        });
    }
    if let Some(h) = &d.host
        && h.disk_total_bytes > 0
        && share(h.disk_free_bytes, h.disk_total_bytes) < 0.1
    {
        out.push(Attention {
            kind: AttentionKind::Disk,
            title: fill(m::ATT_DISK, &[("free", &bytes(h.disk_free_bytes))]),
            meta: m::ATT_DISK_META.into(),
            tone: Tone::Warn,
        });
    }
    out
}

/// A sortable column of 按成员.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Doing,
    Waiting,
    Cpu,
    Memory,
    Tokens,
    Done,
}

impl SortKey {
    pub const ALL: [SortKey; 7] = [
        SortKey::Name,
        SortKey::Doing,
        SortKey::Waiting,
        SortKey::Cpu,
        SortKey::Memory,
        SortKey::Tokens,
        SortKey::Done,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => m::COL_MEMBER,
            SortKey::Doing => m::COL_DOING,
            SortKey::Waiting => m::COL_WAITING,
            SortKey::Cpu => m::COL_CPU,
            SortKey::Memory => m::COL_MEM,
            SortKey::Tokens => m::COL_TOKENS,
            SortKey::Done => m::COL_DONE,
        }
    }

    /// Names sort A to Z first; numbers largest first.
    pub fn default_descending(self) -> bool {
        self != SortKey::Name
    }
}

/// The table's sort: a column and its direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort {
            key: SortKey::Memory,
            descending: true,
        }
    }
}

impl Sort {
    /// A click on a header: the same column flips, another column starts at its default.
    pub fn click(self, key: SortKey) -> Sort {
        if self.key == key {
            Sort {
                key,
                descending: !self.descending,
            }
        } else {
            Sort {
                key,
                descending: key.default_descending(),
            }
        }
    }
}

/// One row of 按成员 with the member's name.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberRow {
    pub member_id: String,
    pub name: String,
    pub doing: u32,
    pub waiting: u32,
    pub cpu_cores: f64,
    pub mem_bytes: u64,
    pub tokens: u64,
    pub done: u32,
}

/// The rows of 按成员 in `sort` order; ties keep the name order.
pub fn member_rows(
    d: &MachineMonitor,
    sort: Sort,
    name: &dyn Fn(&str) -> String,
) -> Vec<MemberRow> {
    let mut rows: Vec<MemberRow> = d
        .members
        .iter()
        .map(|x| MemberRow {
            member_id: x.member_id.clone(),
            name: name(&x.member_id),
            doing: x.running,
            waiting: x.waiting,
            cpu_cores: x.cpu_cores,
            mem_bytes: x.mem_bytes,
            tokens: x.tokens_today,
            done: x.finished_today,
        })
        .collect();
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let key = |r: &MemberRow| -> f64 {
        match sort.key {
            SortKey::Name => 0.,
            SortKey::Doing => r.doing as f64,
            SortKey::Waiting => r.waiting as f64,
            SortKey::Cpu => r.cpu_cores,
            SortKey::Memory => r.mem_bytes as f64,
            SortKey::Tokens => r.tokens as f64,
            SortKey::Done => r.done as f64,
        }
    };
    if sort.key == SortKey::Name {
        if sort.descending {
            rows.reverse();
        }
    } else {
        rows.sort_by(|a, b| {
            let o = key(a).total_cmp(&key(b));
            if sort.descending { o.reverse() } else { o }
        });
    }
    rows
}

/// Runs on the machine now (everything but the queue).
pub fn running_now(d: &MachineMonitor) -> u32 {
    d.runs
        .iter()
        .filter(|r| r.state != run_state::QUEUED)
        .count() as u32
}

/// The 容量 bar: VM, the host's own use and free memory, as shares of the total.
pub fn memory_split(d: &MachineMonitor) -> Option<(f32, f32, f32, u64, u64, u64)> {
    let h = d.host.as_ref()?;
    let total = h.mem_total_bytes?;
    let free = h.mem_available_bytes.unwrap_or(0).min(total);
    let vm = h.mem_vm_bytes.min(total - free);
    let host = total - free - vm;
    Some((
        share(vm, total),
        share(host, total),
        share(free, total),
        vm,
        host,
        free,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_entry_name_tells_two_entries_of_one_cli_apart() {
        assert_eq!(cli_entry_name("codex", "codex"), "Codex");
        assert_eq!(cli_entry_name("codex", "codexout"), "Codex（codexout）");
        assert_eq!(cli_entry_name("claude", "claude"), "Claude Code");
        assert_eq!(cli_entry_name("acp", "gemini"), "gemini");
    }
    use chat_core::proto::monitor::{
        MonitorCli, MonitorFailure, MonitorHost, MonitorMachine, MonitorQueue,
    };

    fn sample() -> MachineMonitor {
        MachineMonitor {
            machine: MonitorMachine {
                id: "m1".into(),
                name: "runner-1".into(),
                connected: true,
                connected_at: Some(1_000),
                ..Default::default()
            },
            host: Some(MonitorHost {
                load1: 7.4,
                mem_total_bytes: Some(64 << 30),
                mem_available_bytes: Some(26 << 30),
                mem_vm_bytes: 30 << 30,
                disk_total_bytes: 1000,
                disk_free_bytes: 50,
                ..Default::default()
            }),
            queue: MonitorQueue {
                count: 4,
                oldest_at: Some(0),
            },
            clis: vec![
                MonitorCli {
                    runtime: "claude".into(),
                    kind: "claude".into(),
                    available: true,
                    version: None,
                    login: "logged_in".into(),
                    failed_today: 0,
                    method: None,
                    email: None,
                    plan: None,
                    usage: None,
                    usage_at: None,
                },
                MonitorCli {
                    runtime: "codex".into(),
                    kind: "codex".into(),
                    available: true,
                    version: None,
                    login: "logged_out".into(),
                    failed_today: 2,
                    method: None,
                    email: None,
                    plan: None,
                    usage: None,
                    usage_at: None,
                },
            ],
            members: vec![
                member("b", 6, 2, 2.6, 9 << 30, 6_800_000, 48),
                member("a", 7, 0, 1.9, 8 << 30, 4_100_000, 62),
                member("c", 4, 1, 0.8, 5 << 30, 1_900_000, 31),
            ],
            last_failure: Some(MonitorFailure {
                run_id: "r".into(),
                member_id: "c".into(),
                at: 5,
                title: Some("对一下十月的账单".into()),
            }),
            ..Default::default()
        }
    }

    fn member(id: &str, r: u32, w: u32, cpu: f64, mem: u64, tok: u64, done: u32) -> MonitorMember {
        MonitorMember {
            member_id: id.into(),
            running: r,
            waiting: w,
            queued: 0,
            cpu_cores: cpu,
            mem_bytes: mem,
            tokens_today: tok,
            finished_today: done,
        }
    }

    fn names(id: &str) -> String {
        match id {
            "a" => "Scout".into(),
            "b" => "Forge".into(),
            "c" => "Ledger".into(),
            _ => id.into(),
        }
    }

    #[test]
    fn numbers_read_like_the_design() {
        assert_eq!(bytes(67_216_998_400), "62.6 GiB");
        assert_eq!(bytes(896_000_000_000), "834 GiB");
        assert_eq!(bytes(12 << 30), "12 GiB");
        assert_eq!(rate(3_565_158.5), "3.4 MiB/s");
        assert_eq!(rate_parts(1_153_433.), ("1.1".into(), "MiB/s".into()));
        assert_eq!(count_short(6_800_000), "6.8M");
        assert_eq!(count_short(14_200), "14K");
        assert_eq!(count_short(950), "950");
        assert_eq!(duration(130_000), "2 分 10 秒");
        assert_eq!(duration(3 * 3_600_000 + 5), "3 小时");
        assert_eq!(ago(4_000), "刚刚");
        assert_eq!(seconds(1300), "1.3 秒");
        assert_eq!(cores(2.64), "2.6 核");
    }

    #[test]
    fn strip_shows_the_host_and_each_cli_login() {
        let cells = strip(&sample(), 1_000 + 3 * 3_600_000);
        let v: Vec<(&str, &str, Tone)> = cells
            .iter()
            .map(|c| (c.label.as_str(), c.value.as_str(), c.tone))
            .collect();
        assert_eq!(
            v,
            vec![
                ("主机", "在线 3 小时", Tone::Ok),
                ("Claude Code", "已登录", Tone::Ok),
                ("Codex", "未登录", Tone::Warn)
            ]
        );
    }

    #[test]
    fn attention_lists_what_needs_the_owner_most_urgent_first() {
        let list = attention(&sample(), 10_000, &names, &|_| "10:31".into());
        let titles: Vec<&str> = list.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "3 个 VM 在等你",
                "Codex 未登录",
                "4 个任务在排队",
                "Ledger 的任务没跑成",
                "磁盘只剩 50 B"
            ]
        );
        assert_eq!(list[0].meta, "Forge 2 个 · Ledger 1 个");
        assert_eq!(list[1].meta, "今天 2 个任务没跑成");
        assert_eq!(list[3].meta, "对一下十月的账单 · 10:31");
    }

    #[test]
    fn attention_says_a_subscription_is_near_its_limit_once_per_cli() {
        use chat_core::proto::machine::UsageWindow;
        let win = |pct: f64, at: Option<u64>| UsageWindow {
            kind: "session".into(),
            label: None,
            used_pct: pct,
            resets_at: at,
        };
        let mut d = sample();
        d.clis[0].usage = Some(vec![win(40.0, Some(1)), win(92.4, Some(7_000))]);
        // Signed out: the sign-in row says enough.
        d.clis[1].usage = Some(vec![win(100.0, None)]);
        let list = attention(&d, 10_000, &names, &|t| format!("t{t}"));
        let limits: Vec<&Attention> = list
            .iter()
            .filter(|a| a.kind == AttentionKind::Limit)
            .collect();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].title, "Claude Code 的订阅额度用了 92%");
        assert_eq!(limits[0].meta, "t7000 恢复；用完时新任务会等到恢复再做");
        assert_eq!(limits[0].tone, Tone::Warn);
        // Full, with no reset time; below 90 % says nothing.
        d.clis[0].usage = Some(vec![win(100.0, None)]);
        let list = attention(&d, 10_000, &names, &|_| String::new());
        let full = list
            .iter()
            .find(|a| a.kind == AttentionKind::Limit)
            .unwrap();
        assert_eq!(full.title, "Claude Code 的订阅额度用完了");
        assert_eq!(full.meta, "用完时新任务会等到恢复再做");
        assert_eq!(full.tone, Tone::Bad);
        d.clis[0].usage = Some(vec![win(89.9, Some(1))]);
        assert!(
            attention(&d, 10_000, &names, &|_| String::new())
                .iter()
                .all(|a| a.kind != AttentionKind::Limit)
        );
    }

    #[test]
    fn headers_sort_and_flip() {
        let d = sample();
        let s = Sort::default();
        let order = |s: Sort| -> Vec<String> {
            member_rows(&d, s, &names)
                .into_iter()
                .map(|r| r.name)
                .collect()
        };
        assert_eq!(order(s), vec!["Forge", "Scout", "Ledger"]);
        let s = s.click(SortKey::Done);
        assert!(s.descending);
        assert_eq!(order(s), vec!["Scout", "Forge", "Ledger"]);
        let s = s.click(SortKey::Done);
        assert_eq!(order(s), vec!["Ledger", "Forge", "Scout"]);
        let s = s.click(SortKey::Name);
        assert!(!s.descending);
        assert_eq!(order(s), vec!["Forge", "Ledger", "Scout"]);
    }

    #[test]
    fn memory_split_adds_up_to_the_host() {
        let (vm, host, free, vb, hb, fb) = memory_split(&sample()).unwrap();
        assert_eq!((vb, hb, fb), (30 << 30, 8 << 30, 26 << 30));
        assert!((vm + host + free - 1.).abs() < 0.001);
    }
}
