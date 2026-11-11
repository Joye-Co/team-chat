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
    // The task's first action, as the daemon does it.
    if args.get(1).map(String::as_str) == Some("repair-program") {
        daemon::selfupdate::repair(std::path::Path::new(&args[2]));
        return;
    }
    if args.get(1).map(String::as_str) == Some("run") {
        windows::stand_in(&args[2..]);
    }
    println!("\nrunning 4 tests");
    windows::a_program_others_can_replace_is_refused();
    println!("test a_program_others_can_replace_is_refused ... ok");
    windows::the_task_keeps_the_daemon_running();
    println!("test the_task_keeps_the_daemon_running ... ok");
    windows::a_second_daemon_of_the_config_does_not_run();
    println!("test a_second_daemon_of_the_config_does_not_run ... ok");
    windows::a_failed_install_leaves_nothing();
    println!("test a_failed_install_leaves_nothing ... ok");
    println!("\ntest result: ok. 4 passed; 0 failed");
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
        // As the daemon: the only one of this config, or none. A stand-in that is not says so
        // in the log and in a file the test counts, and ends.
        let _instance = match service::single_instance(&config) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("stand-in {} not started: {e}", std::process::id());
                let refused = config.with_extension("refused");
                let n = std::fs::read_to_string(&refused)
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                let _ = std::fs::write(&refused, (n + 1).to_string());
                std::process::exit(2);
            }
        };
        service::stoppable(Path::new(log)).expect("stop event and pid file");
        // A stand-in that is told to takes its time to stop, like a daemon with a run.
        let slow = config.with_extension("slow-stop").exists();
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
        // Asked to stop (uninstall, restart): the daemon shuts down.
        std::thread::spawn(move || {
            service::wait_stop();
            eprintln!("asked to stop");
            if slow {
                std::thread::sleep(Duration::from_secs(8));
            }
            std::process::exit(0);
        });
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

    /// True where the user of this test has no logon session of its own (CI starts the test
    /// as a standard user with other credentials). A task of the daemon runs only in its
    /// user's logon session, so there the Task Scheduler registers it and never starts it.
    fn no_logon_session() -> bool {
        std::env::var_os("TC_NO_LOGON_SESSION").is_some()
    }

    /// The Task Scheduler's part, played by the test where the user has no logon session:
    /// while the task is registered and no instance of it runs, run its actions in order
    /// (the first to its end, the last stays), read from the definition that was registered.
    /// The same rule as the task's triggers with "one instance only", once a second instead
    /// of once a minute. It ends with the test.
    struct Scheduler {
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Scheduler {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// (program, arguments) of each action of the task definition in `file`.
    fn actions(file: &Path) -> Vec<(String, String)> {
        let bytes = std::fs::read(file).unwrap();
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        // The declaration names UTF-16; the text here is already decoded.
        let text = text.replacen("encoding=\"UTF-16\"", "", 1);
        let doc = roxmltree::Document::parse(&text).expect("the task definition is XML");
        doc.descendants()
            .filter(|n| n.tag_name().name() == "Exec")
            .map(|exec| {
                let part = |name: &str| {
                    exec.children()
                        .find(|c| c.tag_name().name() == name)
                        .and_then(|c| c.text())
                        .unwrap_or_default()
                        .to_string()
                };
                (part("Command"), part("Arguments"))
            })
            .collect()
    }

    fn play_scheduler(name: &str, file: &Path) -> Option<Scheduler> {
        use std::os::windows::process::CommandExt as _;
        if !no_logon_session() {
            return None;
        }
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (stopped, name, file) = (stop.clone(), name.to_string(), file.to_path_buf());
        let thread = std::thread::spawn(move || {
            let mut running: Option<std::process::Child> = None;
            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                if running
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
                {
                    continue;
                }
                running = None;
                if !service::task_exists(&name) || !file.is_file() {
                    continue;
                }
                let actions = actions(&file);
                for (i, (program, arguments)) in actions.iter().enumerate() {
                    let mut cmd = Command::new(program);
                    cmd.raw_arg(arguments);
                    if i + 1 < actions.len() {
                        let _ = cmd.status();
                    } else {
                        running = cmd.spawn().ok();
                    }
                }
            }
            if let Some(mut c) = running {
                let _ = c.kill();
                let _ = c.wait();
            }
        });
        Some(Scheduler {
            stop,
            thread: Some(thread),
        })
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

    /// The pids of the running programs named `image`.
    fn pids_of(image: &str) -> Vec<u32> {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {image}"), "/FO", "CSV", "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split(',').nth(1)?.trim_matches('"').parse().ok())
            .collect()
    }

    /// One daemon per user and config: a second one started by hand with the same config
    /// (as the Task Scheduler can, while the first shuts down) ends at once and leaves the
    /// first one's pid file alone; with another config it runs.
    pub fn a_second_daemon_of_the_config_does_not_run() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("stand-in-2.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let start = |dir: &str| {
            let dir = tmp.path().join(dir);
            std::fs::create_dir_all(&dir).unwrap();
            let config = dir.join("daemon.toml");
            if !config.exists() {
                std::fs::write(&config, "").unwrap();
            }
            let child = Command::new(&exe)
                .args(["run", "--config"])
                .arg(&config)
                .arg("--log-file")
                .arg(dir.join("daemon.log"))
                .spawn()
                .unwrap();
            (child, config, dir)
        };
        let (mut first, config, dir) = start("a");
        let running = wait_running(&config, None, Duration::from_secs(30), "first");
        assert_eq!(running.pid, first.id());
        let pid_file = std::fs::read_to_string(service::task_pid_file(&dir)).unwrap();

        let (mut second, _, _) = start("a");
        let code = second.wait().unwrap().code();
        assert_eq!(
            code,
            Some(2),
            "the second daemon of the config ended at once"
        );
        assert_eq!(
            std::fs::read_to_string(config.with_extension("refused")).unwrap(),
            "1"
        );
        assert_eq!(
            std::fs::read_to_string(service::task_pid_file(&dir)).unwrap(),
            pid_file,
            "the first daemon's pid file is as it was"
        );
        assert!(daemon::winjob::pid_alive(first.id()));

        // Another config is another daemon.
        let (mut other, other_config, other_dir) = start("b");
        wait_running(
            &other_config,
            None,
            Duration::from_secs(30),
            "another config",
        );

        // Stopping the first by its pid file leaves the other; and once the first is gone
        // its config can be run again.
        assert_eq!(service::stop_daemon(&dir), Ok(true));
        let _ = first.wait();
        assert!(daemon::winjob::pid_alive(other.id()));
        let (mut again, _, _) = start("a");
        wait_running(&config, Some(running.pid), Duration::from_secs(30), "again");
        for (child, dir) in [(&mut again, &dir), (&mut other, &other_dir)] {
            assert_eq!(service::stop_daemon(dir), Ok(true));
            let _ = child.wait();
        }
    }

    /// An install whose task cannot be registered leaves nothing: no definition file (the
    /// app reads it as "this computer is set up"), no repair copy, no folder.
    pub fn a_failed_install_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("state");
        let exe = tmp.path().join("stand-in-3.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let config = tmp.path().join("daemon.toml");
        std::fs::write(&config, "").unwrap();
        // The Task Scheduler takes no task of this name (a `*` cannot be in one).
        let name = format!("{}.bad*name-{}", service::task_name(), std::process::id());
        let err = service::install_task(&name, &exe, &config, &dir, true)
            .expect_err("a task with this name cannot be registered");
        println!("  refused: {err}");
        assert!(!service::task_exists(&name));
        assert!(!service::task_file(&dir).exists(), "no definition file");
        assert!(
            !dir.exists(),
            "nothing left: {:?}",
            std::fs::read_dir(&dir).map(|d| d.count())
        );
    }

    /// A program that other users of the computer can replace is not registered: the task
    /// would run whatever they put there as this user.
    pub fn a_program_others_can_replace_is_refused() {
        let public = std::env::var_os("PUBLIC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
        let dir = public.join(format!("service-task-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("stand-in.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let config = dir.join("daemon.toml");
        std::fs::write(&config, "").unwrap();
        let name = format!("{}.refused-{}", service::task_name(), std::process::id());
        let res = service::install_task(&name, &exe, &config, &dir, true);
        let registered = service::task_exists(&name);
        let _ = service::uninstall_task(&name, &dir, true);
        let _ = std::fs::remove_dir_all(&dir);
        let err = res.expect_err("a program in the public folder must be refused");
        println!("  refused: {err}");
        assert!(err.contains("not a safe place"), "{err}");
        assert!(!registered);
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
        // Where this user has no logon session the Task Scheduler never starts the task: the
        // test starts its actions the way the scheduler would.
        let _scheduler = play_scheduler(&name, &file);
        // Registered as written, with the logon trigger for this user's security id (not the
        // fallback without it).
        let definition: Vec<u16> = std::fs::read(&file).unwrap()[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(String::from_utf16_lossy(&definition).contains("<LogonTrigger>"));
        let first = wait_running(&config, None, Duration::from_secs(60), "started");
        assert!(first.console, "the daemon has a console (a headless one)");

        // One instance: asking for another start changes nothing.
        assert!(schtasks(&["/Run", "/TN", &name]) || no_logon_session());
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

        // Restarted on request: the running one is stopped, another one runs.
        service::restart_task(&name, &dir).expect("restart");
        let third = wait_running(
            &config,
            Some(third.pid),
            Duration::from_secs(60),
            "started again on request",
        );

        // An update interrupted between its two renames (a power loss): no program file, only
        // the renamed-aside one. The task's first action puts it back, then the daemon starts.
        let aside = dir.join(format!("stand-in.exe{}9", selfupdate::STALE_MARK));
        std::fs::rename(&exe, &aside).unwrap();
        assert!(
            Command::new("taskkill")
                .args(["/F", "/PID", &third.pid.to_string()])
                .output()
                .is_ok_and(|o| o.status.success())
        );
        let third = wait_running(
            &config,
            Some(third.pid),
            NEXT_START,
            "started after an interrupted update",
        );
        assert!(exe.is_file() && !aside.exists(), "the program file is back");

        // A restart while the daemon is slow to stop (it has a run to cancel) and the task
        // is asked to start again and again meanwhile, as its every-minute trigger does: the
        // starts that come while the old daemon still runs end at once (one daemon per
        // config), and afterwards exactly one daemon runs, the one the pid file names.
        std::fs::write(config.with_extension("slow-stop"), "").unwrap();
        let slow = {
            // The running stand-in read the marker at its start: start one that has it.
            service::restart_task(&name, &dir).expect("restart");
            wait_running(
                &config,
                Some(third.pid),
                Duration::from_secs(60),
                "slow one",
            )
        };
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let trigger = {
            let (stop, name) = (stop.clone(), name.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = schtasks(&["/Run", "/TN", &name]);
                    std::thread::sleep(Duration::from_millis(400));
                }
            })
        };
        std::thread::sleep(Duration::from_secs(1));
        service::restart_task(&name, &dir).expect("restart under starts");
        let after = wait_running(
            &config,
            Some(slow.pid),
            Duration::from_secs(90),
            "started after a slow stop",
        );
        std::thread::sleep(Duration::from_secs(3));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        trigger.join().unwrap();
        assert!(!daemon::winjob::pid_alive(slow.pid), "the old daemon ended");
        let image = exe.file_name().unwrap().to_string_lossy().into_owned();
        wait_until(Duration::from_secs(30), "one daemon runs", || {
            pids_of(&image).len() == 1
        });
        let only = pids_of(&image)[0];
        let named: u32 = std::fs::read_to_string(service::task_pid_file(&dir))
            .expect("the pid file is there")
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(named, only, "the pid file names the daemon that runs");
        let third = Running {
            pid: only,
            console: after.console,
        };

        // Uninstalled while the daemon is slow to stop and starts keep coming: no task, no
        // process, and none comes back.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let trigger = {
            let (stop, name) = (stop.clone(), name.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = schtasks(&["/Run", "/TN", &name]);
                    std::thread::sleep(Duration::from_millis(400));
                }
            })
        };
        std::thread::sleep(Duration::from_secs(1));
        service::uninstall_task(&name, &dir, true).expect("uninstall");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        trigger.join().unwrap();
        wait_until(Duration::from_secs(30), "no daemon runs", || {
            pids_of(&image).is_empty()
        });
        assert!(
            !service::task_pid_file(&dir).exists(),
            "the pid file is gone"
        );
        // Uninstalled: no task, no process.
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
