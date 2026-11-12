//! settings-backend track: the [`CoreProvider`] side of 设置 › 通知 (提醒我 switches, 每日摘要,
//! 勿扰), 成员 › 默认规则 (approval rules for AI members) and 账号 › 登录的设备. Each request runs
//! on the chat-core runtime and answers through an [`AdminReply`].

use std::{future::Future, sync::Arc};

use chat_core::{
    ApiError, Client,
    proto::{
        self, ApprovalActionsPatch, NotifyEventSwitchesPatch, QuietHoursPatch,
        UpdateApprovalPolicyRequest, UpdateNotifySettingsRequest,
    },
};

use super::{CoreProvider, settings::api_error};
use crate::data::{
    AdminError, AdminReply, AiProviderKind, AiServiceDraft, AiServiceSettings, AiTestOutcome,
    ApprovalDefaults, ApproverRole, ClockTime, DeviceInfo, DeviceKind, NotifyPreferences,
    WorkerModelSetting, reply_now,
};

/// This computer's IANA time zone. On the Mac from `TZ` or the `/etc/localtime` link
/// (`/var/db/timezone/zoneinfo/Asia/Shanghai`); on Windows from the system's calendar,
/// which names the zone the IANA way. `None` when it cannot be read: the server then keeps
/// the zone it has.
pub(super) fn local_time_zone() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if valid_zone(tz) && tz.contains('/') {
            return Some(tz.to_string());
        }
    }
    system_time_zone().filter(|zone| valid_zone(zone))
}

/// A zone name the server takes: an IANA id, nothing else.
fn valid_zone(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && zone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '/'))
}

#[cfg(not(target_os = "windows"))]
fn system_time_zone() -> Option<String> {
    let link = std::fs::read_link("/etc/localtime").ok()?;
    let text = link.to_string_lossy();
    text.split("zoneinfo/").nth(1).map(str::to_string)
}

/// `Windows.Globalization.Calendar` reports the system zone as an IANA id
/// (`Asia/Shanghai`), unlike Win32, which names it Windows' own way (`China Standard Time`).
#[cfg(target_os = "windows")]
fn system_time_zone() -> Option<String> {
    let zone = windows::Globalization::Calendar::new()
        .and_then(|calendar| calendar.GetTimeZone())
        .ok()?;
    Some(zone.to_string())
}

/// A read on a server without the settings routes (404) means the server is too old.
fn worker_from(w: &proto::WorkerModel) -> WorkerModelSetting {
    WorkerModelSetting {
        runtime: w.runtime.clone(),
        model: w.model.clone(),
        effort: w.effort.clone(),
    }
}

fn read_error(e: &ApiError) -> AdminError {
    match api_error(e) {
        AdminError::NotFound => AdminError::Unsupported,
        other => other,
    }
}

fn clock(text: &str, fallback: ClockTime) -> ClockTime {
    ClockTime::parse(text).unwrap_or(fallback)
}

pub(super) fn prefs_from(s: &proto::NotifySettings) -> NotifyPreferences {
    let d = NotifyPreferences::default();
    NotifyPreferences {
        waiting: s.events.waiting,
        result: s.events.result,
        blocked: s.events.blocked,
        mention: s.events.mention,
        digest_at: s.digest_at.as_deref().and_then(ClockTime::parse),
        quiet_enabled: s.quiet_hours.enabled,
        quiet_start: clock(&s.quiet_hours.start, d.quiet_start),
        quiet_end: clock(&s.quiet_hours.end, d.quiet_end),
    }
}

pub(super) fn prefs_request(
    p: &NotifyPreferences,
    zone: Option<String>,
) -> UpdateNotifySettingsRequest {
    UpdateNotifySettingsRequest {
        events: Some(NotifyEventSwitchesPatch {
            waiting: Some(p.waiting),
            mention: Some(p.mention),
            result: Some(p.result),
            blocked: Some(p.blocked),
        }),
        digest_at: Some(p.digest_at.map(ClockTime::wire)),
        quiet_hours: Some(QuietHoursPatch {
            enabled: Some(p.quiet_enabled),
            start: Some(p.quiet_start.wire()),
            end: Some(p.quiet_end.wire()),
        }),
        time_zone: zone,
    }
}

pub(super) fn defaults_from(p: &proto::ApprovalPolicy) -> ApprovalDefaults {
    ApprovalDefaults {
        merge_main: p.ask_before.merge_main,
        deploy_prod: p.ask_before.deploy_prod,
        prod_config: p.ask_before.prod_config,
        delete_data: p.ask_before.delete_data,
        external_publish: p.ask_before.external_publish,
        max_spend_cny: p.max_spend_cny,
        // A role this version does not know reads as the stricter one.
        approver: match p.approver {
            proto::Approver::Requester => ApproverRole::Requester,
            proto::Approver::Owners | proto::Approver::Unknown => ApproverRole::Owners,
        },
    }
}

pub(super) fn defaults_request(d: &ApprovalDefaults) -> UpdateApprovalPolicyRequest {
    UpdateApprovalPolicyRequest {
        ask_before: Some(ApprovalActionsPatch {
            merge_main: Some(d.merge_main),
            deploy_prod: Some(d.deploy_prod),
            prod_config: Some(d.prod_config),
            delete_data: Some(d.delete_data),
            external_publish: Some(d.external_publish),
        }),
        max_spend_cny: Some(d.max_spend_cny),
        approver: Some(match d.approver {
            ApproverRole::Owners => proto::Approver::Owners,
            ApproverRole::Requester => proto::Approver::Requester,
        }),
        // The team's own patterns and per-kind approvers are kept as they are.
        ..Default::default()
    }
}

pub(super) fn ai_from(c: &proto::AiServiceConfig) -> AiServiceSettings {
    let provider = c.provider.and_then(|p| match p {
        proto::AiProvider::Deepseek => Some(AiProviderKind::Deepseek),
        proto::AiProvider::Anthropic => Some(AiProviderKind::Anthropic),
        proto::AiProvider::OpenaiCompatible => Some(AiProviderKind::OpenaiCompatible),
        // A provider this version does not know reads as not set up; saving replaces it.
        proto::AiProvider::Unknown => None,
    });
    AiServiceSettings {
        provider,
        model: if provider.is_some() {
            c.model.clone()
        } else {
            String::new()
        },
        base_url: c.base_url.clone(),
        key_hint: c
            .key_hint
            .clone()
            .filter(|_| c.key_set && provider.is_some()),
    }
}

pub(super) fn ai_request(d: &AiServiceDraft) -> proto::UpdateAiServiceRequest {
    proto::UpdateAiServiceRequest {
        provider: match d.provider {
            AiProviderKind::Deepseek => proto::AiProvider::Deepseek,
            AiProviderKind::Anthropic => proto::AiProvider::Anthropic,
            AiProviderKind::OpenaiCompatible => proto::AiProvider::OpenaiCompatible,
        },
        model: d.model.clone(),
        base_url: d.base_url.clone().filter(|_| d.provider.needs_base_url()),
        api_key: d.api_key.clone(),
    }
}

pub(super) fn device(d: &proto::Device) -> DeviceInfo {
    DeviceInfo {
        id: d.id.clone(),
        name: d.name.clone(),
        kind: DeviceKind::from_name(&d.name),
        signed_in_at: d.created_at,
        last_seen_at: d.last_used_at,
        this_mac: d.current,
    }
}

impl CoreProvider {
    fn sb_request<T, F>(&self, work: impl FnOnce(Arc<Client>) -> F) -> AdminReply<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, AdminError>> + Send + 'static,
    {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        let fut = work(a.client.clone());
        a.client.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx
    }

    pub(super) fn notify_preferences_inner(&self) -> AdminReply<NotifyPreferences> {
        self.sb_request(|client| async move {
            let s = client
                .http()
                .notify_settings()
                .await
                .map_err(|e| read_error(&e))?;
            Ok(prefs_from(&s))
        })
    }

    pub(super) fn set_notify_preferences_inner(
        &self,
        prefs: NotifyPreferences,
    ) -> AdminReply<NotifyPreferences> {
        let req = prefs_request(&prefs, local_time_zone());
        self.sb_request(move |client| async move {
            let s = client
                .http()
                .update_notify_settings(&req)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(prefs_from(&s))
        })
    }

    pub(super) fn approval_defaults_inner(&self) -> AdminReply<ApprovalDefaults> {
        self.sb_request(|client| async move {
            let p = client
                .http()
                .approval_policy()
                .await
                .map_err(|e| read_error(&e))?;
            Ok(defaults_from(&p))
        })
    }

    pub(super) fn set_approval_defaults_inner(
        &self,
        defaults: ApprovalDefaults,
    ) -> AdminReply<ApprovalDefaults> {
        let req = defaults_request(&defaults);
        self.sb_request(move |client| async move {
            let p = client
                .http()
                .update_approval_policy(&req)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(defaults_from(&p))
        })
    }

    pub(super) fn ai_service_inner(&self) -> AdminReply<AiServiceSettings> {
        self.sb_request(|client| async move {
            let c = client
                .http()
                .ai_service()
                .await
                .map_err(|e| read_error(&e))?;
            Ok(ai_from(&c))
        })
    }

    pub(super) fn set_ai_service_inner(
        &self,
        draft: AiServiceDraft,
    ) -> AdminReply<AiServiceSettings> {
        let req = ai_request(&draft);
        self.sb_request(move |client| async move {
            let c = client
                .http()
                .update_ai_service(&req)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(ai_from(&c))
        })
    }

    pub(super) fn worker_models_inner(&self) -> AdminReply<Vec<WorkerModelSetting>> {
        self.sb_request(|client| async move {
            let r = client
                .http()
                .worker_models()
                .await
                .map_err(|e| read_error(&e))?;
            Ok(r.workers.iter().map(worker_from).collect())
        })
    }

    pub(super) fn set_worker_model_inner(
        &self,
        setting: WorkerModelSetting,
    ) -> AdminReply<WorkerModelSetting> {
        let req = proto::SetWorkerModelRequest {
            model: Some(setting.model.clone()),
            effort: Some(setting.effort.clone()),
        };
        self.sb_request(move |client| async move {
            let w = client
                .http()
                .set_worker_model(&setting.runtime, &req)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(worker_from(&w))
        })
    }

    pub(super) fn test_ai_service_inner(&self, draft: AiServiceDraft) -> AdminReply<AiTestOutcome> {
        let req = ai_request(&draft);
        self.sb_request(move |client| async move {
            let r = client
                .http()
                .test_ai_service(&req)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(if r.ok {
                AiTestOutcome::Ok {
                    latency_ms: r.latency_ms,
                }
            } else {
                AiTestOutcome::Failed(r.error.unwrap_or_default())
            })
        })
    }

    pub(super) fn devices_inner(&self) -> AdminReply<Vec<DeviceInfo>> {
        self.sb_request(|client| async move {
            let r = client.http().devices().await.map_err(|e| read_error(&e))?;
            Ok(r.devices.iter().map(device).collect())
        })
    }

    pub(super) fn sign_out_device_inner(&self, device_id: &str) -> AdminReply<()> {
        let id = device_id.to_string();
        self.sb_request(move |client| async move {
            // NotFound: the device was signed out already (elsewhere): refresh the list.
            client
                .http()
                .sign_out_device(&id)
                .await
                .map_err(|e| api_error(&e))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_iana_names_are_sent() {
        assert!(valid_zone("Asia/Shanghai"));
        assert!(valid_zone("Etc/GMT+8"));
        assert!(!valid_zone("China Standard Time"));
        assert!(!valid_zone(""));
    }

    #[test]
    fn preferences_round_trip_through_the_wire_request() {
        let p = NotifyPreferences {
            result: false,
            digest_at: Some(ClockTime::new(9, 0)),
            quiet_enabled: true,
            quiet_start: ClockTime::new(23, 30),
            ..NotifyPreferences::default()
        };
        let req = prefs_request(&p, Some("Asia/Shanghai".into()));
        assert!(req.validate().is_ok());
        assert_eq!(req.digest_at, Some(Some("09:00".into())));
        let mut stored = proto::NotifySettings::default();
        stored.events.result = false;
        stored.digest_at = Some("09:00".into());
        stored.quiet_hours.enabled = true;
        stored.quiet_hours.start = "23:30".into();
        assert_eq!(prefs_from(&stored), p);
        assert_eq!(
            prefs_from(&proto::NotifySettings::default()),
            NotifyPreferences::default()
        );
        let off = prefs_request(&NotifyPreferences::default(), None);
        assert_eq!(off.digest_at, Some(None));
        assert_eq!(off.time_zone, None);
    }

    #[test]
    fn approval_defaults_round_trip_and_unknown_approver_reads_as_owners() {
        assert_eq!(
            defaults_from(&proto::ApprovalPolicy::default()),
            ApprovalDefaults::default()
        );
        let want = ApprovalDefaults {
            merge_main: false,
            max_spend_cny: None,
            approver: ApproverRole::Owners,
            ..ApprovalDefaults::default()
        };
        let req = defaults_request(&want);
        assert!(req.validate().is_ok());
        assert_eq!(req.max_spend_cny, Some(None));
        let p = proto::ApprovalPolicy {
            approver: proto::Approver::Unknown,
            ..Default::default()
        };
        assert_eq!(defaults_from(&p).approver, ApproverRole::Owners);
        let over = ApprovalDefaults {
            max_spend_cny: Some(0),
            ..ApprovalDefaults::default()
        };
        assert!(defaults_request(&over).validate().is_err());
    }

    #[test]
    fn ai_service_reads_without_the_key_and_sends_only_what_the_provider_needs() {
        let stored = proto::AiServiceConfig {
            provider: Some(proto::AiProvider::Deepseek),
            model: "deepseek-chat".into(),
            base_url: None,
            key_set: true,
            key_hint: Some("a1b2".into()),
            updated_at: 1,
            ..Default::default()
        };
        let s = ai_from(&stored);
        assert_eq!(s.provider, Some(AiProviderKind::Deepseek));
        assert_eq!(s.key_hint.as_deref(), Some("a1b2"));
        let unknown = proto::AiServiceConfig {
            provider: Some(proto::AiProvider::Unknown),
            ..stored.clone()
        };
        assert_eq!(ai_from(&unknown), AiServiceSettings::default());
        assert_eq!(
            ai_from(&proto::AiServiceConfig::default()),
            AiServiceSettings::default()
        );

        // The saved key works for the same provider only.
        let keep = AiServiceDraft {
            provider: AiProviderKind::Deepseek,
            model: "deepseek-chat".into(),
            base_url: Some("https://ignored.example".into()),
            api_key: None,
        };
        assert_eq!(keep.check(&s), Ok(()));
        let req = ai_request(&keep);
        assert!(req.validate().is_ok());
        assert_eq!((req.base_url, req.api_key), (None, None));
        let switch = AiServiceDraft {
            provider: AiProviderKind::Anthropic,
            ..keep.clone()
        };
        assert_eq!(
            switch.check(&s),
            Err(crate::data::AiDraftProblem::KeyRequired)
        );
        let compat = AiServiceDraft {
            provider: AiProviderKind::OpenaiCompatible,
            base_url: Some("http://x".into()),
            api_key: Some("sk-test-0000".into()),
            ..keep.clone()
        };
        assert_eq!(
            compat.check(&s),
            Err(crate::data::AiDraftProblem::BaseUrlInvalid)
        );
        let spaced = AiServiceDraft {
            api_key: Some("sk test 0000".into()),
            ..keep
        };
        assert_eq!(
            spaced.check(&s),
            Err(crate::data::AiDraftProblem::KeyInvalid)
        );
    }

    #[test]
    fn device_kind_from_the_sign_in_name() {
        for (name, kind) in [
            ("Joye 的 MacBook Pro", DeviceKind::Laptop),
            ("Joye's MacBook Air", DeviceKind::Laptop),
            ("Joye 的 iMac", DeviceKind::Desktop),
            ("Mac mini", DeviceKind::Desktop),
            ("Studio Mac Pro", DeviceKind::Desktop),
            ("Joye 的 iPhone", DeviceKind::Phone),
            ("", DeviceKind::Laptop),
        ] {
            assert_eq!(DeviceKind::from_name(name), kind, "{name}");
        }
        assert_eq!(DeviceKind::Desktop.symbol(), "desktopcomputer");
        let d = device(&proto::Device {
            id: "TK".into(),
            name: "Joye 的 iPhone".into(),
            created_at: 1,
            last_used_at: None,
            current: false,
        });
        assert_eq!(
            (d.kind, d.this_mac, d.last_seen_at),
            (DeviceKind::Phone, false, None)
        );
    }

    #[test]
    fn clock_time_parses_and_formats() {
        assert_eq!(ClockTime::parse("07:05"), Some(ClockTime::new(7, 5)));
        assert_eq!(ClockTime::parse("7:05"), None);
        assert_eq!(ClockTime::new(18, 0).wire(), "18:00");
    }
}

// ---------------------------------------------------------------------------
// Notices for the `blocked` event and the digest
// ---------------------------------------------------------------------------

/// The title of a `blocked` notification (成员卡住或出错); `None` for the other events.
pub(super) fn event_title(n: &chat_core::Notification, group: &str) -> Option<String> {
    use crate::copy::{fill, settings_backend as sb};
    match n.event? {
        proto::NotifyEvent::Blocked => Some(fill(sb::BLOCKED_TITLE, &[("group", group)])),
        _ => None,
    }
}

/// The body of a digest notice: the counts that are not zero, in the order of the 通知 pane.
pub(super) fn digest_body(d: &proto::DigestTransient) -> String {
    use crate::copy::{fill, settings_backend as sb};
    let c = &d.counts;
    let parts: Vec<String> = [
        (c.waiting, sb::DIGEST_WAITING),
        (c.result, sb::DIGEST_RESULT),
        (c.blocked, sb::DIGEST_BLOCKED),
        (c.mention, sb::DIGEST_MENTION),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, text)| fill(text, &[("n", &n.to_string())]))
    .collect();
    if parts.is_empty() {
        sb::DIGEST_EMPTY.to_string()
    } else {
        parts.join(sb::DIGEST_JOIN)
    }
}

impl CoreProvider {
    pub(super) fn digest_notice(&self, d: &proto::DigestTransient) -> super::Notice {
        // The newest item opens when the notice is clicked.
        let first = d.items.first();
        super::Notice {
            title: crate::copy::settings_backend::DIGEST_TITLE.to_string(),
            body: digest_body(d),
            group_id: first.map(|i| i.group_id.clone()).unwrap_or_default(),
            topic_id: first.map(|i| i.topic_id.clone()),
        }
    }
}

#[cfg(test)]
mod notice_tests {
    use super::*;

    #[test]
    fn digest_body_lists_only_counts_above_zero() {
        let mut d = proto::DigestTransient {
            at: 0,
            since: 0,
            counts: proto::DigestCounts::default(),
            items: vec![],
        };
        assert_eq!(digest_body(&d), crate::copy::settings_backend::DIGEST_EMPTY);
        d.counts.waiting = 2;
        d.counts.result = 1;
        assert_eq!(digest_body(&d), "2 个话题等你拍板，1 个话题有了结果");
    }
}
