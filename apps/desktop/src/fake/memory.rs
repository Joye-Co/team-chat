//! 沉淀 of [`FakeProvider`], in memory: 团队约定 and every AI member's 记得的事 with a
//! believable day of 记忆更新, so the screens and their scenes show real content. Writes
//! and 撤回 follow the server's rules (docs/protocol.md, "Memory").

use std::{
    collections::HashMap,
    sync::{Mutex, atomic::AtomicU8, atomic::Ordering},
};

use super::{FakeProvider, ME};
use crate::data::{
    AdminError, AdminReply, Millis,
    memory::{
        Memory, MemoryChange, MemoryDay, MemoryEdit, MemoryKind, MemoryScope, MemorySourceKind,
        MemoryStatus, MemoryUpdate, NewMemory,
    },
    reply_now,
};

/// What the fake answers, for the state scenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryShape {
    /// The samples.
    Normal,
    /// Nothing kept yet, no 记忆更新.
    Empty,
    /// Requests never answer (the skeletons).
    Loading,
    /// Requests fail as offline (the Alert with 重试).
    Failed,
    /// The samples at their longest (break-ui): 500-character texts, long notes, many lines.
    Long,
}

impl MemoryShape {
    fn from_u8(n: u8) -> Self {
        match n {
            1 => MemoryShape::Empty,
            2 => MemoryShape::Loading,
            3 => MemoryShape::Failed,
            4 => MemoryShape::Long,
            _ => MemoryShape::Normal,
        }
    }
    fn to_u8(self) -> u8 {
        match self {
            MemoryShape::Normal => 0,
            MemoryShape::Empty => 1,
            MemoryShape::Loading => 2,
            MemoryShape::Failed => 3,
            MemoryShape::Long => 4,
        }
    }
}

/// Who uses the server in a memory scene, and whether it has an AI 服务.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryTeam {
    /// Only me, with the AI members (盯 4.12: 我的约定).
    pub solo: bool,
    /// The server has an AI 服务 (盯 4.6).
    pub ai: bool,
}

impl MemoryTeam {
    pub const TEAM: MemoryTeam = MemoryTeam {
        solo: false,
        ai: true,
    };
    pub const SOLO: MemoryTeam = MemoryTeam {
        solo: true,
        ai: true,
    };
    pub const NO_AI: MemoryTeam = MemoryTeam {
        solo: false,
        ai: false,
    };
}

#[derive(Default)]
struct Scope {
    memories: Vec<Memory>,
    day: MemoryDay,
}

#[derive(Default)]
pub struct FakeMemory {
    shape: AtomicU8,
    scopes: Mutex<HashMap<String, Scope>>,
    next: Mutex<u64>,
}

const MIN: Millis = 60_000;
const HOUR: Millis = 60 * MIN;
const DAY: Millis = 24 * HOUR;

/// Today's local date and today's 03:05 (the consolidation), from the app clock.
fn today() -> (String, Millis) {
    use chrono::Timelike as _;
    let now = crate::selftest::clock::local_now();
    let day = now.format("%Y-%m-%d").to_string();
    let since_midnight = now.num_seconds_from_midnight() as Millis * 1000;
    let at = now.timestamp_millis() - since_midnight + 3 * HOUR + 5 * MIN;
    (day, at)
}

struct Sample {
    id: &'static str,
    kind: MemoryKind,
    text: &'static str,
    when: Option<&'static str>,
    source: MemorySourceKind,
    note: Option<&'static str>,
    human: bool,
    status: MemoryStatus,
    /// Days before today.
    age: Millis,
}

fn s(
    id: &'static str,
    kind: MemoryKind,
    text: &'static str,
    when: Option<&'static str>,
    source: MemorySourceKind,
) -> Sample {
    Sample {
        id,
        kind,
        text,
        when,
        source,
        note: None,
        human: source == MemorySourceKind::Manual,
        status: MemoryStatus::Active,
        age: 3,
    }
}

impl Sample {
    fn note(mut self, n: &'static str) -> Self {
        self.note = Some(n);
        self
    }
    fn human(mut self) -> Self {
        self.human = true;
        self
    }
    fn status(mut self, st: MemoryStatus) -> Self {
        self.status = st;
        self
    }
    fn age(mut self, days: Millis) -> Self {
        self.age = days;
        self
    }
}

fn member_samples() -> Vec<Sample> {
    use MemoryKind::*;
    use MemorySourceKind::*;
    vec![
        s(
            "lesson-stress",
            Lesson,
            "合并前先在 staging 跑一万条消息的压测，过了再开 PR。",
            Some("改了消息同步或重连的代码"),
            Run,
        )
        .note("11 月 1 日和 2 日两次回滚，都是没跑压测就合并了")
        .age(0),
        s(
            "lesson-build",
            Lesson,
            "Swift 构建报找不到模块时，先清掉 DerivedData 再构建一次，别急着改代码。",
            Some("Mac 上构建失败"),
            Run,
        )
        .age(0),
        s(
            "lesson-shots",
            Lesson,
            "界面改动的 PR 描述里放浅色和深色两张截图，Joye 审的时候先看图。",
            Some("开界面相关的 PR"),
            Message,
        )
        .human()
        .age(6),
        s(
            "lesson-d1",
            Lesson,
            "接口超时先查 D1 的慢查询，再看 Worker 日志。",
            Some("staging 巡检报超时"),
            Run,
        )
        .status(MemoryStatus::Pending)
        .age(0),
        s(
            "playbook-release",
            Playbook,
            "发版：改版本号 → 写更新说明 → 打 tag → 等 CI 出安装包 → 先在 staging 装一遍，再发正式。",
            Some("要发新版本"),
            Manual,
        )
        .age(12),
        s(
            "playbook-reconnect",
            Playbook,
            "重现重连问题：两台 Mac 登同一个账号，一台断网 30 秒再连上，看消息有没有重复或丢失。",
            None,
            Run,
        )
        .age(4),
        s(
            "pref-short",
            Preference,
            "交付说明写三行以内：做了什么、怎么验的、还差什么。",
            None,
            Message,
        )
        .human()
        .age(9),
        s(
            "lesson-429",
            Lesson,
            "日志里看到 429 就把并发降到 1。",
            None,
            Run,
        )
        .status(MemoryStatus::Retired)
        .note("撤回了：429 是测试账号限流，和并发无关")
        .age(0),
        s(
            "lesson-1000",
            Lesson,
            "压测用 1000 条消息就够了。",
            None,
            Run,
        )
        .status(MemoryStatus::Retired)
        .note("被「合并前先在 staging 跑一万条消息的压测」取代")
        .age(0),
    ]
}

fn team_samples() -> Vec<Sample> {
    use MemoryKind::*;
    use MemorySourceKind::*;
    vec![
        s(
            "team-pr-title",
            Convention,
            "PR 标题用英文，描述用中文。",
            None,
            Manual,
        )
        .age(20),
        s(
            "team-staging",
            Convention,
            "先发 staging，巡检 30 分钟没有报警再发生产。",
            Some("上线 orderbot 或拾光"),
            Card,
        )
        .age(0),
        s(
            "team-screens",
            Convention,
            "改界面要附浅色和深色两张截图。",
            Some("开界面相关的 PR"),
            MemorySourceKind::Other,
        )
        .note("Forge 和 Tata 的经验里都有这一条，升为团队约定")
        .age(0),
        s(
            "team-friday",
            Convention,
            "周五下午不发生产。",
            None,
            Manual,
        )
        .age(15),
        s(
            "team-patrol",
            Fact,
            "staging 每天 08:00 自动巡检，结果发到运维群。",
            None,
            Message,
        )
        .age(8),
        s(
            "team-owner",
            Fact,
            "拾光的数据看板归 Ledger 管，改指标先问它。",
            None,
            Message,
        )
        .age(5),
        s(
            "team-two-reviews",
            Convention,
            "合并到 main 要两个人审。",
            None,
            Manual,
        )
        .status(MemoryStatus::Retired)
        .note("改成一个人审就行：Joye 11 月 5 日在开发群说的")
        .age(0),
    ]
}

/// 群里的约定 of every 群: what its members said in it.
fn group_samples() -> Vec<Sample> {
    use MemoryKind::*;
    use MemorySourceKind::*;
    vec![
        s(
            "group-zh",
            Convention,
            "这个群的产出都用中文写，代码注释除外。",
            None,
            Manual,
        )
        .age(14),
        s(
            "group-shots",
            Convention,
            "合并前在群里贴浅色和深色两张截图，Joye 看过再合。",
            Some("界面相关的改动"),
            Message,
        )
        .note("Joye 11 月 10 日在群里说的")
        .age(0),
        s(
            "group-staging",
            Fact,
            "staging 的地址和账号在群公告里，别写进代码或产出。",
            None,
            Message,
        )
        .age(6),
        s(
            "group-standup",
            Convention,
            "每天早上 10 点前在群里说一句今天做什么。",
            None,
            Message,
        )
        .status(MemoryStatus::Pending)
        .age(0),
        s(
            "group-weekly",
            Convention,
            "每周一早上开一次同步会。",
            None,
            Message,
        )
        .status(MemoryStatus::Retired)
        .note("撤回了：改成在话题里随时说")
        .age(0),
    ]
}

/// 我的偏好: what I said I like.
fn person_samples() -> Vec<Sample> {
    use MemoryKind::*;
    use MemorySourceKind::*;
    vec![
        s(
            "pref-short",
            Preference,
            "回复写短一点，先说结论，再说怎么做的。",
            None,
            Manual,
        )
        .age(10),
        s(
            "pref-pr",
            Preference,
            "PR 标题用英文，描述用中文。",
            Some("开 PR"),
            Message,
        )
        .note("你在开发群里说过两次")
        .age(0),
        s(
            "pref-quiet",
            Fact,
            "晚上十点以后别发提醒，第二天早上再说。",
            None,
            Message,
        )
        .age(4),
        s(
            "pref-shots",
            Preference,
            "交付时附浅色和深色两张截图。",
            Some("界面相关的产出"),
            Message,
        )
        .status(MemoryStatus::Pending)
        .age(0),
    ]
}

fn group_day(ms: &[Memory]) -> MemoryDay {
    let (day, at) = today();
    let mut updates = Vec::new();
    if let Some(m) = find(ms, "/group-shots") {
        updates.push(line(
            "u-group-shots",
            &day,
            at,
            MemoryChange::Added,
            None,
            Some(m),
            m.note.as_deref(),
        ));
    }
    if let Some(m) = find(ms, "/group-weekly") {
        let mut before = m.clone();
        before.status = MemoryStatus::Active;
        updates.push(line(
            "u-group-weekly",
            &day,
            at,
            MemoryChange::Retired,
            Some(&before),
            Some(m),
            m.note.as_deref(),
        ));
    }
    MemoryDay {
        day: Some(day),
        updates,
    }
}

fn person_day(ms: &[Memory]) -> MemoryDay {
    let (day, at) = today();
    let mut updates = Vec::new();
    if let Some(m) = find(ms, "/pref-pr") {
        updates.push(line(
            "u-pref-pr",
            &day,
            at - MIN,
            MemoryChange::Added,
            None,
            Some(m),
            m.note.as_deref(),
        ));
    }
    MemoryDay {
        day: Some(day),
        updates,
    }
}

const LONG_TEXT: &str = "合并前先在 staging 跑一万条消息的压测，覆盖断线重连、两台设备同时在线、休眠唤醒和切换网络这四种情况；每种情况至少跑三轮，记下每轮的丢失数、重复数和 p95 送达时间，和上一个版本的数字放在一起比较；任何一项比上一个版本差就先别开 PR，回去找原因，找到以后在 PR 描述里写清楚是哪一次提交引入的、为什么、怎么修的。压测脚本在仓库的 scripts 目录里，跑之前先确认 staging 上没有别人在跑同一个脚本，不然两边的数字会互相干扰；跑完把结果贴进话题，并且附上 Grafana 的截图链接。如果压测本身挂了（比如 staging 重启），不要把挂掉当成通过，等它恢复以后整轮重跑。这一条是从十一月一日和二日两次回滚里得出来的：两次都是改了重连的代码，单元测试都过了，但是没有跑压测，上线后半小时内出现了消息重复，最后只能整体回滚。以后凡是动到消息同步、重连、已读位置或者未读计数的代码，都按这一条来做，没有例外，也不要因为改动看起来很小就跳过。";
const LONG_WHEN: &str = "改了消息同步、断线重连、已读位置、未读计数或者推送的任何代码，包括看起来只是重命名或者挪了位置的改动，以及升级了和网络相关的依赖库";
const LONG_NOTE: &str = "十一月一日和二日两次回滚都是没跑压测就合并了；十一月四日 Tata 在拾光上也遇到一样的问题，复盘里写了同样的结论；十一月六日 Joye 在开发群里说以后合并前一律先压测";

fn build(samples: Vec<Sample>, scope: &str, author: &str, long: bool) -> Vec<Memory> {
    let (_, at) = today();
    let mut out: Vec<Memory> = samples
        .into_iter()
        .map(|x| {
            let created = at - x.age * DAY - 2 * DAY;
            let updated = if x.age == 0 { at } else { at - x.age * DAY };
            Memory {
                id: format!("{scope}/{}", x.id),
                scope: scope.into(),
                kind: x.kind,
                text: if long {
                    LONG_TEXT.into()
                } else {
                    x.text.into()
                },
                when: if long {
                    Some(LONG_WHEN.into())
                } else {
                    x.when.map(str::to_string)
                },
                source: x.source,
                source_ids: vec!["01HSAMPLE".into()],
                note: if long {
                    Some(LONG_NOTE.into())
                } else {
                    x.note.map(str::to_string)
                },
                author_id: if x.human { ME.into() } else { author.into() },
                human: x.human,
                status: x.status,
                uses: 3,
                last_used_at: Some(at),
                version: if x.age == 0 { 2 } else { 1 },
                created_at: created,
                updated_at: updated,
            }
        })
        .collect();
    if long {
        // Many of one kind: the list a busy member reaches after a few weeks.
        let base = out[0].clone();
        for i in 0..8 {
            let mut m = base.clone();
            m.id = format!("{}-{i}", base.id);
            m.updated_at -= (i + 1) * DAY;
            out.push(m);
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    out
}

fn line(
    id: &str,
    day: &str,
    at: Millis,
    change: MemoryChange,
    before: Option<&Memory>,
    after: Option<&Memory>,
    note: Option<&str>,
) -> MemoryUpdate {
    MemoryUpdate {
        id: id.into(),
        day: day.into(),
        memory_id: after.or(before).map(|m| m.id.clone()).unwrap_or_default(),
        change,
        before: before.cloned(),
        after: after.cloned(),
        note: note.map(str::to_string),
        undone_at: None,
        at,
    }
}

fn find<'a>(ms: &'a [Memory], suffix: &str) -> Option<&'a Memory> {
    ms.iter().find(|m| m.id.ends_with(suffix))
}

fn member_day(ms: &[Memory]) -> MemoryDay {
    let (day, at) = today();
    let mut updates = Vec::new();
    if let Some(m) = find(ms, "/lesson-stress") {
        updates.push(line(
            "u-stress",
            &day,
            at,
            MemoryChange::Added,
            None,
            Some(m),
            Some("从两次复盘里整理出来"),
        ));
    }
    if let Some(m) = find(ms, "/lesson-build") {
        let mut before = m.clone();
        before.text = "构建失败就重试一次。".into();
        before.version = 1;
        updates.push(line(
            "u-build",
            &day,
            at,
            MemoryChange::Changed,
            Some(&before),
            Some(m),
            None,
        ));
    }
    if let Some(m) = find(ms, "/lesson-1000") {
        let mut before = m.clone();
        before.status = MemoryStatus::Active;
        updates.push(line(
            "u-1000",
            &day,
            at,
            MemoryChange::Retired,
            Some(&before),
            Some(m),
            m.note.as_deref(),
        ));
    }
    if let Some(m) = find(ms, "/playbook-release") {
        updates.push(line(
            "u-release",
            &day,
            at,
            MemoryChange::Flagged,
            Some(m),
            Some(m),
            Some("发版的做法里少了「更新下载页」这一步，可能过时了"),
        ));
    }
    if let Some(m) = find(ms, "/lesson-429") {
        let mut l = line("u-429", &day, at, MemoryChange::Added, None, Some(m), None);
        l.undone_at = Some(at + 5 * HOUR);
        updates.push(l);
    }
    MemoryDay {
        day: Some(day),
        updates,
    }
}

fn team_day(ms: &[Memory]) -> MemoryDay {
    let (day, at) = today();
    let mut updates = Vec::new();
    if let Some(m) = find(ms, "/team-screens") {
        updates.push(line(
            "u-team-screens",
            &day,
            at,
            MemoryChange::Added,
            None,
            Some(m),
            m.note.as_deref(),
        ));
    }
    if let Some(m) = find(ms, "/team-staging") {
        let mut before = m.clone();
        before.text = "先发 staging，再发生产。".into();
        updates.push(line(
            "u-team-staging",
            &day,
            at,
            MemoryChange::Changed,
            Some(&before),
            Some(m),
            None,
        ));
    }
    if let Some(m) = find(ms, "/team-two-reviews") {
        let mut before = m.clone();
        before.status = MemoryStatus::Active;
        updates.push(line(
            "u-team-two",
            &day,
            at,
            MemoryChange::Retired,
            Some(&before),
            Some(m),
            m.note.as_deref(),
        ));
    }
    MemoryDay {
        day: Some(day),
        updates,
    }
}

fn long_day(ms: &[Memory]) -> MemoryDay {
    let (day, at) = today();
    let mut updates = Vec::new();
    for (i, m) in ms.iter().take(6).enumerate() {
        let change = match i % 3 {
            0 => MemoryChange::Added,
            1 => MemoryChange::Changed,
            _ => MemoryChange::Retired,
        };
        updates.push(line(
            &format!("u-long-{i}"),
            &day,
            at,
            change,
            Some(m),
            Some(m),
            Some(LONG_NOTE),
        ));
    }
    MemoryDay {
        day: Some(day),
        updates,
    }
}

impl FakeMemory {
    pub fn set_shape(&self, shape: MemoryShape) {
        self.shape.store(shape.to_u8(), Ordering::Relaxed);
        self.scopes.lock().unwrap().clear();
    }

    fn shape(&self) -> MemoryShape {
        MemoryShape::from_u8(self.shape.load(Ordering::Relaxed))
    }

    /// `Some(reply)` when the shape answers instead of the store.
    fn shaped<T: Send + 'static>(&self) -> Option<AdminReply<T>> {
        match self.shape() {
            MemoryShape::Loading => {
                let (tx, rx) = futures::channel::oneshot::channel();
                // Never answers: the sender stays alive for the life of the process.
                std::mem::forget(tx);
                Some(rx)
            }
            MemoryShape::Failed => Some(reply_now(Err(AdminError::Offline))),
            _ => None,
        }
    }

    fn scope<R>(&self, scope: &str, f: impl FnOnce(&mut Scope) -> R) -> R {
        let shape = self.shape();
        let mut scopes = self.scopes.lock().unwrap();
        let entry = scopes.entry(scope.to_string()).or_insert_with(|| {
            if shape == MemoryShape::Empty {
                return Scope::default();
            }
            let long = shape == MemoryShape::Long;
            if scope == "team" {
                let memories = build(team_samples(), scope, "b-jojo", long);
                let day = if long {
                    long_day(&memories)
                } else {
                    team_day(&memories)
                };
                Scope { memories, day }
            } else if let Some(member) = scope.strip_prefix("member:") {
                let memories = build(member_samples(), scope, member, long);
                let day = if long {
                    long_day(&memories)
                } else {
                    member_day(&memories)
                };
                Scope { memories, day }
            } else if scope.starts_with("group:") || scope.starts_with("person:") {
                let group = scope.starts_with("group:");
                let samples = if group {
                    group_samples()
                } else {
                    person_samples()
                };
                let memories = build(samples, scope, "b-forge", long);
                let day = if long {
                    long_day(&memories)
                } else if group {
                    group_day(&memories)
                } else {
                    person_day(&memories)
                };
                Scope { memories, day }
            } else {
                Scope::default()
            }
        });
        f(entry)
    }

    fn scope_of_memory(&self, id: &str) -> Option<String> {
        id.split_once('/').map(|(scope, _)| scope.to_string())
    }
}

impl FakeProvider {
    pub(super) fn memories_fake(&self, scope: &MemoryScope) -> AdminReply<Vec<Memory>> {
        if let Some(r) = self.memory.shaped() {
            return r;
        }
        reply_now(Ok(self.memory.scope(&scope.wire(), |s| s.memories.clone())))
    }

    pub(super) fn memory_updates_fake(&self, scope: &MemoryScope) -> AdminReply<MemoryDay> {
        if let Some(r) = self.memory.shaped() {
            return r;
        }
        reply_now(Ok(self.memory.scope(&scope.wire(), |s| s.day.clone())))
    }

    pub(super) fn create_memory_fake(&self, new: NewMemory) -> AdminReply<Memory> {
        if let Some(r) = self.memory.shaped() {
            return r;
        }
        let text = new.text.trim().to_string();
        if text.is_empty() || new.kind == MemoryKind::Other {
            return reply_now(Err(AdminError::Invalid));
        }
        let scope = new.scope.wire();
        let n = {
            let mut next = self.memory.next.lock().unwrap();
            *next += 1;
            *next
        };
        let now = crate::selftest::clock::now_ms();
        let m = Memory {
            id: format!("{scope}/new-{n}"),
            scope: scope.clone(),
            kind: new.kind,
            text,
            when: new
                .when
                .map(|w| w.trim().to_string())
                .filter(|w| !w.is_empty()),
            source: MemorySourceKind::Manual,
            source_ids: Vec::new(),
            note: None,
            author_id: ME.into(),
            human: true,
            status: MemoryStatus::Active,
            uses: 0,
            last_used_at: None,
            version: 1,
            created_at: now,
            updated_at: now,
        };
        self.memory
            .scope(&scope, |s| s.memories.insert(0, m.clone()));
        reply_now(Ok(m))
    }

    pub(super) fn update_memory_fake(&self, id: &str, edit: MemoryEdit) -> AdminReply<Memory> {
        if let Some(r) = self.memory.shaped() {
            return r;
        }
        let Some(scope) = self.memory.scope_of_memory(id) else {
            return reply_now(Err(AdminError::NotFound));
        };
        let now = crate::selftest::clock::now_ms();
        let result = self.memory.scope(&scope, |s| {
            let m = s
                .memories
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or(AdminError::NotFound)?;
            let mut changed = false;
            if let Some(t) = edit.text.as_deref().map(str::trim) {
                if t.is_empty() {
                    return Err(AdminError::Invalid);
                }
                if t != m.text {
                    m.text = t.to_string();
                    m.human = true;
                    changed = true;
                }
            }
            if let Some(k) = edit.kind.filter(|k| *k != m.kind) {
                m.kind = k;
                m.human = true;
                changed = true;
            }
            if let Some(w) = &edit.when {
                let w = w
                    .as_deref()
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_string);
                if w != m.when {
                    m.when = w;
                    m.human = true;
                    changed = true;
                }
            }
            if let Some(st) = edit.status.filter(|st| *st != m.status) {
                m.status = st;
                changed = true;
            }
            if changed {
                m.version += 1;
                m.updated_at = now;
                m.author_id = ME.into();
            }
            Ok(m.clone())
        });
        if result.is_ok() {
            self.memory.scope(&scope, |s| {
                s.memories.sort_by_key(|m| std::cmp::Reverse(m.updated_at))
            });
        }
        reply_now(result)
    }

    pub(super) fn undo_memory_update_fake(&self, id: &str) -> AdminReply<MemoryUpdate> {
        if let Some(r) = self.memory.shaped() {
            return r;
        }
        let now = crate::selftest::clock::now_ms();
        let scopes: Vec<String> = self.memory.scopes.lock().unwrap().keys().cloned().collect();
        for scope in scopes {
            let result = self.memory.scope(&scope, |s| {
                let u = s.day.updates.iter_mut().find(|u| u.id == id)?;
                if u.change == MemoryChange::Flagged || u.change == MemoryChange::Other {
                    return Some(Err(AdminError::Invalid));
                }
                if u.undone_at.is_some() {
                    return Some(Err(AdminError::Conflict));
                }
                u.undone_at = Some(now);
                let u = u.clone();
                if let Some(m) = s.memories.iter_mut().find(|m| m.id == u.memory_id) {
                    match u.change {
                        MemoryChange::Added => m.status = MemoryStatus::Retired,
                        MemoryChange::Retired => m.status = MemoryStatus::Active,
                        MemoryChange::Changed => {
                            if let Some(b) = &u.before {
                                m.kind = b.kind;
                                m.text = b.text.clone();
                                m.when = b.when.clone();
                                m.status = b.status;
                            }
                        }
                        _ => {}
                    }
                    m.version += 1;
                    m.updated_at = now;
                }
                Some(Ok(u))
            });
            if let Some(r) = result {
                return reply_now(r);
            }
        }
        reply_now(Err(AdminError::NotFound))
    }

    /// Sets what the fake's memory requests answer (state scenes).
    pub fn set_memory_shape(&self, shape: MemoryShape) {
        self.memory.set_shape(shape);
    }
}
