//! What GPUI's Windows platform really does for the commands of the app's 窗口 menu and
//! for the Dock menu, in a real GPUI app on a desktop session (docs/plans/windows-parity/
//! system.zh.md): minimize, maximize and restore, full screen, bringing windows forward,
//! and the taskbar's jump list landing under the app's AppUserModelID.
//!
//! `cargo run -p desktop --example win_window_probe -- <out-dir>` writes
//! `<out-dir>/report.json`. Exit status 0 when every checked command did its work.
//!
//! The unit tests cannot see this: they run on GPUI's test platform, which has no windows.

#[cfg(windows)]
mod probe {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        time::{Duration, Instant, SystemTime},
    };

    use gpui::{
        App, AppContext as _, AsyncApp, Bounds, Context, IntoElement, MenuItem, Render, Styled,
        Window, WindowBounds, WindowOptions, div, point, px, rgb, size,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use serde_json::json;
    use windows::{
        Win32::{
            Foundation::HWND,
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoIncrementMTAUsage},
            UI::{
                Shell::{DestinationList, ICustomDestinationList},
                WindowsAndMessaging::{
                    GetForegroundWindow, IsIconic, IsZoomed, SW_RESTORE, ShowWindow,
                },
            },
        },
        core::HSTRING,
    };

    gpui::actions!(probe, [First, Second]);

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0xffffff))
        }
    }

    fn hwnd(window: &Window) -> HWND {
        match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => HWND(h.hwnd.get() as *mut _),
            _ => HWND::default(),
        }
    }

    async fn settle(cx: &mut AsyncApp) {
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
    }

    /// The folder where the shell keeps every app's custom jump list, one file per
    /// AppUserModelID.
    fn destinations() -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("APPDATA")?)
                .join(r"Microsoft\Windows\Recent\CustomDestinations"),
        )
    }

    fn files(dir: &Path) -> HashMap<PathBuf, SystemTime> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
            .collect()
    }

    /// Whether the file holds `text` as UTF-16 (how a shell link stores its arguments).
    fn holds(path: &Path, text: &str) -> bool {
        let needle: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        std::fs::read(path).is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
    }

    pub fn main() {
        let out = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("win-window-probe"));
        std::fs::create_dir_all(&out).expect("create out dir");
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(120));
            eprintln!("win_window_probe: timed out");
            std::process::exit(2);
        });
        let app_id = format!("probe.window.{}", std::process::id());

        gpui_platform::application().run(move |cx: &mut App| {
            let before = destinations().map(|d| files(&d)).unwrap_or_default();
            // The order the app uses: the identity first, then the Dock menu.
            cx.set_app_identity(&app_id, "Window probe");
            cx.set_dock_menu(vec![
                MenuItem::action("First task", First),
                MenuItem::separator(),
                MenuItem::action("Second task", Second),
            ]);
            let open = |cx: &mut App, x: f32| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(x), px(80.)),
                            size(px(480.), px(320.)),
                        ))),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Host),
                )
                .expect("a window")
            };
            let (first, second) = (open(cx, 80.), open(cx, 240.));
            cx.spawn(async move |cx: &mut AsyncApp| {
                settle(cx).await;
                let h1 = first.update(cx, |_, w, _| hwnd(w)).unwrap();
                let h2 = second.update(cx, |_, w, _| hwnd(w)).unwrap();
                // SAFETY (all blocks below): read-only state queries and ShowWindow on
                // live windows of this thread.
                first.update(cx, |_, w, _| w.minimize_window()).unwrap();
                settle(cx).await;
                let minimized = unsafe { IsIconic(h1).as_bool() };
                unsafe {
                    let _ = ShowWindow(h1, SW_RESTORE);
                }
                settle(cx).await;
                first.update(cx, |_, w, _| w.zoom_window()).unwrap();
                settle(cx).await;
                let zoomed = unsafe { IsZoomed(h1).as_bool() };
                let says_maximized = first.update(cx, |_, w, _| w.is_maximized()).unwrap();
                unsafe {
                    let _ = ShowWindow(h1, SW_RESTORE);
                }
                settle(cx).await;
                let restored = unsafe { !IsZoomed(h1).as_bool() };
                first.update(cx, |_, w, _| w.toggle_fullscreen()).unwrap();
                settle(cx).await;
                let fullscreen = first.update(cx, |_, w, _| w.is_fullscreen()).unwrap();
                first.update(cx, |_, w, _| w.toggle_fullscreen()).unwrap();
                settle(cx).await;
                let windowed = !first.update(cx, |_, w, _| w.is_fullscreen()).unwrap();
                // 前置全部窗口: each window activated in turn, the last one on top.
                first.update(cx, |_, w, _| w.activate_window()).unwrap();
                second.update(cx, |_, w, _| w.activate_window()).unwrap();
                settle(cx).await;
                let front = unsafe { GetForegroundWindow() };
                let second_in_front = front == h2;

                // The jump list: GPUI commits it on a background thread. Its file is new
                // (or newer) in the shell's folder and holds the second task's argument;
                // deleting the list of exactly this app id removes that file, so the list
                // was filed under the id set above and not under a default identity.
                let dir = destinations();
                let find = |before: &HashMap<PathBuf, SystemTime>| {
                    dir.as_ref().and_then(|dir| {
                        files(dir).into_iter().find_map(|(path, modified)| {
                            let fresh = before.get(&path).is_none_or(|old| *old < modified);
                            (fresh && holds(&path, "--dock-action 1")).then_some(path)
                        })
                    })
                };
                // First as a bare GPUI app commits it: its background threads have no COM
                // apartment unless the process has a multithreaded one.
                let mut list_file = None;
                let deadline = Instant::now() + Duration::from_secs(6);
                while list_file.is_none() && Instant::now() < deadline {
                    list_file = find(&before);
                    settle(cx).await;
                }
                let without_apartment = list_file.is_some();
                // Then as the app does it: the process joins the multithreaded apartment
                // before it sets the Dock menu.
                if list_file.is_none() {
                    // SAFETY: keeps the process's multithreaded apartment alive for good.
                    let joined = unsafe { CoIncrementMTAUsage().is_ok() };
                    println!("joined the multithreaded apartment: {joined}");
                    cx.update(|cx| {
                        cx.set_dock_menu(vec![
                            MenuItem::action("First task", First),
                            MenuItem::separator(),
                            MenuItem::action("Second task", Second),
                        ])
                    });
                    let deadline = Instant::now() + Duration::from_secs(15);
                    while list_file.is_none() && Instant::now() < deadline {
                        list_file = find(&before);
                        settle(cx).await;
                    }
                }
                let new_files: Vec<String> = dir
                    .as_ref()
                    .map(|dir| {
                        files(dir)
                            .into_keys()
                            .filter(|path| !before.contains_key(path))
                            .map(|path| path.display().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let deleted = unsafe {
                    CoCreateInstance::<_, ICustomDestinationList>(
                        &DestinationList,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                    .and_then(|list| list.DeleteList(&HSTRING::from(app_id.as_str())))
                    .is_ok()
                };
                settle(cx).await;
                let under_app_id = list_file.as_ref().is_some_and(|f| !f.exists());

                let ok = minimized
                    && second_in_front
                    && zoomed
                    && says_maximized
                    && restored
                    && fullscreen
                    && windowed
                    && list_file.is_some()
                    && deleted
                    && under_app_id;
                let report = json!({
                    "ok": ok,
                    "minimize": minimized,
                    "zoom": zoomed,
                    "is_maximized": says_maximized,
                    "restore": restored,
                    "fullscreen": fullscreen,
                    "leave_fullscreen": windowed,
                    // Activating windows in turn leaves the last one in front.
                    "second_window_in_front": second_in_front,
                    // Reported: whether GPUI's own commit worked before the process had a
                    // multithreaded apartment (the app joins one first).
                    "jump_list_without_apartment": without_apartment,
                    "new_destination_files": new_files,
                    "jump_list_file": list_file.as_ref().map(|f| f.display().to_string()),
                    "jump_list_deleted_by_app_id": deleted,
                    "jump_list_under_app_id": under_app_id,
                });
                println!("{report:#}");
                let _ = std::fs::write(out.join("report.json"), format!("{report:#}\n"));
                std::process::exit(if ok { 0 } else { 1 });
            })
            .detach();
        });
    }
}

fn main() {
    #[cfg(windows)]
    probe::main();
    #[cfg(not(windows))]
    eprintln!("the window probe runs only on Windows");
}
