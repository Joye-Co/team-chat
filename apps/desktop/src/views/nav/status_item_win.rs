//! The notification-area icon (`Shell_NotifyIconW`; GPUI has no API for it): the Windows
//! twin of the macOS menu bar item, from the same unread totals.
//! 设置 › 通用 › 菜单栏里显示 hides or shows it.
//!
//! The brand's template image, tinted white or black for the taskbar's theme, with a dot
//! when something waits on you; the tooltip says what. A click opens the main window. A
//! right click opens the menu the Dock icon has on macOS, plus 设置… and 退出.
//!
//! While the icon is in the notification area the app keeps running after its last window
//! closes, as on macOS (GPUI quits then by default off macOS): the icon opens the window
//! again and its menu quits. With the icon hidden or not added, closing the last window
//! quits, so the app never runs with no way to reach it.
//!
//! The same totals put the unread count on the app's taskbar buttons
//! (`ITaskbarList3::SetOverlayIcon`), as the Dock icon's badge on macOS: a red disc with the
//! count, drawn here ([`super::tray_icon::badge_bgra`]) because Windows takes an icon, not
//! a number. It is set again when a window opens ([`window_attached`]).
//!
//! The icon belongs to the hidden events window ([`crate::win32`]). Its procedure only
//! sends on a channel that a GPUI task drains. The menu is the system's own popup menu; its
//! loop runs in that task between two app updates, as gpui-component's native menu does,
//! so GPUI is not borrowed while the loop dispatches messages.

use futures::{
    StreamExt as _,
    channel::mpsc::{UnboundedSender, unbounded},
};
use gpui::{Action, App, Global};
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        Graphics::Gdi::{CreateBitmap, DeleteObject},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::{
            Shell::{
                ITaskbarList3, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NOTIFYICONDATAW, Shell_NotifyIconW, TaskbarList,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu,
                GetCursorPos, HICON, ICONINFO, MF_SEPARATOR, MF_STRING, PostMessageW,
                RegisterWindowMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU,
                WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_SETTINGCHANGE,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::tray_icon::{BADGE_SIDE, and_mask_len, badge_bgra, badge_text, tinted_bgra};
use crate::{
    app::{Changes, NewTopic, OpenSettings, Quit, Services},
    copy::{fill, nav, settings},
    data::{Change, ChatProvider, SessionState},
    win32::wide,
};

/// The brand's template images (assets/brand/menubar), 32 px; Windows scales them to the
/// notification area's icon size.
const ICON: &[u8] = include_bytes!("../../../../../assets/brand/menubar/StatusItemTemplate@2x.png");
const ICON_WAITING: &[u8] =
    include_bytes!("../../../../../assets/brand/menubar/StatusItemWaitingTemplate@2x.png");

/// The message the icon sends to the events window.
const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

/// What the events window's procedure tells the app.
enum Event {
    /// A click on the icon.
    Open,
    /// A right click: show the menu.
    Menu,
    /// Explorer started again (the icon is gone) or the taskbar's theme may have changed.
    Refresh,
}

/// The rows of the icon's menu; `None` is a separator. A row's command id is its index
/// plus one.
fn menu_rows() -> Vec<Option<(&'static str, Box<dyn Action>)>> {
    vec![
        Some((nav::MENU_SHOW_MAIN, Box::new(super::ShowMainWindow))),
        None,
        Some((nav::MENU_NEW_TOPIC, Box::new(NewTopic))),
        Some((nav::MENU_NEW_GROUP, Box::new(super::NewGroup))),
        Some((nav::MENU_START_DM, Box::new(super::StartDm))),
        None,
        Some((settings::MENU_SETTINGS, Box::new(OpenSettings))),
        Some((settings::MENU_QUIT, Box::new(Quit))),
    ]
}

/// Keeps the icon's state for the life of the app.
struct StatusItem {
    hwnd: isize,
    /// The icon is in the notification area now.
    added: bool,
    /// 菜单栏里显示.
    visible: bool,
    shown: Option<(u32, u32)>,
    tooltip: String,
    icon: Option<isize>,
}

impl Global for StatusItem {}

/// Unread and mention totals over the groups that are not muted.
pub fn totals(provider: &dyn ChatProvider) -> (u32, u32) {
    if !matches!(provider.session(), SessionState::SignedIn { .. }) {
        return (0, 0);
    }
    provider
        .groups()
        .iter()
        .filter(|g| !provider.group_muted(&g.id))
        .fold((0, 0), |(u, m), g| {
            (u.saturating_add(g.unread), m.saturating_add(g.mentions))
        })
}

/// True when the taskbar is light (个性化 › 颜色 › Windows 模式); dark is the default.
fn taskbar_is_light() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|v| v != 0)
}

/// An icon of the template `png` in the taskbar's foreground color.
fn make_icon(png: &[u8], light_taskbar: bool) -> Option<HICON> {
    let (width, height, bgra) = tinted_bgra(png, !light_taskbar)?;
    icon_from_bgra(width, height, &bgra)
}

/// An icon from straight-alpha BGRA rows.
fn icon_from_bgra(width: u32, height: u32, bgra: &[u8]) -> Option<HICON> {
    let mask = vec![0u8; and_mask_len(width, height)];
    // SAFETY: both buffers hold the bytes the bitmaps' sizes ask for; CreateIconIndirect
    // copies the bitmaps, which are deleted right after.
    unsafe {
        let color = CreateBitmap(
            width as i32,
            height as i32,
            1,
            32,
            Some(bgra.as_ptr().cast()),
        );
        let mask = CreateBitmap(
            width as i32,
            height as i32,
            1,
            1,
            Some(mask.as_ptr().cast()),
        );
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// The theme's red as B, G, R: the fill of the taskbar badge.
fn badge_fill(cx: &App) -> [u8; 3] {
    let red = gpui::Rgba::from(crate::ui::native::native_colors(cx).red);
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    [byte(red.b), byte(red.g), byte(red.r)]
}

/// The shell's taskbar object, made on first use. `None` when the shell has no taskbar
/// (no Explorer in this session).
fn taskbar() -> Option<ITaskbarList3> {
    thread_local! {
        static TASKBAR: std::cell::OnceCell<Option<ITaskbarList3>> =
            const { std::cell::OnceCell::new() };
    }
    TASKBAR.with(|t| {
        t.get_or_init(|| {
            // SAFETY: GPUI initialized COM on this (the main) thread; the object is used
            // on this thread only.
            unsafe {
                let list: ITaskbarList3 =
                    CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()?;
                list.HrInit().ok()?;
                Some(list)
            }
        })
        .clone()
    })
}

/// Puts the unread badge on `hwnd`'s taskbar button, or takes it off when nothing is
/// unread. False when the taskbar refused (the window has no button yet).
fn set_overlay(hwnd: HWND, unread: u32, fill: [u8; 3]) -> bool {
    let Some(taskbar) = taskbar() else {
        return false;
    };
    let icon = badge_bgra(unread, fill)
        .and_then(|bgra| icon_from_bgra(BADGE_SIDE, BADGE_SIDE, &bgra));
    // What a screen reader says for the badge.
    let label = wide(&badge_text(unread).unwrap_or_default());
    // SAFETY: `hwnd` is a window of this thread; the taskbar copies the icon, which is
    // destroyed right after; a null icon clears the overlay.
    unsafe {
        let done = taskbar
            .SetOverlayIcon(hwnd, icon.unwrap_or_default(), PCWSTR(label.as_ptr()))
            .is_ok();
        if let Some(icon) = icon {
            let _ = DestroyIcon(icon);
        }
        done
    }
}

/// Puts the current unread badge on the taskbar button of every window of the app.
fn sync_overlays(unread: u32, cx: &mut App) {
    let fill = badge_fill(cx);
    for handle in cx.windows() {
        if let Ok(Some(hwnd)) = handle.update(cx, |_, window, _| crate::win32::hwnd(window)) {
            set_overlay(hwnd, unread, fill);
        }
    }
}

/// A window opened: its taskbar button gets the badge too. The button exists only after
/// the window is shown, so this runs on a later turn.
pub fn window_attached(cx: &mut App) {
    cx.defer(|cx| {
        if let Some(unread) = cx
            .try_global::<StatusItem>()
            .and_then(|s| s.shown)
            .map(|(unread, _)| unread)
        {
            sync_overlays(unread, cx);
        }
    });
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

impl StatusItem {
    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    /// Adds the icon or brings it up to date; takes it out while it is hidden.
    fn sync(&mut self, waiting: bool) {
        if !self.visible {
            self.remove();
            return;
        }
        let Some(icon) = make_icon(
            if waiting { ICON_WAITING } else { ICON },
            taskbar_is_light(),
        ) else {
            return;
        };
        let mut data = icon_data(self.hwnd());
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon;
        // The tooltip holds 127 UTF-16 units and a NUL.
        for (slot, unit) in data
            .szTip
            .iter_mut()
            .zip(self.tooltip.encode_utf16().take(127))
        {
            *slot = unit;
        }
        // SAFETY: `data` is a valid NOTIFYICONDATAW for a window of this thread.
        let done = unsafe {
            if self.added {
                Shell_NotifyIconW(NIM_MODIFY, &data).as_bool()
                    // Explorer lost it (it started again): add it back.
                    || Shell_NotifyIconW(NIM_ADD, &data).as_bool()
            } else {
                Shell_NotifyIconW(NIM_ADD, &data).as_bool()
            }
        };
        self.added = done;
        // The notification area keeps its own copy of the icon.
        if let Some(old) = self.icon.replace(icon.0 as isize) {
            // SAFETY: `old` is an icon this module made and no longer shows.
            unsafe {
                let _ = DestroyIcon(HICON(old as *mut _));
            }
        }
    }

    fn remove(&mut self) {
        if self.added {
            // SAFETY: as in `sync`.
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(self.hwnd()));
            }
            self.added = false;
        }
    }
}

/// Shows the menu at the pointer and returns the chosen row's command id (0 for none).
/// Runs the system's menu loop, so it returns when the menu closes.
fn track_menu(hwnd: HWND) -> usize {
    // SAFETY: the menu is created, shown for a window of this thread and destroyed here;
    // the labels outlive the calls that copy them.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        for (ix, row) in menu_rows().iter().enumerate() {
            let _ = match row {
                Some((label, _)) => {
                    let label = wide(label);
                    AppendMenuW(menu, MF_STRING, ix + 1, PCWSTR(label.as_ptr()))
                }
                None => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
            };
        }
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // Without this the menu stays open when the person clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN).0,
            at.x,
            at.y,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    }
}

fn watch_window(tx: UnboundedSender<Event>) {
    // SAFETY: a static string; the message id is the same for the whole session.
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    crate::win32::on_message(move |_hwnd, msg, _wparam, lparam| {
        let event = if msg == WM_TRAY {
            match lparam.0 as u32 {
                WM_LBUTTONUP => Some(Event::Open),
                WM_RBUTTONUP | WM_CONTEXTMENU => Some(Event::Menu),
                _ => None,
            }
        } else if msg == WM_SETTINGCHANGE || (taskbar_created != 0 && msg == taskbar_created) {
            Some(Event::Refresh)
        } else {
            None
        };
        if let Some(event) = event {
            let _ = tx.unbounded_send(event);
        }
    });
}

pub fn install(services: Services, cx: &mut App) {
    let Some(hwnd) = crate::win32::events_window() else {
        return;
    };
    let (tx, mut rx) = unbounded();
    watch_window(tx);
    cx.set_global(StatusItem {
        hwnd: hwnd.0 as isize,
        added: false,
        visible: crate::views::settings::prefs::get().menu_bar,
        shown: None,
        tooltip: String::new(),
        icon: None,
    });
    update(&services, cx);

    let on_event = services.clone();
    let menu_window = hwnd.0 as isize;
    cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            match event {
                Event::Open => cx.update(super::show_main_window),
                Event::Menu => {
                    // Outside any app update: the menu's loop dispatches messages.
                    let id = track_menu(HWND(menu_window as *mut _));
                    let Some(Some((_, action))) = id
                        .checked_sub(1)
                        .and_then(|ix| menu_rows().into_iter().nth(ix))
                    else {
                        continue;
                    };
                    cx.update(|cx| {
                        if action.partial_eq(&Quit) {
                            cx.dispatch_action(action.as_ref());
                            return;
                        }
                        super::show_main_window(cx);
                        // After the window is shown (that is deferred too). The commands
                        // of the main window (新话题) are handled in it, so dispatch
                        // there; signed out there is none, and the app's handlers get it.
                        cx.defer(move |cx| {
                            let in_window = action.boxed_clone();
                            if !super::with_main_window(cx, |window, cx| {
                                window.dispatch_action(in_window, cx)
                            }) {
                                cx.dispatch_action(action.as_ref());
                            }
                        });
                    });
                }
                Event::Refresh => cx.update(|cx| {
                    cx.global_mut::<StatusItem>().shown = None;
                    update(&on_event, cx);
                }),
            }
        }
    })
    .detach();

    let watched = services.clone();
    cx.subscribe(&services.hub, move |_, changes: &Changes, cx| {
        if changes.any(|c| matches!(c, Change::Groups | Change::Session)) {
            update(&watched, cx);
        }
    })
    .detach();

    // The notification area keeps the icon of a process that ended until the pointer
    // passes over it: take it out on the way.
    cx.on_app_quit(|cx| {
        if let Some(state) = cx.try_global::<StatusItem>() {
            let (hwnd, added) = (state.hwnd(), state.added);
            if added {
                // SAFETY: as in `StatusItem::sync`.
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
                }
            }
        }
        async {}
    })
    .detach();
}

/// Keeps the app alive without windows only while the icon is there to bring it back.
fn sync_quit_mode(cx: &mut App) {
    let added = cx.try_global::<StatusItem>().is_some_and(|s| s.added);
    cx.set_quit_mode(quit_mode(added));
}

fn quit_mode(icon_added: bool) -> gpui::QuitMode {
    if icon_added {
        gpui::QuitMode::Explicit
    } else {
        gpui::QuitMode::LastWindowClosed
    }
}

/// 菜单栏里显示: hides or shows the icon (its state stays, so the totals keep updating).
pub fn set_visible(shown: bool, cx: &mut App) {
    if !cx.has_global::<StatusItem>() {
        return;
    }
    let state = cx.global_mut::<StatusItem>();
    state.visible = shown;
    let waiting = state.shown.is_some_and(|(u, m)| u > 0 || m > 0);
    state.sync(waiting);
    sync_quit_mode(cx);
}

fn update(services: &Services, cx: &mut App) {
    let provider = services.provider.as_ref();
    let (unread, mentions) = totals(provider);
    // A group read here (or on another device) takes its toasts out of the
    // notification center.
    let groups = provider.groups();
    let read: Vec<&str> = groups
        .iter()
        .filter(|g| g.unread == 0 && g.mentions == 0)
        .map(|g| g.id.as_str())
        .collect();
    crate::provider::windows::withdraw_read(&read, cx);
    let state = cx.global_mut::<StatusItem>();
    if state.shown == Some((unread, mentions)) {
        return;
    }
    state.shown = Some((unread, mentions));
    state.tooltip = if mentions > 0 {
        fill(nav::STATUS_ITEM_MENTIONS, &[("n", &mentions.to_string())])
    } else if unread > 0 {
        fill(nav::STATUS_ITEM_UNREAD, &[("n", &unread.to_string())])
    } else {
        nav::STATUS_ITEM_NONE.to_string()
    };
    // The dot says that something waits; the tooltip says what.
    state.sync(unread > 0 || mentions > 0);
    sync_quit_mode(cx);
    sync_overlays(unread, cx);
}

/// For the self-test: the icon's state matches the totals, and it is in the notification
/// area unless it is hidden.
pub fn check(services: &Services, cx: &App) -> bool {
    let Some(state) = cx.try_global::<StatusItem>() else {
        return false;
    };
    state.shown == Some(totals(services.provider.as_ref())) && state.added == state.visible
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_opens_the_window_and_can_quit() {
        let rows = menu_rows();
        let labels: Vec<&str> = rows.iter().flatten().map(|(label, _)| *label).collect();
        assert_eq!(labels.first(), Some(&nav::MENU_SHOW_MAIN));
        assert_eq!(labels.last(), Some(&settings::MENU_QUIT));
        assert!(rows.last().unwrap().as_ref().unwrap().1.partial_eq(&Quit));
    }

    /// Windows makes an icon from both template images, for a dark and a light taskbar.
    #[test]
    fn the_template_images_become_icons() {
        for png in [ICON, ICON_WAITING] {
            for light in [false, true] {
                let icon = make_icon(png, light).expect("an icon");
                // SAFETY: the icon was made just above and is not shown.
                unsafe { DestroyIcon(icon).unwrap() };
            }
        }
    }

    /// Windows makes an icon from the badge of every size of count, and the taskbar takes
    /// it for a window that has a taskbar button (and takes it off again).
    #[test]
    fn the_taskbar_takes_the_unread_badge() {
        use windows::Win32::{
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
                WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        };
        let fill = [0x30, 0x3b, 0xff];
        for unread in [1, 42, 250] {
            let bgra = badge_bgra(unread, fill).unwrap();
            let icon = icon_from_bgra(BADGE_SIDE, BADGE_SIDE, &bgra).expect("a badge icon");
            // SAFETY: the icon was made just above and is not shown.
            unsafe { DestroyIcon(icon).unwrap() };
        }
        // SAFETY: a plain visible window of this thread, destroyed below; the message
        // loop runs long enough for the shell to give it a taskbar button.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("badge test"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                40,
                40,
                320,
                200,
                None,
                None,
                None,
                None,
            )
            .expect("a window");
            let pump = || {
                let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
                let mut msg = MSG::default();
                while std::time::Instant::now() < until {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            };
            pump();
            // A session without Explorer has no taskbar: nothing to check there.
            if taskbar().is_some() {
                let set = set_overlay(hwnd, 7, fill);
                pump();
                let cleared = set_overlay(hwnd, 0, fill);
                println!("taskbar overlay: set {set}, cleared {cleared}");
                assert!(set && cleared, "the taskbar took and cleared the badge");
            } else {
                println!("taskbar overlay: no taskbar in this session");
            }
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn the_app_outlives_its_windows_only_with_the_icon() {
        assert_eq!(quit_mode(true), gpui::QuitMode::Explicit);
        assert_eq!(quit_mode(false), gpui::QuitMode::LastWindowClosed);
    }

    #[test]
    fn the_taskbar_theme_can_be_read() {
        let _ = taskbar_is_light();
    }
}
