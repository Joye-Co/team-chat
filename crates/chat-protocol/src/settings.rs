//! Settings: notification preferences by event with quiet hours and a daily digest, the team's
//! approval rules for AI members, the team's AI 服务 and my signed-in devices (docs/protocol.md, "Settings").

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::limits::{Validation, ValidationError};
use crate::serde_util::{non_null, nullable};

/// `hello.features` value of a client that handles `digest` and `notifyEvent`.
pub const FEATURE_SETTINGS: &str = "settings/1";

fn invalid(field: &'static str, message: &str) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// True for "HH:MM" on a 24 h clock.
pub fn is_clock_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 5
        || b[2] != b':'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit())
    {
        return false;
    }
    let h = (b[0] - b'0') * 10 + (b[1] - b'0');
    let m = (b[3] - b'0') * 10 + (b[4] - b'0');
    h < 24 && m < 60
}

/// Minutes after midnight of an "HH:MM" value.
pub fn clock_minutes(s: &str) -> Option<u32> {
    is_clock_time(s)
        .then(|| s[..2].parse::<u32>().unwrap_or(0) * 60 + s[3..].parse::<u32>().unwrap_or(0))
}

fn check_clock(field: &'static str, s: &str) -> Validation {
    if is_clock_time(s) {
        Ok(())
    } else {
        invalid(field, "HH:MM")
    }
}

fn check_zone(s: &str) -> Validation {
    let ok = (1..=64).contains(&s.chars().count())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '/'));
    if ok {
        Ok(())
    } else {
        invalid("timeZone", "an IANA time zone name")
    }
}

/// The events a member can switch notifications for. A value this version does not know reads
/// as `Unknown` (show a plain notification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotifyEvent {
    /// 需要我拍板
    Waiting,
    /// 有人 @我
    Mention,
    /// 我交代的事做完了
    Result,
    /// 成员卡住或出错
    Blocked,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEventSwitches {
    pub waiting: bool,
    pub mention: bool,
    pub result: bool,
    pub blocked: bool,
}

impl Default for NotifyEventSwitches {
    fn default() -> Self {
        Self {
            waiting: true,
            mention: true,
            result: true,
            blocked: true,
        }
    }
}

impl NotifyEventSwitches {
    /// The switch of `event`; `None` for an event this version does not know.
    pub fn get(&self, event: NotifyEvent) -> Option<bool> {
        match event {
            NotifyEvent::Waiting => Some(self.waiting),
            NotifyEvent::Mention => Some(self.mention),
            NotifyEvent::Result => Some(self.result),
            NotifyEvent::Blocked => Some(self.blocked),
            NotifyEvent::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    pub enabled: bool,
    pub start: String,
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            start: "22:00".into(),
            end: "08:00".into(),
        }
    }
}

/// `GET /v1/me/notify-settings`: the member's settings with defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifySettings {
    /// 提醒我.
    pub events: NotifyEventSwitches,
    /// 每日摘要 local time; `None` = no digest.
    pub digest_at: Option<String>,
    pub quiet_hours: QuietHours,
    pub time_zone: String,
    /// 0 = never changed.
    pub updated_at: Millis,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            events: NotifyEventSwitches::default(),
            digest_at: None,
            quiet_hours: QuietHours::default(),
            time_zone: "Asia/Shanghai".into(),
            updated_at: 0,
        }
    }
}

impl NotifySettings {
    pub fn validate(&self) -> Validation {
        if let Some(d) = &self.digest_at {
            check_clock("digestAt", d)?;
        }
        check_clock("quietHours.start", &self.quiet_hours.start)?;
        check_clock("quietHours.end", &self.quiet_hours.end)?;
        check_zone(&self.time_zone)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEventSwitchesPatch {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub waiting: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub mention: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub blocked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietHoursPatch {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub start: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub end: Option<String>,
}

/// `PATCH /v1/me/notify-settings`: only the given fields change.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotifySettingsRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub events: Option<NotifyEventSwitchesPatch>,
    /// `Some(None)` = `null` turns the digest off.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub digest_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub quiet_hours: Option<QuietHoursPatch>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub time_zone: Option<String>,
}

impl UpdateNotifySettingsRequest {
    pub fn validate(&self) -> Validation {
        if let Some(Some(d)) = &self.digest_at {
            check_clock("digestAt", d)?;
        }
        if let Some(q) = &self.quiet_hours {
            if let Some(s) = &q.start {
                check_clock("quietHours.start", s)?;
            }
            if let Some(e) = &q.end {
                check_clock("quietHours.end", e)?;
            }
        }
        if let Some(z) = &self.time_zone {
            check_zone(z)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestItem {
    pub group_id: String,
    pub topic_id: String,
    pub event: NotifyEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestCounts {
    pub waiting: u64,
    pub mention: u64,
    pub result: u64,
    pub blocked: u64,
}

/// Transient `digest` (feature "settings/1" only): what happened since the last digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestTransient {
    pub at: Millis,
    pub since: Millis,
    pub counts: DigestCounts,
    pub items: Vec<DigestItem>,
}

// ---------------------------------------------------------------------------
// Approval defaults for AI members
// ---------------------------------------------------------------------------

/// 合并到 main, 发布到生产, 改生产环境配置, 删除数据或文件, 对外发布.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalActions {
    pub merge_main: bool,
    pub deploy_prod: bool,
    pub prod_config: bool,
    pub delete_data: bool,
    pub external_publish: bool,
}

impl Default for ApprovalActions {
    fn default() -> Self {
        Self {
            merge_main: true,
            deploy_prod: true,
            prod_config: true,
            delete_data: true,
            external_publish: true,
        }
    }
}

/// 谁可以批准. A value this version does not know reads as `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Approver {
    /// 管理员.
    Owners,
    /// 交代的人 (and owners).
    #[default]
    Requester,
    #[serde(other)]
    Unknown,
}

fn check_spend(v: Option<u32>) -> Validation {
    match v {
        Some(n) if !(1..=1_000_000).contains(&n) => invalid("maxSpendCny", "1..1000000 or null"),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPolicy {
    pub ask_before: ApprovalActions,
    /// Whole yuan; `None` = 不限.
    pub max_spend_cny: Option<u32>,
    pub approver: Approver,
    /// 0 = never changed.
    pub updated_at: Millis,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            ask_before: ApprovalActions::default(),
            max_spend_cny: Some(50),
            approver: Approver::Requester,
            updated_at: 0,
        }
    }
}

impl ApprovalPolicy {
    pub fn validate(&self) -> Validation {
        check_spend(self.max_spend_cny)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalActionsPatch {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub merge_main: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deploy_prod: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub prod_config: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub delete_data: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub external_publish: Option<bool>,
}

/// `PATCH /v1/settings/approvals` (owner): only the given fields change.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApprovalPolicyRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask_before: Option<ApprovalActionsPatch>,
    /// `Some(None)` = `null` = 不限.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_spend_cny: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub approver: Option<Approver>,
}

impl UpdateApprovalPolicyRequest {
    pub fn validate(&self) -> Validation {
        if let Some(v) = self.max_spend_cny {
            check_spend(v)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AI 服务: the team's model provider (owner only)
// ---------------------------------------------------------------------------

/// Who answers the server's own model calls (AI 话题标题). A value this version does not know
/// reads as `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AiProvider {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "deepseek")]
    Deepseek,
    #[serde(rename = "openai-compatible")]
    OpenaiCompatible,
    #[serde(other)]
    Unknown,
}

impl AiProvider {
    /// The model suggested when the owner picks this provider.
    pub fn default_model(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "claude-haiku-5-5",
            AiProvider::Deepseek => "deepseek-chat",
            AiProvider::OpenaiCompatible | AiProvider::Unknown => "",
        }
    }
}

/// Printable ASCII without spaces.
fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn check_model(m: &str) -> Validation {
    if m.len() > 128 || !is_token(m) {
        return invalid("model", "1..128 printable characters, no spaces");
    }
    Ok(())
}

fn check_base_url(u: &str) -> Validation {
    if u.len() > 256 || !u.starts_with("https://") || !is_token(&u["https://".len()..]) {
        return invalid("baseUrl", "https URL, at most 256 characters");
    }
    Ok(())
}

/// `GET /v1/settings/ai` (owner). Never carries the key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiServiceConfig {
    /// `None` = not set up.
    pub provider: Option<AiProvider>,
    /// "" when not set up.
    pub model: String,
    /// Only for `openai-compatible`.
    pub base_url: Option<String>,
    pub key_set: bool,
    /// The key's last 4 characters.
    pub key_hint: Option<String>,
    /// 0 = never changed.
    pub updated_at: Millis,
}

impl AiServiceConfig {
    pub fn validate(&self) -> Validation {
        if self.model.len() > 128 {
            return invalid("model", "at most 128 characters");
        }
        if let Some(u) = &self.base_url {
            check_base_url(u)?;
        }
        if self.key_hint.as_ref().is_some_and(|h| h.chars().count() > 4) {
            return invalid("keyHint", "at most 4 characters");
        }
        Ok(())
    }
}

/// `PUT /v1/settings/ai` (owner) and `POST /v1/settings/ai/test`. Without `api_key` the saved
/// key is used (allowed only while the provider stays the same).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAiServiceRequest {
    pub provider: AiProvider,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

pub type TestAiServiceRequest = UpdateAiServiceRequest;

impl UpdateAiServiceRequest {
    pub fn validate(&self) -> Validation {
        if self.provider == AiProvider::Unknown {
            return invalid("provider", "anthropic, deepseek or openai-compatible");
        }
        check_model(&self.model)?;
        match &self.base_url {
            Some(u) => check_base_url(u)?,
            None if self.provider == AiProvider::OpenaiCompatible => {
                return invalid("baseUrl", "openai-compatible needs baseUrl");
            }
            None => {}
        }
        if let Some(k) = &self.api_key
            && (k.len() < 8 || k.len() > 512 || !is_token(k))
        {
            return invalid("apiKey", "8..512 printable characters, no spaces");
        }
        Ok(())
    }
}

/// The answer of 测试连接.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiServiceTestResult {
    pub ok: bool,
    /// The provider's error, when not ok. Plain text, may be English.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub latency_ms: u64,
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// One signed-in device: a live token of mine. `id` is the token's record id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: Millis,
    pub last_used_at: Option<Millis>,
    pub current: bool,
}

/// `GET /v1/me/devices`, newest use first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesResponse {
    pub devices: Vec<Device>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_times() {
        for ok in ["00:00", "09:30", "23:59"] {
            assert!(is_clock_time(ok), "{ok}");
        }
        for bad in ["24:00", "9:00", "12:60", "12-00", "", "ab:cd"] {
            assert!(!is_clock_time(bad), "{bad}");
        }
        assert_eq!(clock_minutes("22:15"), Some(22 * 60 + 15));
        assert_eq!(clock_minutes("x"), None);
    }

    #[test]
    fn lenient_enums_and_defaults() {
        let e: NotifyEvent = serde_json::from_str(r#""someday""#).unwrap();
        assert_eq!(e, NotifyEvent::Unknown);
        let a: Approver = serde_json::from_str(r#""anyone""#).unwrap();
        assert_eq!(a, Approver::Unknown);
        let s = NotifySettings::default();
        assert!(s.validate().is_ok());
        assert_eq!(s.events.get(NotifyEvent::Unknown), None);
        assert_eq!(s.events.get(NotifyEvent::Blocked), Some(true));
        assert_eq!(ApprovalPolicy::default().max_spend_cny, Some(50));
        let o: UpdateApprovalPolicyRequest =
            serde_json::from_str(r#"{"maxSpendCny":null}"#).unwrap();
        assert_eq!(o.max_spend_cny, Some(None));
        assert_eq!(
            serde_json::to_string(&o).unwrap(),
            r#"{"maxSpendCny":null}"#
        );
    }
}
