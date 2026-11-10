//! Windows glue for the provider: notifications, wake from sleep and network changes.
//! The same surface as [`super::macos`].
//!
//! Notifications are WinRT toasts through GPUI (`App::show_system_notification`). The app
//! has no package identity (an installer build and `cargo run` alike), so GPUI gives the
//! process an explicit AppUserModelID and registers its display name under
//! `HKCU\Software\Classes\AppUserModelId\<id>`; no Start menu shortcut is needed. The tag
//! of a toast carries the group and topic, and a click comes back on [`clicks`] while the
//! app runs. A click on a toast left in the notification center after the app quit only
//! starts nothing: there is no COM activator.
//!
//! Next to every toast the taskbar button flashes until the app comes forward, so a
//! notification is noticed even when toasts are off for the app or Focus Assist holds
//! them back. GPUI's toasts are silent; the sound of 设置 › 通用 › 声音 is the system's
//! notification sound, played here.
//!
//! Reconnects: GPUI's `on_system_wake` (`WM_POWERBROADCAST`), plus a watcher thread that
//! compares the network adapters' addresses every few seconds and notices clock jumps (a
//! sleep the broadcast missed). Each calls `on_change`.

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{App, SystemNotification};

use super::{
    Notice,
    system_events::{clock_jumped, interface_key, notice_tag, parse_tag},
};

type ClickSlot = Mutex<Option<UnboundedSender<(String, String)>>>;

fn click_slot() -> &'static ClickSlot {
    static SLOT: OnceLock<ClickSlot> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Clicked notifications as (group id, topic id). A new call replaces the previous
/// receiver.
pub fn clicks() -> UnboundedReceiver<(String, String)> {
    let (tx, rx) = unbounded();
    *click_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    rx
}

fn on_click(tag: &str) {
    let Some(target) = parse_tag(tag) else {
        return;
    };
    if let Some(tx) = click_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = tx.unbounded_send(target);
    }
}

/// How notifications are shown in this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyMode {
    /// WinRT toasts under the app's AppUserModelID, with the taskbar flash.
    Toast,
}

/// Sets up notifications. Call once on the main thread, before the first window opens
/// (the taskbar groups windows by the AppUserModelID set here).
pub fn install_notifications(cx: &mut App) -> NotifyMode {
    cx.set_app_identity(super::session::app_id(), env!("PRODUCT_NAME"));
    cx.on_system_notification_response(|response, _| on_click(&response.tag));
    NotifyMode::Toast
}

/// True when the foreground window is a visible, not minimized window of this app. The
/// hidden events window does not count: it becomes the foreground window while the
/// notification-area menu is up and stays so after the menu closes, with nothing of the
/// app on screen.
pub fn app_is_active() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    };
    let mut pid = 0u32;
    // SAFETY: every call only reads; a null foreground window is checked first.
    let (visible, minimized) = unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        (
            IsWindowVisible(foreground).as_bool(),
            IsIconic(foreground).as_bool(),
        )
    };
    counts_as_active(pid == std::process::id(), visible, minimized)
}

/// The app is in front only with one of its windows on screen.
fn counts_as_active(ours: bool, visible: bool, minimized: bool) -> bool {
    ours && visible && !minimized
}

/// The tags of the toasts posted by this run, per group, so they can be taken back out
/// of the notification center once the group is read.
static POSTED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Takes the toasts of `read_groups` (groups with nothing unread) out of the
/// notification center. Call on the main thread.
pub fn withdraw_read(read_groups: &[&str], cx: &App) {
    let tags: Vec<String> = {
        let mut posted = POSTED.lock().unwrap_or_else(|e| e.into_inner());
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *posted)
            .into_iter()
            .partition(|(group, _)| read_groups.contains(&group.as_str()));
        *posted = kept;
        gone.into_iter().map(|(_, tag)| tag).collect()
    };
    for tag in tags {
        cx.dismiss_system_notification(&tag);
    }
}

/// Shows a notification, with the system's notification sound when `sound` (设置 › 通用 ›
/// 声音), and flashes the taskbar button. Call on the main thread.
pub fn post(notice: &Notice, sound: bool, cx: &mut App) {
    let tag = notice_tag(
        &notice.group_id,
        notice.topic_id.as_deref(),
        &chat_core::new_id(),
    );
    cx.show_system_notification(SystemNotification {
        tag: tag.clone().into(),
        title: notice.title.clone().into(),
        body: notice.body.clone().into(),
        actions: Vec::new(),
    });
    POSTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((notice.group_id.clone(), tag));
    if sound {
        play_notification_sound();
    }
    flash_taskbar(cx);
}

fn play_notification_sound() {
    use windows::{
        Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT},
        core::w,
    };
    // SAFETY: the alias is a static string; SND_ASYNC returns at once.
    unsafe {
        let _ = PlaySoundW(
            w!("Notification.Default"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// Flashes the taskbar button of the app's first window until the app comes forward.
fn flash_taskbar(cx: &mut App) {
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
    };
    let Some(hwnd) = cx.windows().into_iter().find_map(|handle| {
        handle
            .update(cx, |_, window, _| crate::win32::hwnd(window))
            .ok()?
    }) else {
        return;
    };
    let info = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd,
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    // SAFETY: `info` is a valid FLASHWINFO for a window of this thread.
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}

/// Calls `on_change` on wake from sleep and when the network changes. Call once on the
/// main thread.
pub fn watch_system(on_change: Arc<dyn Fn(&'static str) + Send + Sync>, cx: &mut App) {
    let wake = on_change.clone();
    cx.on_system_wake(move |_| wake("wake")).detach();

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
                } else if clock_jumped(wall_gap, mono_gap, TICK) {
                    on_change("clock");
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "cannot start the network watcher");
    }
}

/// The addresses of the adapters that are up and not loopback, as
/// [`interface_key`] counts them.
fn interfaces() -> BTreeSet<String> {
    use windows::Win32::{
        Foundation::ERROR_BUFFER_OVERFLOW,
        NetworkManagement::{
            IpHelper::{
                GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
                GetAdaptersAddresses, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
            },
            Ndis::IfOperStatusUp,
        },
        Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC},
    };

    let mut out = BTreeSet::new();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    for _ in 0..4 {
        // u64 storage keeps the list aligned for its structs.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let head = buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `head` points to `size` writable bytes; the call fills a linked list
        // inside that buffer or reports the size it needs.
        let rc =
            unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(head), &mut size) };
        if rc == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if rc != 0 {
            return out;
        }
        // SAFETY: on success every `Next`, `FirstUnicastAddress` and `lpSockaddr` pointer
        // is null or points into `buf`, which lives until the end of this block. Socket
        // addresses are read by their documented layout: the family in the first two
        // bytes, an IPv4 address at offset 4, an IPv6 address at offset 8.
        unsafe {
            let mut adapter = head.cast_const();
            while !adapter.is_null() {
                let a = &*adapter;
                adapter = a.Next;
                if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
                    continue;
                }
                let name = a.AdapterName.to_string().unwrap_or_default();
                let mut unicast = a.FirstUnicastAddress.cast_const();
                while !unicast.is_null() {
                    let u = &*unicast;
                    unicast = u.Next;
                    let sockaddr = u.Address.lpSockaddr.cast::<u8>().cast_const();
                    let len = u.Address.iSockaddrLength as usize;
                    if sockaddr.is_null() || len < 2 {
                        continue;
                    }
                    let family = sockaddr.cast::<u16>().read_unaligned();
                    let ip = if family == AF_INET.0 && len >= 8 {
                        let mut b = [0u8; 4];
                        std::ptr::copy_nonoverlapping(sockaddr.add(4), b.as_mut_ptr(), 4);
                        IpAddr::V4(Ipv4Addr::from(b))
                    } else if family == AF_INET6.0 && len >= 24 {
                        let mut b = [0u8; 16];
                        std::ptr::copy_nonoverlapping(sockaddr.add(8), b.as_mut_ptr(), 16);
                        IpAddr::V6(Ipv6Addr::from(b))
                    } else {
                        continue;
                    };
                    if let Some(key) = interface_key(&name, ip) {
                        out.insert(key);
                    }
                }
            }
        }
        return out;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_tags_carry_group_and_topic() {
        let mut rx = clicks();
        on_click(&notice_tag("g1", Some("t1"), "x"));
        on_click("garbage");
        assert_eq!(rx.try_recv().ok(), Some(("g1".into(), "t1".into())));
        assert!(rx.try_recv().is_err());
    }

    /// A CI runner has a network: the adapter list is read and has an address.
    #[test]
    fn interfaces_can_be_listed() {
        let up = interfaces();
        assert!(!up.is_empty(), "no adapter with an address: {up:?}");
        assert_eq!(up, interfaces(), "two reads in a row agree");
    }

    #[test]
    fn this_process_is_not_the_foreground_app_or_is() {
        // Never panics, with or without a desktop session.
        let _ = app_is_active();
    }

    #[test]
    fn a_hidden_or_minimized_foreground_window_is_not_active() {
        assert!(counts_as_active(true, true, false));
        assert!(
            !counts_as_active(true, false, false),
            "the hidden events window"
        );
        assert!(!counts_as_active(true, true, true), "minimized");
        assert!(!counts_as_active(false, true, false), "another app");
    }

    /// The events window is never visible, so it can never make the app count as active.
    #[test]
    fn the_events_window_is_hidden() {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        let hwnd = crate::win32::events_window().expect("an events window");
        // SAFETY: a live window of this thread.
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
    }
}
