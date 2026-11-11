//! One running app, and what a second launch asks of it.
//!
//! macOS runs one instance of an app: opening it again, a link of its URL scheme, or a
//! click on its Dock icon all reach the running app. Windows starts a new process each
//! time: from the Start menu, for a `<scheme>://` link (`"<exe>" "%1"`), for a clicked
//! notification (its toast launches a link, see [`super::windows`]) and for a task of the
//! taskbar's jump list (`<exe> --dock-action <n>`, GPUI's form of the Dock menu). So on
//! Windows the first process holds a named mutex and listens on a hidden window; a later
//! process finds that window, hands it its [`Command`] (`WM_COPYDATA`), lets it take the
//! foreground and exits. A first process runs its own command once its window is open.
//!
//! Only a plain launch takes part: a process started with any other argument (a
//! self-test, `--fake`, a sandbox) runs on its own.

use gpui::App;

use crate::app::Services;

/// What a launch asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Show the main window (a plain launch).
    Show,
    /// Open what a `<scheme>://` link names.
    Link(String),
    /// Run a command of the Dock menu (a task of the taskbar's jump list), by its place
    /// among the menu's commands.
    DockAction(usize),
}

fn scheme_prefix() -> &'static str {
    concat!(env!("PRODUCT_URL_SCHEME"), ":")
}

/// True for a `<scheme>:` link.
pub fn is_link(arg: &str) -> bool {
    arg.len() > scheme_prefix().len()
        && arg[..scheme_prefix().len()].eq_ignore_ascii_case(scheme_prefix())
}

impl Command {
    /// The command of a launch with these arguments. `None` when the process was started
    /// for something else (any other argument), which never joins the running app.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<Self> {
        let mut command = Command::Show;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if is_link(&arg) {
                command = Command::Link(arg);
            } else if arg == "--dock-action" {
                command = Command::DockAction(args.next()?.parse().ok()?);
            } else {
                return None;
            }
        }
        Some(command)
    }

    /// The command as the bytes one process sends to the other.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn encode(&self) -> String {
        match self {
            Command::Show => "show".into(),
            Command::Link(link) => format!("link {link}"),
            Command::DockAction(n) => format!("dock {n}"),
        }
    }

    /// Reads [`Command::encode`]. Anything else is nothing: the window that receives it
    /// can be sent bytes by any process of the session.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn decode(text: &str) -> Option<Self> {
        if text == "show" {
            return Some(Command::Show);
        }
        if let Some(link) = text.strip_prefix("link ") {
            return (is_link(link) && link.len() <= MAX_LINK).then(|| Command::Link(link.into()));
        }
        text.strip_prefix("dock ")?
            .parse()
            .ok()
            .map(Command::DockAction)
    }
}

/// The longest link the app reads.
const MAX_LINK: usize = 2048;

/// Where a link goes: a 群 (its 群聊) or one of its 话题.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
    pub group_id: String,
    pub topic_id: Option<String>,
}

/// One id of a link's path, percent-encoded so any id is a single path segment.
fn encode_segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let id = String::from_utf8(out).ok()?;
    (!id.is_empty() && !id.chars().any(char::is_control)).then_some(id)
}

/// The link of a 群 or one of its 话题: `<scheme>://group/<g>[/topic/<t>]`, the start of
/// a message link ([`crate::views::timeline::menu::message_link`]).
pub fn link_to(group_id: &str, topic_id: Option<&str>) -> String {
    let mut link = format!(
        "{}://group/{}",
        env!("PRODUCT_URL_SCHEME"),
        encode_segment(group_id)
    );
    if let Some(topic) = topic_id.filter(|t| !t.is_empty()) {
        link.push_str("/topic/");
        link.push_str(&encode_segment(topic));
    }
    link
}

/// The 群 and 话题 a link names; `None` for any other link (the app then only comes to
/// the front). What follows the 话题 (`/message/<m>`) is not read.
pub fn parse_link(link: &str) -> Option<LinkTarget> {
    if !is_link(link) || link.len() > MAX_LINK {
        return None;
    }
    let rest = link[scheme_prefix().len()..].strip_prefix("//")?;
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.split('/');
    if parts.next()? != "group" {
        return None;
    }
    let group_id = decode_segment(parts.next()?)?;
    let topic_id = match parts.next() {
        Some("topic") => Some(decode_segment(parts.next()?)?),
        _ => None,
    };
    Some(LinkTarget { group_id, topic_id })
}

/// Does what `command` asks, in the running app.
pub fn run(command: Command, services: &Services, cx: &mut App) {
    match command {
        Command::Show => crate::views::nav::show_main_window(cx),
        Command::Link(link) => {
            crate::views::nav::show_main_window(cx);
            let Some(target) = parse_link(&link) else {
                return;
            };
            // Only a 群 this account has: a link is text anyone can write.
            let provider = &services.provider;
            if !provider.groups().iter().any(|g| g.id == target.group_id) {
                return;
            }
            let topic = target
                .topic_id
                .filter(|t| provider.topic(&target.group_id, t).is_some());
            services.nav.update(cx, |nav, cx| match topic {
                Some(topic) => nav.open_topic(target.group_id, topic, cx),
                None => nav.open_group_chat(target.group_id, cx),
            });
        }
        Command::DockAction(index) => crate::views::nav::menus::run_dock_action(index, cx),
    }
}

/// Starts listening for later launches (Windows) and runs this launch's own command when
/// it is more than "show" (the window is opening already). Call once, after the first
/// window is asked for.
pub fn install(launch: Option<Command>, services: Services, cx: &mut App) {
    #[cfg(target_os = "windows")]
    win::listen(services.clone(), cx);
    match launch {
        Some(Command::Show) | None => {}
        Some(command) => {
            // After the window that is opening now exists.
            cx.defer(move |cx| run(command, &services, cx));
        }
    }
}

#[cfg(target_os = "windows")]
pub use win::{Claim, claim};

#[cfg(target_os = "windows")]
mod win {
    use std::time::{Duration, Instant};

    use futures::{StreamExt as _, channel::mpsc::unbounded};
    use gpui::App;
    use windows::{
        Win32::{
            Foundation::{
                CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT,
                WPARAM,
            },
            System::{
                DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW,
                Threading::CreateMutexW,
            },
            UI::WindowsAndMessaging::{
                AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowW,
                GetWindowThreadProcessId, RegisterClassW, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
                WINDOW_EX_STYLE, WM_COPYDATA, WNDCLASSW, WS_OVERLAPPED,
            },
        },
        core::{PCWSTR, w},
    };

    use super::Command;
    use crate::{app::Services, win32::wide};

    /// Marks a `WM_COPYDATA` as a launch command of this app.
    const COPY_TAG: usize = 0x5443_4C4E;
    const CLASS: PCWSTR = w!("DesktopInstance");

    /// How long a later launch waits for the first one's window (it may still be
    /// starting).
    const FIND_FOR: Duration = Duration::from_secs(8);

    /// The names of this build's mutex and window: one per app id, so a debug build and
    /// the installed app do not take each other for the same app.
    fn names(app_id: &str) -> (Vec<u16>, Vec<u16>) {
        (
            wide(&format!("Local\\{app_id}.instance")),
            wide(&format!("{app_id}.instance")),
        )
    }

    /// Held by the first process for its whole life.
    pub struct Instance(HANDLE);

    impl Drop for Instance {
        fn drop(&mut self) {
            // SAFETY: the handle is the mutex this process created.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub enum Claim {
        /// This is the app's only process.
        First(Instance),
        /// Another process runs the app and has the command now: exit.
        Forwarded,
    }

    /// Becomes the app's one process, or hands `command` to the process that is. Call
    /// before GPUI starts.
    pub fn claim(command: &Command) -> Claim {
        claim_as(super::super::session::app_id(), command)
    }

    pub fn claim_as(app_id: &str, command: &Command) -> Claim {
        let (mutex, window) = names(app_id);
        // SAFETY: the name is NUL-terminated; the error is read right after the call.
        let (handle, exists) = unsafe {
            let handle = CreateMutexW(None, false, PCWSTR(mutex.as_ptr()));
            (handle, GetLastError() == ERROR_ALREADY_EXISTS)
        };
        let Ok(handle) = handle else {
            // No mutex, no way to tell: run as the only process.
            return Claim::First(Instance(HANDLE::default()));
        };
        let instance = Instance(handle);
        if !exists {
            return Claim::First(instance);
        }
        let until = Instant::now() + FIND_FOR;
        loop {
            if send(&window, command) {
                return Claim::Forwarded;
            }
            if Instant::now() >= until {
                // The other process never showed up (it is exiting, or hung before its
                // window): better two apps than none.
                tracing::warn!("the running app did not answer; starting another");
                return Claim::First(instance);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Sends `command` to the window named `window`. False when there is no such window
    /// or it did not take the command.
    fn send(window: &[u16], command: &Command) -> bool {
        let text = command.encode();
        let data = COPYDATASTRUCT {
            dwData: COPY_TAG,
            cbData: text.len() as u32,
            lpData: text.as_ptr() as *mut _,
        };
        // SAFETY: the names are NUL-terminated; `data` and `text` outlive the synchronous
        // send; the target copies the bytes before it returns.
        unsafe {
            let Ok(hwnd) = FindWindowW(CLASS, PCWSTR(window.as_ptr())) else {
                return false;
            };
            // The person asked for the app by starting it again: let the running process
            // come to the front (only the foreground process can pass that right on).
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let _ = AllowSetForegroundWindow(pid);
            let mut answer = 0usize;
            let sent = SendMessageTimeoutW(
                hwnd,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&data as *const _ as isize),
                SMTO_ABORTIFHUNG,
                5000,
                Some(&mut answer),
            );
            sent.0 != 0 && answer == 1
        }
    }

    thread_local! {
        static COMMANDS: std::cell::RefCell<Option<futures::channel::mpsc::UnboundedSender<Command>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_COPYDATA {
            // SAFETY: for WM_COPYDATA the system passes a valid COPYDATASTRUCT whose
            // buffer holds `cbData` bytes for the length of this call.
            let command = unsafe {
                let data = &*(lparam.0 as *const COPYDATASTRUCT);
                (data.dwData == COPY_TAG && !data.lpData.is_null() && data.cbData <= 4096)
                    .then(|| {
                        std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize)
                    })
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(Command::decode)
            };
            let taken = command.is_some_and(|command| {
                COMMANDS.with_borrow(|tx| {
                    tx.as_ref()
                        .is_some_and(|tx| tx.unbounded_send(command).is_ok())
                })
            });
            return LRESULT(taken as isize);
        }
        // SAFETY: the arguments are the ones this procedure was called with.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Creates the hidden window later launches look for. Main thread.
    fn create_window(app_id: &str) -> Option<HWND> {
        let (_, title) = names(app_id);
        // SAFETY: the class and its procedure live for the whole process; the window is
        // created and used on this thread only.
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()
        }
    }

    /// The commands later launches send to this thread's window, as a stream.
    fn commands(app_id: &str) -> Option<futures::channel::mpsc::UnboundedReceiver<Command>> {
        let (tx, rx) = unbounded();
        COMMANDS.with_borrow_mut(|slot| *slot = Some(tx));
        create_window(app_id)?;
        Some(rx)
    }

    /// Runs what later launches ask for. Call once on the main thread.
    pub fn listen(services: Services, cx: &mut App) {
        let Some(mut rx) = commands(super::super::session::app_id()) else {
            return;
        };
        cx.spawn(async move |cx| {
            while let Some(command) = rx.next().await {
                cx.update(|cx| super::run(command, &services, cx));
            }
        })
        .detach();
    }

    #[cfg(test)]
    mod tests {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
        };

        use super::*;

        /// Two real claims of one app id: the first is the app, the second hands over its
        /// link (a clicked notification) through the hidden window and is told to exit.
        #[test]
        fn a_second_launch_hands_its_command_to_the_first() {
            let app_id = format!("test.instance.{}", std::process::id());
            let first = claim_as(&app_id, &Command::Show);
            assert!(matches!(first, Claim::First(_)), "nobody runs it yet");
            let mut rx = commands(&app_id).expect("the instance window");
            let link = super::super::link_to("g 1", Some("话题/2"));
            let second = std::thread::spawn({
                let (app_id, link) = (app_id.clone(), link.clone());
                move || {
                    let a = matches!(claim_as(&app_id, &Command::Link(link)), Claim::Forwarded);
                    let b = matches!(claim_as(&app_id, &Command::DockAction(2)), Claim::Forwarded);
                    let c = matches!(claim_as(&app_id, &Command::Show), Claim::Forwarded);
                    (a, b, c)
                }
            });
            // The first process's message loop.
            let until = Instant::now() + Duration::from_secs(20);
            let mut got = Vec::new();
            while got.len() < 3 && Instant::now() < until {
                let mut msg = MSG::default();
                // SAFETY: a plain message pump on this thread.
                unsafe {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                }
                while let Ok(command) = rx.try_recv() {
                    got.push(command);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(second.join().unwrap(), (true, true, true));
            assert_eq!(
                got,
                vec![Command::Link(link), Command::DockAction(2), Command::Show]
            );
            // Once the first is gone, the next launch is the app again.
            drop(first);
            assert!(matches!(
                claim_as(&format!("{app_id}.other"), &Command::Show),
                Claim::First(_)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Option<Command> {
        Command::from_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn a_launch_says_what_it_wants() {
        let link = concat!(env!("PRODUCT_URL_SCHEME"), "://group/g1/topic/t1");
        assert_eq!(args(&[]), Some(Command::Show));
        assert_eq!(args(&[link]), Some(Command::Link(link.into())));
        assert_eq!(args(&["--dock-action", "2"]), Some(Command::DockAction(2)));
        // A self-test or a sandbox never joins the running app.
        assert_eq!(args(&["--fake"]), None);
        assert_eq!(args(&["--screenshot", "out"]), None);
        assert_eq!(args(&["--dock-action"]), None);
        assert_eq!(args(&["--dock-action", "x"]), None);
        assert_eq!(args(&["https://example.com"]), None);
    }

    #[test]
    fn commands_survive_the_trip_between_processes() {
        let link = link_to("g1", Some("t1"));
        for command in [
            Command::Show,
            Command::Link(link),
            Command::DockAction(0),
            Command::DockAction(3),
        ] {
            assert_eq!(Command::decode(&command.encode()), Some(command));
        }
        for junk in [
            "",
            "quit",
            "link https://example.com",
            "dock -1",
            "dock x",
            "show ",
        ] {
            assert_eq!(Command::decode(junk), None, "{junk:?}");
        }
        let long = format!("link {}//{}", scheme_prefix(), "a".repeat(MAX_LINK));
        assert_eq!(Command::decode(&long), None);
    }

    #[test]
    fn links_name_a_group_and_a_topic() {
        let scheme = env!("PRODUCT_URL_SCHEME");
        let target = |link: &str| parse_link(link).map(|t| (t.group_id, t.topic_id));
        assert_eq!(
            target(&format!("{scheme}://group/g1/topic/t1")),
            Some(("g1".into(), Some("t1".into())))
        );
        // A message link opens its 话题.
        assert_eq!(
            target(&format!("{scheme}://group/g1/topic/t1/message/m9")),
            Some(("g1".into(), Some("t1".into())))
        );
        assert_eq!(
            target(&format!("{scheme}://group/g1")),
            Some(("g1".into(), None))
        );
        assert_eq!(
            target(&format!(
                "{}://group/g1/topic/t1?x=1#y",
                scheme.to_uppercase()
            )),
            Some(("g1".into(), Some("t1".into())))
        );
        for bad in [
            format!("{scheme}://"),
            format!("{scheme}://group/"),
            format!("{scheme}://group/g1/topic/"),
            format!("{scheme}://settings"),
            format!("{scheme}:group/g1"),
            format!("{scheme}://group/%zz"),
            format!("{scheme}://group/a%0Ab"),
            "https://group/g1".to_string(),
        ] {
            assert_eq!(target(&bad), None, "{bad}");
        }
    }

    #[test]
    fn any_id_makes_one_path_segment() {
        let link = link_to("g 1/x", Some("话题?#2"));
        assert!(!link.contains(' ') && !link.contains('?') && !link.contains('#'));
        let target = parse_link(&link).unwrap();
        assert_eq!(target.group_id, "g 1/x");
        assert_eq!(target.topic_id.as_deref(), Some("话题?#2"));
        assert_eq!(
            link_to("g1", None),
            link_to("g1", Some("")),
            "no 话题 is the 群聊"
        );
    }
}
