//! macOS glue for the provider: notifications, wake from sleep and network
//! changes.
//!
//! Notifications:
//! - In an app bundle (the DMG build, ad-hoc signed), they go through
//!   `UNUserNotificationCenter`. The notification id carries the group and
//!   topic, and a click comes back on [`clicks`], which the app turns into
//!   "open this topic".
//! - Without a bundle (`cargo run`), `UNUserNotificationCenter` cannot be
//!   used (it raises an exception). The fallback shows the notification with
//!   `osascript` (`display notification`) and bounces the Dock icon; a click
//!   there does not open the topic.
//!
//! Reconnects: an `NSWorkspaceDidWakeNotification` observer, plus a watcher
//! thread that compares the network interfaces every few seconds and notices
//! clock jumps (a sleep the observer missed). Each calls `on_change`.

use std::{
    collections::BTreeSet,
    ptr::NonNull,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use block2::RcBlock;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2::{
    AnyThread, MainThreadMarker, define_class, msg_send, rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType, NSWorkspace, NSWorkspaceDidWakeNotification};
use objc2_foundation::{NSBundle, NSError, NSNotification, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::Notice;

/// Separates the parts of a notification id: group, topic, unique suffix.
const SEP: char = '\u{1f}';

fn click_slot() -> &'static Mutex<Option<UnboundedSender<(String, String)>>> {
    static SLOT: OnceLock<Mutex<Option<UnboundedSender<(String, String)>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Clicked notifications as (group id, topic id). A new call replaces the
/// previous receiver.
pub fn clicks() -> UnboundedReceiver<(String, String)> {
    let (tx, rx) = unbounded();
    *click_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    rx
}

fn on_click(identifier: &str) {
    let mut parts = identifier.split(SEP);
    let (Some(group), Some(topic)) = (parts.next(), parts.next()) else {
        return;
    };
    if group.is_empty() {
        return;
    }
    if let Some(tx) = click_slot().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.unbounded_send((group.to_string(), topic.to_string()));
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class has no ivars.
    #[unsafe(super(NSObject))]
    #[name = "TeamChatNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            on_click(&id);
            completion.call(());
        }

        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: plain NSObject init.
        unsafe { msg_send![super(this), init] }
    }
}

/// How notifications are shown in this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyMode {
    /// `UNUserNotificationCenter` (app bundle).
    UserNotifications,
    /// `osascript` banner and a Dock bounce (no bundle).
    Fallback,
}

static MODE: OnceLock<NotifyMode> = OnceLock::new();

/// Sets up notifications. Call once on the main thread. Asks for permission
/// the first time (macOS shows its own prompt).
pub fn install_notifications(mtm: MainThreadMarker) -> NotifyMode {
    *MODE.get_or_init(|| {
        let bundled = NSBundle::mainBundle().bundleIdentifier().is_some();
        if !bundled {
            tracing::info!("no app bundle: notifications use the osascript fallback");
            return NotifyMode::Fallback;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let _ = mtm;
        let delegate = Delegate::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The center keeps only a weak reference.
        std::mem::forget(delegate);
        let done = RcBlock::new(|granted: Bool, _error: *mut NSError| {
            tracing::info!(granted = granted.as_bool(), "notification permission");
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &done,
        );
        NotifyMode::UserNotifications
    })
}

/// True when this app is the active app.
pub fn app_is_active(mtm: MainThreadMarker) -> bool {
    NSApplication::sharedApplication(mtm).isActive()
}

/// Shows a notification. Call on the main thread.
pub fn post(notice: &Notice, mtm: MainThreadMarker) {
    match MODE.get().copied().unwrap_or(NotifyMode::Fallback) {
        NotifyMode::UserNotifications => {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notice.title));
            content.setBody(&NSString::from_str(&notice.body));
            content.setThreadIdentifier(&NSString::from_str(&notice.group_id));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let id = format!(
                "{}{SEP}{}{SEP}{}",
                notice.group_id,
                notice.topic_id.as_deref().unwrap_or(""),
                chat_core::new_id()
            );
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&id),
                &content,
                None,
            );
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
        NotifyMode::Fallback => {
            let (title, body) = (notice.title.clone(), notice.body.clone());
            // Arguments, not script text: nothing in the message can run as AppleScript.
            std::thread::spawn(move || {
                let _ = std::process::Command::new("/usr/bin/osascript")
                    .args([
                        "-e",
                        "on run argv",
                        "-e",
                        "display notification (item 2 of argv) with title (item 1 of argv)",
                        "-e",
                        "end run",
                        &title,
                        &body,
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            });
            NSApplication::sharedApplication(mtm)
                .requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
        }
    }
}

/// Calls `on_change` on wake from sleep and when the network changes.
/// Call once on the main thread.
pub fn watch_system(on_change: Arc<dyn Fn(&'static str) + Send + Sync>) {
    let wake = on_change.clone();
    let block = RcBlock::new(move |_n: NonNull<NSNotification>| wake("wake"));
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is a valid notification name; the block is Send + Sync.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None,
            None,
            &block,
        )
    };
    // Observe for the life of the process.
    std::mem::forget(observer);
    std::mem::forget(block);

    let spawned = std::thread::Builder::new()
        .name("network-watch".into())
        .spawn(move || {
            const TICK: Duration = Duration::from_secs(3);
            let mut last = interfaces();
            let mut wall = SystemTime::now();
            let mut mono = Instant::now();
            loop {
                std::thread::sleep(TICK);
                let now = interfaces();
                let wall_gap = SystemTime::now()
                    .duration_since(wall)
                    .unwrap_or(Duration::ZERO);
                let mono_gap = mono.elapsed();
                wall = SystemTime::now();
                mono = Instant::now();
                if now != last {
                    tracing::info!(up = now.len(), "network changed");
                    last = now;
                    on_change("network");
                } else if wall_gap > mono_gap + Duration::from_secs(10)
                    || wall_gap > TICK * 10
                {
                    on_change("clock");
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "cannot start the network watcher");
    }
}

/// Up, non-loopback interfaces with an IPv4 or global IPv6 address.
fn interfaces() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills a list that freeifaddrs releases; pointers are
    // checked before use.
    unsafe {
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null()
                || ifa.ifa_flags & libc::IFF_UP as u32 == 0
                || ifa.ifa_flags & libc::IFF_LOOPBACK as u32 != 0
            {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                .to_string_lossy()
                .to_string();
            match (*ifa.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                    if !ip.is_link_local() {
                        out.insert(format!("{name} {ip}"));
                    }
                }
                libc::AF_INET6 => {
                    let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                    // Temporary addresses rotate; only note that a global one exists.
                    if (ip.segments()[0] & 0xe000) == 0x2000 {
                        out.insert(format!("{name} v6"));
                    }
                }
                _ => {}
            }
        }
        libc::freeifaddrs(head);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_ids_carry_group_and_topic() {
        let mut rx = clicks();
        on_click(&format!("g1{SEP}t1{SEP}x"));
        on_click("garbage");
        assert_eq!(rx.try_next().unwrap(), Some(("g1".into(), "t1".into())));
        assert!(rx.try_next().is_err());
    }

    #[test]
    fn interfaces_can_be_listed() {
        // Not empty on a Mac with a network, but never panics.
        let _ = interfaces();
    }
}
