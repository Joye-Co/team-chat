//! The daemon as a scheduled task on Windows, against the real Task Scheduler (CI registers
//! the task for the runner's own user): it is registered and started, runs with a console and
//! without a second instance, is started again after it was killed, moves to a new program
//! file by the replace protocol of `daemon::selfupdate`, and is gone after uninstall.
//!
//! This test program is its own stand-in daemon (`harness = false`): the task starts a copy of
//! it as `<copy> run --config <file> --log-file <file>`, the arguments of the real daemon.
//! Nothing here runs on Unix, where launchd and systemd are the service managers.

#[cfg(not(windows))]
fn main() {
    println!("service_task: the scheduled task is Windows only, nothing to run here");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("run") {
        windows::stand_in(&args[2..]);
    }
    println!("\nrunning 1 test");
    windows::the_task_keeps_the_daemon_running();
    println!("test the_task_keeps_the_daemon_running ... ok");
    println!("\ntest result: ok. 1 passed; 0 failed");
}

#[cfg(windows)]
mod windows {
    use daemon::selfupdate::{self, Watch};
    use daemon::service;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// The stand-in ends by itself after this long, so a failed test leaves nothing behind.
    const LIFE: Duration = Duration::from_secs(900);
    /// The task asks for a start every minute: a start is at most this far away.
    const NEXT_START: Duration = Duration::from_secs(150);

    fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
        let at = args.iter().position(|a| a == name)?;
        args.get(at + 1).map(String::as_str)
    }

    fn state_file(config: &Path) -> PathBuf {
        config.with_extension("state")
    }

    /// The stand-in daemon: says who it is in the state file next to its config, then waits
    /// for a new program file the way the daemon does and exits when there is one.
    pub fn stand_in(args: &[String]) -> ! {
        let config = PathBuf::from(arg(args, "--config").expect("--config"));
        let log = arg(args, "--log-file").expect("--log-file");
        service::log_to(Path::new(log)).expect("log file");
        // Whether this process has a console (the headless console host's).
        let mut list = [0u32; 4];
        // SAFETY: the buffer has room for the count passed.
        let on_console = unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(list.as_mut_ptr(), 4)
        } > 0;
        // The daemon's start: the files an update moved aside are deleted.
        let mut watch = Watch::current().expect("own program file");
        let exe = std::env::current_exe().unwrap();
        let state = format!(
            "{} console={} exe={}",
            std::process::id(),
            u8::from(on_console),
            exe.display()
        );
        let tmp = config.with_extension("state.new");
        std::fs::write(&tmp, &state).unwrap();
        std::fs::rename(&tmp, state_file(&config)).unwrap();
        eprintln!("stand-in started: {state}");
        let start = Instant::now();
        while start.elapsed() < LIFE {
            std::thread::sleep(Duration::from_millis(500));
            if watch.check() {
                eprintln!("a new version is installed; restarting");
                std::process::exit(0);
            }
        }
        std::process::exit(0);
    }

    struct Running {
        pid: u32,
        console: bool,
    }

    fn read_state(config: &Path) -> Option<Running> {
        let text = std::fs::read_to_string(state_file(config)).ok()?;
        let mut parts = text.split(' ');
        let pid = parts.next()?.parse().ok()?;
        let console = parts.next()? == "console=1";
        Some(Running { pid, console })
    }

    /// Waits for a stand-in other than `not` to say it runs.
    fn wait_running(config: &Path, not: Option<u32>, limit: Duration, what: &str) -> Running {
        let start = Instant::now();
        loop {
            if let Some(r) = read_state(config)
                && Some(r.pid) != not
                && daemon::winjob::pid_alive(r.pid)
            {
                println!("  {what}: pid {} after {:?}", r.pid, start.elapsed());
                return r;
            }
            assert!(
                start.elapsed() < limit,
                "{what}: no stand-in within {limit:?}\nlog:\n{}",
                std::fs::read_to_string(service::task_log(config.parent().unwrap()))
                    .unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn wait_until(limit: Duration, what: &str, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(start.elapsed() < limit, "{what}: not within {limit:?}");
            std::thread::sleep(Duration::from_millis(250));
        }
        println!("  {what} after {:?}", start.elapsed());
    }

    fn schtasks(args: &[&str]) -> bool {
        Command::new("schtasks")
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Removes the task when the test fails half way.
    struct Cleanup {
        name: String,
        dir: PathBuf,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = service::uninstall_task(&self.name, &self.dir, true);
        }
    }

    pub fn the_task_keeps_the_daemon_running() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("with space & more");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("stand-in.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let config = dir.join("daemon.toml");
        std::fs::write(&config, "").unwrap();
        let name = format!("{}.test-{}", service::task_name(), std::process::id());
        let cleanup = Cleanup {
            name: name.clone(),
            dir: dir.clone(),
        };

        // Installed: registered, and started right away without a logon.
        let file = service::install_task(&name, &exe, &config, &dir, true).expect("install");
        assert!(file.is_file());
        assert!(service::task_exists(&name));
        let first = wait_running(&config, None, Duration::from_secs(60), "started");
        assert!(first.console, "the daemon has a console (a headless one)");

        // One instance: asking for another start changes nothing.
        assert!(schtasks(&["/Run", "/TN", &name]));
        std::thread::sleep(Duration::from_secs(5));
        assert_eq!(read_state(&config).map(|r| r.pid), Some(first.pid));

        // Killed: the task starts it again.
        assert!(
            Command::new("taskkill")
                .args(["/F", "/PID", &first.pid.to_string()])
                .output()
                .is_ok_and(|o| o.status.success())
        );
        let second = wait_running(&config, Some(first.pid), NEXT_START, "started after a kill");

        // A new version by the replace protocol: the running file is renamed, never written.
        let new = dir.join("stand-in.exe.new");
        std::fs::copy(std::env::current_exe().unwrap(), &new).unwrap();
        let aside = dir.join(format!("stand-in.exe{}1", selfupdate::STALE_MARK));
        std::fs::rename(&exe, &aside).expect("a running program file can be renamed");
        std::fs::rename(&new, &exe).unwrap();
        assert!(daemon::winjob::pid_alive(second.pid));
        // The stand-in sees the new file, exits, and the task starts the new file, which
        // deletes the old one.
        let third = wait_running(
            &config,
            Some(second.pid),
            NEXT_START + Duration::from_secs(30),
            "started on the new version",
        );
        wait_until(Duration::from_secs(30), "the old file is deleted", || {
            !aside.exists()
        });

        // Uninstalled: no task, no process.
        service::uninstall_task(&name, &dir, true).expect("uninstall");
        assert!(!service::task_exists(&name));
        assert!(!file.exists());
        wait_until(Duration::from_secs(30), "the daemon ended", || {
            !daemon::winjob::pid_alive(third.pid)
        });
        // Nothing starts it again.
        std::thread::sleep(Duration::from_secs(75));
        assert_eq!(read_state(&config).map(|r| r.pid), Some(third.pid));
        drop(cleanup);
    }
}
