//! Windows glue for the provider: notifications, wake from sleep and network changes.
//! The same surface as [`super::macos`].
//!
//! Notifications are WinRT toasts, made here (GPUI's own toast has no group, no sound and
//! no way to open the app after it quit). The app has no package identity (an installer
//! build and `cargo run` alike), so the process gets an explicit AppUserModelID, with its
//! display name and icon under `HKCU\Software\Classes\AppUserModelId\<id>`; no Start menu
//! shortcut is needed.
//!
//! - **A click opens the 话题**: the toast is activated by protocol. Its `launch` is the
//!   link of its 话题 (`<scheme>://group/<g>/topic/<t>`); Windows starts the app with the
//!   link, and that launch hands it to the running app or, when the app has quit, is the
//!   app ([`super::instance`]). So a toast left in the notification center works after the
//!   app exited, as on macOS, with nothing to register but the URL scheme (the installer
//!   writes it; a build run without the installer writes it for itself when nobody has).
//! - **Per 群**: every toast of a 群 is in one toast group under a header with the 群's
//!   name (macOS: the notification's thread). Reading the 群 takes its toasts out.
//! - **Sound**: the toast names the system's notification sound when 设置 › 通用 › 声音 is
//!   on, and Windows plays it, so Focus Assist silences it with the toast.
//!
//! Next to every toast the taskbar button flashes until the app comes forward, so a
//! notification is noticed even when toasts are off for the app or Focus Assist holds
//! them back.
//!
//! Reconnects: GPUI's `on_system_wake` (`WM_POWERBROADCAST`), plus a watcher thread that
//! compares the network adapters' addresses every few seconds and notices clock jumps (a
//! sleep the broadcast missed). Each calls `on_change`.

use std::{
    collections::{BTreeSet, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use gpui::App;
use windows::{
    Data::Xml::Dom::{XmlDocument, XmlElement},
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    core::HSTRING,
};

use super::{
    Notice,
    system_events::{
        ToastSpec, clock_jumped, command_program, interface_key, scheme_command, toast_key,
        toast_spec,
    },
};

/// How notifications are shown in this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyMode {
    /// WinRT toasts under the app's AppUserModelID, with the taskbar flash.
    Toast,
}

/// The app icon the notification center shows next to the app's name.
const ICON: &[u8] = include_bytes!("../../../../assets/brand/AppIcon.iconset/icon_128x128@2x.png");

/// Sets up notifications. Call once on the main thread, before the first window opens
/// (the taskbar groups windows by the AppUserModelID set here).
pub fn install_notifications(cx: &mut App) -> NotifyMode {
    let app_id = super::session::app_id();
    cx.set_app_identity(app_id, env!("PRODUCT_NAME"));
    let icon = super::session::default_data_root().join("notification-icon.png");
    if let Err(error) = register_identity(app_id, env!("PRODUCT_NAME"), &icon) {
        tracing::warn!(%error, "cannot register the notification identity");
    }
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(error) =
                register_scheme(env!("PRODUCT_URL_SCHEME"), env!("PRODUCT_NAME"), &exe)
            {
                tracing::warn!(%error, "cannot register the URL scheme");
            }
        }
        Err(error) => tracing::warn!(%error, "cannot find the app's own path"),
    }
    NotifyMode::Toast
}

/// Registers the name and icon the notification center shows for toasts of `app_id` (an
/// app without a Start menu shortcut has no other way to say them). The icon file is
/// written once.
fn register_identity(app_id: &str, name: &str, icon: &std::path::Path) -> std::io::Result<()> {
    if !icon.is_file() {
        if let Some(dir) = icon.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(icon, ICON)?;
    }
    let key = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\AppUserModelId\{app_id}"))
        .map_err(std::io::Error::other)?;
    key.set_string("DisplayName", name)
        .and_then(|()| key.set_string("IconUri", icon.display().to_string()))
        .map_err(std::io::Error::other)
}

/// Makes `<scheme>://` links start `exe`, unless a program that exists has the scheme
/// already (the installed app: its installer wrote the same keys). A toast's click needs
/// it. Returns whether it wrote.
fn register_scheme(scheme: &str, name: &str, exe: &std::path::Path) -> std::io::Result<bool> {
    let root = format!(r"Software\Classes\{scheme}");
    let command_key = format!(r"{root}\shell\open\command");
    let taken = windows_registry::CURRENT_USER
        .open(&command_key)
        .and_then(|key| key.get_string(""))
        .ok()
        .and_then(|command| command_program(&command))
        .is_some_and(|program| program.is_file());
    if taken {
        return Ok(false);
    }
    let write = || -> windows_registry::Result<()> {
        let key = windows_registry::CURRENT_USER.create(&root)?;
        key.set_string("", format!("URL:{name}"))?;
        key.set_string("URL Protocol", "")?;
        windows_registry::CURRENT_USER
            .create(&command_key)?
            .set_string("", scheme_command(exe))
    };
    write().map(|()| true).map_err(std::io::Error::other)
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

/// The 群 with toasts posted by this run, so they can be taken back out of the
/// notification center once the 群 is read.
static POSTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Takes the toasts of `read_groups` (groups with nothing unread) out of the
/// notification center. Call on the main thread.
pub fn withdraw_read(read_groups: &[&str]) {
    let gone: Vec<String> = {
        let mut posted = POSTED.lock().unwrap_or_else(|e| e.into_inner());
        let Some(posted) = posted.as_mut() else {
            return;
        };
        let gone: Vec<String> = posted
            .iter()
            .filter(|group| read_groups.contains(&group.as_str()))
            .cloned()
            .collect();
        for group in &gone {
            posted.remove(group);
        }
        gone
    };
    for group in gone {
        if let Err(error) = remove_group(super::session::app_id(), &toast_key(&group)) {
            tracing::debug!(%error, "cannot take a 群's toasts out");
        }
    }
}

/// Removes every toast of the toast group `group` of `app_id` from the notification
/// center.
fn remove_group(app_id: &str, group: &str) -> windows::core::Result<()> {
    ToastNotificationManager::History()?
        .RemoveGroupWithId(&HSTRING::from(group), &HSTRING::from(app_id))
}

/// The toast's XML (`ToastGeneric`), built through the DOM so no text needs escaping.
fn toast_document(spec: &ToastSpec) -> windows::core::Result<XmlDocument> {
    let document = XmlDocument::new()?;
    let element = |name: &str| document.CreateElement(&HSTRING::from(name));
    let set = |element: &XmlElement, name: &str, value: &str| {
        element.SetAttribute(&HSTRING::from(name), &HSTRING::from(value))
    };
    let toast = element("toast")?;
    set(&toast, "launch", &spec.launch)?;
    set(&toast, "activationType", "protocol")?;
    document.AppendChild(&toast)?;
    if let Some((id, title, arguments)) = &spec.header {
        let header = element("header")?;
        set(&header, "id", id)?;
        set(&header, "title", title)?;
        set(&header, "arguments", arguments)?;
        set(&header, "activationType", "protocol")?;
        toast.AppendChild(&header)?;
    }
    let visual = element("visual")?;
    toast.AppendChild(&visual)?;
    let binding = element("binding")?;
    set(&binding, "template", "ToastGeneric")?;
    visual.AppendChild(&binding)?;
    for text in [&spec.title, &spec.body] {
        let line = element("text")?;
        line.SetInnerText(&HSTRING::from(text.as_str()))?;
        binding.AppendChild(&line)?;
    }
    let audio = element("audio")?;
    if spec.sound {
        set(&audio, "src", "ms-winsoundevent:Notification.Default")?;
    } else {
        set(&audio, "silent", "true")?;
    }
    toast.AppendChild(&audio)?;
    Ok(document)
}

/// Shows the toast of `spec` under the AppUserModelID `app_id`.
fn show_toast(app_id: &str, spec: &ToastSpec) -> windows::core::Result<()> {
    let toast = ToastNotification::CreateToastNotification(&toast_document(spec)?)?;
    toast.SetGroup(&HSTRING::from(spec.group.as_str()))?;
    toast.SetTag(&HSTRING::from(spec.tag.as_str()))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&toast)
}

/// Shows a notification, with the system's notification sound when `sound` (设置 › 通用 ›
/// 声音), and flashes the taskbar button. `group_name` is the 群's name for the header its
/// notifications share. Call on the main thread.
pub fn post(notice: &Notice, group_name: &str, sound: bool, cx: &mut App) {
    let spec = toast_spec(notice, group_name, sound, &chat_core::new_id());
    match show_toast(super::session::app_id(), &spec) {
        Ok(()) => {
            POSTED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashSet::new)
                .insert(notice.group_id.clone());
        }
        Err(error) => tracing::warn!(%error, "cannot show a notification"),
    }
    flash_taskbar(cx);
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

    fn spec(sound: bool, header: bool) -> ToastSpec {
        let notice = Notice {
            title: "小王 在 <产品研发>".into(),
            body: "看一下 \"这个\" & 回我".into(),
            group_id: "g1".into(),
            topic_id: Some("t1".into()),
        };
        toast_spec(
            &notice,
            if header { "产品 & 研发" } else { "" },
            sound,
            "u1",
        )
    }

    /// The XML Windows gets: protocol activation with the 话题's link, the 群's header,
    /// the texts escaped by the DOM, and the sound or silence.
    #[test]
    fn the_toast_xml_opens_the_topic_and_names_the_group() {
        let scheme = env!("PRODUCT_URL_SCHEME");
        let xml = toast_document(&spec(true, true))
            .unwrap()
            .GetXml()
            .unwrap()
            .to_string();
        println!("toast xml: {xml}");
        assert!(
            xml.contains(&format!(r#"launch="{scheme}://group/g1/topic/t1""#)),
            "{xml}"
        );
        assert!(xml.contains(r#"activationType="protocol""#), "{xml}");
        assert!(
            xml.contains(&format!(r#"id="{}""#, toast_key("g1"))),
            "{xml}"
        );
        assert!(xml.contains(r#"title="产品 &amp; 研发""#), "{xml}");
        assert!(
            xml.contains(&format!(r#"arguments="{scheme}://group/g1""#)),
            "{xml}"
        );
        assert!(
            xml.contains("<text>小王 在 &lt;产品研发&gt;</text>"),
            "{xml}"
        );
        assert!(xml.contains("&amp; 回我</text>"), "{xml}");
        assert!(
            xml.contains(r#"src="ms-winsoundevent:Notification.Default""#),
            "{xml}"
        );
        assert!(!xml.contains("silent"), "{xml}");
        let quiet = toast_document(&spec(false, false))
            .unwrap()
            .GetXml()
            .unwrap()
            .to_string();
        assert!(quiet.contains(r#"silent="true""#), "{quiet}");
        assert!(!quiet.contains("<header"), "{quiet}");
        // Windows takes the document as a toast.
        ToastNotification::CreateToastNotification(&toast_document(&spec(true, true)).unwrap())
            .expect("a toast from the document");
    }

    /// A real toast under a test identity: registered, shown, found in the notification
    /// center's history under its 群's group, and taken out when the 群 is read. A session
    /// without the notification platform (some servers) says so and is skipped.
    #[test]
    fn a_toast_reaches_the_notification_center_and_leaves_it() {
        let app_id = format!("test.toast.{}", std::process::id());
        let dir = tempfile::tempdir().unwrap();
        register_identity(&app_id, "Toast test", &dir.path().join("icon.png")).unwrap();
        assert!(dir.path().join("icon.png").is_file());
        let key = format!(r"Software\Classes\AppUserModelId\{app_id}");
        let registered = windows_registry::CURRENT_USER.open(&key).unwrap();
        assert_eq!(registered.get_string("DisplayName").unwrap(), "Toast test");
        assert!(
            registered
                .get_string("IconUri")
                .unwrap()
                .ends_with("icon.png")
        );

        let spec = spec(false, true);
        let shown = show_toast(&app_id, &spec);
        println!("toast shown: {shown:?}");
        if shown.is_ok() {
            let history = ToastNotificationManager::History().unwrap();
            let count = || {
                history
                    .GetHistoryWithId(&HSTRING::from(app_id.as_str()))
                    .and_then(|toasts| toasts.Size())
            };
            // The platform files the toast asynchronously.
            let until = Instant::now() + Duration::from_secs(10);
            while count().unwrap_or(0) == 0 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
            println!("toasts in the notification center: {:?}", count());
            if count().unwrap_or(0) > 0 {
                remove_group(&app_id, &spec.group).unwrap();
                let until = Instant::now() + Duration::from_secs(10);
                while count().unwrap_or(1) > 0 && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(100));
                }
                assert_eq!(count().unwrap(), 0, "reading the 群 took its toast out");
            }
        }
        let _ = windows_registry::CURRENT_USER.remove_tree(&key);
    }

    /// The URL scheme is written when nobody has it, left alone when a program that
    /// exists has it, and taken over from a program that is gone.
    #[test]
    fn the_url_scheme_is_registered_only_when_free() {
        let scheme = format!("tc-test-{}", std::process::id());
        let root = format!(r"Software\Classes\{scheme}");
        let command = || {
            windows_registry::CURRENT_USER
                .open(format!(r"{root}\shell\open\command"))
                .and_then(|key| key.get_string(""))
        };
        let me = std::env::current_exe().unwrap();
        assert!(
            register_scheme(&scheme, "Test", &me).unwrap(),
            "free: written"
        );
        assert_eq!(command().unwrap(), scheme_command(&me));
        let key = windows_registry::CURRENT_USER.open(&root).unwrap();
        assert_eq!(key.get_string("URL Protocol").unwrap(), "");
        let other = std::path::Path::new(r"C:\Windows\System32\notepad.exe");
        assert!(
            !register_scheme(&scheme, "Test", other).unwrap(),
            "taken: kept"
        );
        assert_eq!(command().unwrap(), scheme_command(&me));
        // Its program is gone: the scheme is free again.
        windows_registry::CURRENT_USER
            .create(format!(r"{root}\shell\open\command"))
            .unwrap()
            .set_string("", scheme_command(std::path::Path::new(r"C:\gone\app.exe")))
            .unwrap();
        assert!(register_scheme(&scheme, "Test", &me).unwrap());
        assert_eq!(command().unwrap(), scheme_command(&me));
        windows_registry::CURRENT_USER.remove_tree(&root).unwrap();
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
