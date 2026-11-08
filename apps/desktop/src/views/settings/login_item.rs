//! 登录时启动 (设置 › 通用): the app's own login item through
//! `SMAppService.mainAppService` (macOS 13+). The system owns the truth: the
//! person can also turn it off in 系统设置 › 通用 › 登录项, so the switch reads
//! the service's status, not only the saved choice.
//!
//! Without an app bundle (`cargo run`) there is no service: the switch keeps
//! the saved choice and nothing is registered.

/// `SMAppServiceStatus`: what the system says about the login item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotRegistered,
    Enabled,
    /// Registered, but the person has to allow it in 登录项.
    RequiresApproval,
    NotFound,
}

impl Status {
    fn from_raw(raw: isize) -> Self {
        match raw {
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            3 => Self::NotFound,
            _ => Self::NotRegistered,
        }
    }

    /// The switch is on while the item is registered.
    pub fn is_on(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::{NSBundle, NSError};

    use super::Status;

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    fn main_app() -> Option<Retained<AnyObject>> {
        // A process without a bundle id has no login item of its own.
        NSBundle::mainBundle().bundleIdentifier()?;
        let class = AnyClass::get(c"SMAppService")?;
        // SAFETY: `+[SMAppService mainAppService]` returns the app's service.
        unsafe { msg_send![class, mainAppService] }
    }

    pub fn status() -> Option<Status> {
        let service = main_app()?;
        // SAFETY: `status` is an NSInteger property.
        let raw: isize = unsafe { msg_send![&service, status] };
        Some(Status::from_raw(raw))
    }

    pub fn set(on: bool) -> Result<Status, String> {
        let service = main_app().ok_or("no app bundle")?;
        // SAFETY: both methods take an NSError out-parameter and return BOOL.
        let done: Result<(), Retained<NSError>> = unsafe {
            if on {
                msg_send![&service, registerAndReturnError: _]
            } else {
                msg_send![&service, unregisterAndReturnError: _]
            }
        };
        done.map_err(|e| e.localizedDescription().to_string())?;
        let now = status().unwrap_or(Status::NotRegistered);
        if now == Status::RequiresApproval {
            // The person has to allow it: show them where.
            let class = AnyClass::get(c"SMAppService").ok_or("no SMAppService")?;
            // SAFETY: a class method without arguments.
            let _: () = unsafe { msg_send![class, openSystemSettingsLoginItems] };
        }
        Ok(now)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Status;
    pub fn status() -> Option<Status> {
        None
    }
    pub fn set(_on: bool) -> Result<Status, String> {
        Err("not macOS".into())
    }
}

thread_local! {
    /// What the system said last; `None` until it is asked. Asking is a
    /// synchronous XPC call to the system (about 1.5 ms), too slow for every
    /// frame of the 通用 pane, so the pane asks again only when it is shown,
    /// when its window comes to the front and after the switch changes it.
    static LAST: std::cell::Cell<Option<Option<Status>>> = const { std::cell::Cell::new(None) };
}

/// Asks the system again (the person may have changed it in 系统设置 ›
/// 登录项), and keeps the answer for [`status`].
pub fn refresh() -> Option<Status> {
    let now = if super::prefs::saving() {
        imp::status()
    } else {
        None
    };
    LAST.with(|l| l.set(Some(now)));
    now
}

/// What the system said last, or `None` when this process has no login item
/// (no bundle, self-tests).
pub fn status() -> Option<Status> {
    LAST.with(|l| l.get()).unwrap_or_else(refresh)
}

/// Whether the switch shows on: the system's state, else the saved choice.
pub fn shown_on(saved: bool) -> bool {
    status().map_or(saved, Status::is_on)
}

/// Turns the login item on or off and saves what the system now says.
pub fn set(on: bool) {
    if !super::prefs::saving() {
        return;
    }
    let result = match imp::status() {
        Some(_) => imp::set(on).map(Status::is_on),
        None => Ok(on),
    };
    let now = match result {
        Ok(now) => now,
        Err(error) => {
            tracing::warn!(%error, on, "login item");
            imp::status().is_some_and(Status::is_on)
        }
    };
    refresh();
    super::prefs::update(|p| p.launch_at_login = now);
}

#[cfg(test)]
mod tests {
    use super::Status;

    #[test]
    fn raw_status_maps_to_the_switch() {
        assert!(!Status::from_raw(0).is_on());
        assert!(Status::from_raw(1).is_on());
        assert!(Status::from_raw(2).is_on());
        assert!(!Status::from_raw(3).is_on());
    }
}
