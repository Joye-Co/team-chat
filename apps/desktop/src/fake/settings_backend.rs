//! settings-backend track: 设置 › 通知, 成员 › 默认规则 and 账号 › 登录的设备 of [`FakeProvider`],
//! in memory. Only owners change the approval rules, as on the server.

use super::{FakeProvider, ME};
use crate::data::*;

const MIN: u64 = 60_000;

pub struct FakeSettingsBackend {
    notify: NotifyPreferences,
    approvals: ApprovalDefaults,
    ai: AiServiceSettings,
    workers: Vec<WorkerModelSetting>,
    devices: Vec<DeviceInfo>,
}

impl FakeSettingsBackend {
    pub fn new(now: Millis) -> Self {
        let now = now.max(0) as u64;
        let dev = |id: &str, name: &str, seen: Option<u64>, this_mac: bool| DeviceInfo {
            id: id.into(),
            name: name.into(),
            kind: DeviceKind::from_name(name),
            signed_in_at: now.saturating_sub(40 * 24 * 60 * MIN),
            last_seen_at: seen,
            this_mac,
        };
        Self {
            notify: NotifyPreferences {
                digest_at: Some(NotifyPreferences::DIGEST_DEFAULT),
                ..NotifyPreferences::default()
            },
            approvals: ApprovalDefaults::default(),
            ai: AiServiceSettings {
                provider: Some(AiProviderKind::Deepseek),
                model: AiProviderKind::Deepseek.default_model().into(),
                base_url: None,
                key_hint: Some("7Qx2".into()),
            },
            workers: vec![WorkerModelSetting {
                runtime: "claude".into(),
                model: Some("opus".into()),
                effort: Some("high".into()),
            }],
            devices: vec![
                dev("tk-mbp", "Joye 的 MacBook Pro", Some(now), true),
                dev(
                    "tk-imac",
                    "办公室 iMac",
                    Some(now.saturating_sub(3 * 60 * MIN)),
                    false,
                ),
                dev(
                    "tk-iphone",
                    "Joye 的 iPhone",
                    Some(now.saturating_sub(2 * 24 * 60 * MIN)),
                    false,
                ),
            ],
        }
    }
}

impl FakeProvider {
    fn sb<R>(&self, f: impl FnOnce(&mut FakeSettingsBackend) -> R) -> R {
        f(&mut self.settings_backend.lock().unwrap())
    }

    pub(super) fn notify_preferences_fake(&self) -> AdminReply<NotifyPreferences> {
        reply_now(Ok(self.sb(|s| s.notify.clone())))
    }

    pub(super) fn set_notify_preferences_fake(
        &self,
        prefs: NotifyPreferences,
    ) -> AdminReply<NotifyPreferences> {
        reply_now(Ok(self.sb(|s| {
            s.notify = prefs;
            s.notify.clone()
        })))
    }

    pub(super) fn approval_defaults_fake(&self) -> AdminReply<ApprovalDefaults> {
        reply_now(Ok(self.sb(|s| s.approvals)))
    }

    pub(super) fn set_approval_defaults_fake(
        &self,
        defaults: ApprovalDefaults,
    ) -> AdminReply<ApprovalDefaults> {
        let owner = self.with(|s| {
            s.members
                .iter()
                .any(|m| m.id == ME && m.level == MemberLevel::Owner)
        });
        if !owner {
            return reply_now(Err(AdminError::Forbidden));
        }
        if defaults
            .max_spend_cny
            .is_some_and(|n| !(1..=1_000_000).contains(&n))
        {
            return reply_now(Err(AdminError::Invalid));
        }
        reply_now(Ok(self.sb(|s| {
            s.approvals = defaults;
            s.approvals
        })))
    }

    fn i_am_owner_fake(&self) -> bool {
        self.with(|s| {
            s.members
                .iter()
                .any(|m| m.id == ME && m.level == MemberLevel::Owner)
        })
    }

    pub(super) fn worker_models_fake(&self) -> AdminReply<Vec<WorkerModelSetting>> {
        reply_now(Ok(self.sb(|s| s.workers.clone())))
    }

    pub(super) fn set_worker_model_fake(
        &self,
        setting: WorkerModelSetting,
    ) -> AdminReply<WorkerModelSetting> {
        if !self.i_am_owner_fake() {
            return reply_now(Err(AdminError::Forbidden));
        }
        self.sb(|s| {
            s.workers.retain(|w| w.runtime != setting.runtime);
            s.workers.push(setting.clone());
        });
        reply_now(Ok(setting))
    }

    pub(super) fn ai_service_fake(&self) -> AdminReply<AiServiceSettings> {
        if !self.i_am_owner_fake() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(Ok(self.sb(|s| s.ai.clone())))
    }

    pub(super) fn set_ai_service_fake(
        &self,
        draft: AiServiceDraft,
    ) -> AdminReply<AiServiceSettings> {
        if !self.i_am_owner_fake() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.sb(|s| {
            draft.check(&s.ai).map_err(|_| AdminError::Invalid)?;
            let hint = match &draft.api_key {
                Some(k) => Some(k[k.len().saturating_sub(4)..].to_string()),
                None => s.ai.key_hint.clone(),
            };
            s.ai = AiServiceSettings {
                provider: Some(draft.provider),
                model: draft.model,
                base_url: draft.base_url.filter(|_| draft.provider.needs_base_url()),
                key_hint: hint,
            };
            Ok(s.ai.clone())
        }))
    }

    /// A key with "bad" in it fails like a provider that refuses the key.
    pub(super) fn test_ai_service_fake(&self, draft: AiServiceDraft) -> AdminReply<AiTestOutcome> {
        if !self.i_am_owner_fake() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.sb(|s| {
            draft.check(&s.ai).map_err(|_| AdminError::Invalid)?;
            Ok(match draft.api_key.as_deref() {
                Some(k) if k.contains("bad") => AiTestOutcome::Failed(
                    "401 Authentication Fails (auth header format should be Bearer sk-...)".into(),
                ),
                _ => AiTestOutcome::Ok { latency_ms: 412 },
            })
        }))
    }

    pub(super) fn devices_fake(&self) -> AdminReply<Vec<DeviceInfo>> {
        reply_now(Ok(self.sb(|s| s.devices.clone())))
    }

    pub(super) fn sign_out_device_fake(&self, device_id: &str) -> AdminReply<()> {
        reply_now(self.sb(|s| {
            let before = s.devices.len();
            s.devices.retain(|d| d.id != device_id);
            if s.devices.len() == before {
                Err(AdminError::NotFound)
            } else {
                Ok(())
            }
        }))
    }
}
