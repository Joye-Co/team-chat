//! settings-backend track: 设置 › 通知, 成员 › 默认规则 and 账号 › 登录的设备 of [`FakeProvider`],
//! in memory. Only owners change the approval rules, as on the server.

use super::{FakeProvider, ME};
use crate::data::*;

const MIN: u64 = 60_000;

pub struct FakeSettingsBackend {
    notify: NotifyPreferences,
    approvals: ApprovalDefaults,
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
