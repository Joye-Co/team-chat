use daemon::config::{self, Config};
use std::path::PathBuf;

const USAGE: &str = "usage: daemon <run|doctor|install-service|uninstall-service|restart-service|example-config> [--config <file>] [--no-load] [--log-file <file>]\n       daemon private-dir <folder>";

fn main() {
    // The same binary is `chat-run` inside a run (a link named chat-run on the run's PATH).
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if daemon::chat_run::started_as(&argv0, daemon::chat_run::NAME) {
        std::process::exit(daemon::chat_run::main(std::env::args().skip(1).collect()));
    }
    // `gh` in a run that uses the app's GitHub: the real gh with the person's token.
    if daemon::chat_run::started_as(&argv0, daemon::github::GH) {
        std::process::exit(daemon::github::gh_main(std::env::args().skip(1).collect()));
    }
    // PID 1 of a run's microVM (the tools disk names it vm-agent).
    if daemon::chat_run::started_as(&argv0, daemon::vm_agent::NAME) {
        std::process::exit(daemon::vm_agent::main());
    }
    // Windows: CLIs installed where the user's PATH does not reach (see the function).
    daemon::sys::add_usual_program_folders();
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    // The scheduled task's first action on Windows: put the program file back after an update
    // that was interrupted (daemon::selfupdate). Never an error: the daemon starts next.
    if cmd == "repair-program" {
        if let Some(exe) = args.next() {
            daemon::selfupdate::repair(std::path::Path::new(&exe));
        }
        return;
    }
    // Makes a folder that only this user can open, private from the moment it exists (the app
    // runs this for the daemon's config folder before it writes the token there). A folder
    // that is already there is taken when it is private, or made private while it is still
    // empty; anything else is refused.
    if cmd == "private-dir" {
        let dir = PathBuf::from(args.next().unwrap_or_else(|| exit_with(USAGE)));
        let made = daemon::sys::create_private_dir(&dir).or_else(|e| {
            let empty = std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_none());
            if empty {
                daemon::sys::set_mode(&dir, 0o700)
            } else {
                Err(e)
            }
        });
        if let Err(e) = made.and_then(|()| daemon::sys::set_mode(&dir, 0o700)) {
            exit_with(&format!("cannot make {} private: {e}", dir.display()));
        }
        if let Some(why) = daemon::sys::acl_problem(&dir) {
            exit_with(&format!("{} is not private: {why}", dir.display()));
        }
        return;
    }
    let mut config_path = config::default_config_path();
    let mut load = true;
    let mut log_file: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => match args.next() {
                Some(p) => config_path = PathBuf::from(p),
                None => exit_with(USAGE),
            },
            "--no-load" => load = false,
            "--log-file" => match args.next() {
                Some(p) => log_file = Some(PathBuf::from(p)),
                None => exit_with(USAGE),
            },
            _ => exit_with(USAGE),
        }
    }
    let load_cfg = || Config::load(&config_path).unwrap_or_else(|e| exit_with(&e));
    match cmd.as_str() {
        "run" => {
            // The Windows scheduled task: its headless console shows nothing.
            if let Some(file) = &log_file
                && let Err(e) = daemon::service::log_to(file)
            {
                exit_with(&e);
            }
            // Windows: the only daemon of this user and config, or none (the Task Scheduler
            // can start a second one while the first shuts down). Held until the process ends.
            let _instance =
                daemon::service::single_instance(&config_path).unwrap_or_else(|e| exit_with(&e));
            if let Some(file) = &log_file
                && let Err(e) = daemon::service::stoppable(file)
            {
                exit_with(&e);
            }
            let cfg = load_cfg();
            let mut problems = cfg.problems();
            // Windows: the config can hold a runtime's keys (`env`); like the token file it
            // must not be readable by other users of the machine.
            if let Some(why) = daemon::sys::acl_problem(&config_path) {
                problems.push(format!(
                    "config {} must be private to this user: {why}",
                    config_path.display()
                ));
            }
            if !problems.is_empty() {
                exit_with(&problems.join("\n"));
            }
            // Windows: the first reading of the machine's load takes half a second; take it
            // here, not on a thread of the runtime.
            let _ = daemon::sys::load_avg();
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| exit_with(&e.to_string()));
            let res = rt.block_on(daemon::daemon::run(cfg, async {
                #[cfg(unix)]
                {
                    let mut term =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                            .expect("signal handler");
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = term.recv() => {}
                    }
                }
                // Windows: Ctrl+C, Ctrl+Break (what a service manager sends a console
                // program) and the console window closing.
                #[cfg(windows)]
                {
                    use tokio::signal::windows::{ctrl_break, ctrl_close};
                    let mut brk = ctrl_break().expect("signal handler");
                    let mut close = ctrl_close().expect("signal handler");
                    // The scheduled task's daemon is asked to stop through an event
                    // (uninstall-service, restart-service): it shares no console with them.
                    let (stop_tx, stop) = tokio::sync::oneshot::channel::<()>();
                    std::thread::spawn(move || {
                        daemon::service::wait_stop();
                        let _ = stop_tx.send(());
                    });
                    tokio::select! {
                        _ = stop => {}
                        _ = tokio::signal::ctrl_c() => {}
                        _ = brk.recv() => {}
                        _ = close.recv() => {}
                    }
                }
            }));
            if let Err(e) = res {
                exit_with(&e);
            }
        }
        "doctor" => {
            let cfg = load_cfg();
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| exit_with(&e.to_string()));
            let (report, ok) = rt.block_on(daemon::doctor::report(&cfg));
            print!("{report}");
            std::process::exit(if ok { 0 } else { 1 });
        }
        "install-service" => {
            let cfg = load_cfg();
            // Windows: a daemon that would stop at start-up (a token or config that other
            // users can read, no token) is not registered: the task would start it every
            // minute and only its log would say why. launchd and systemd installs are as before.
            if cfg!(windows) {
                let mut problems = cfg.problems();
                if let Some(why) = daemon::sys::acl_problem(&config_path) {
                    problems.push(format!(
                        "config {} must be private to this user: {why}",
                        config_path.display()
                    ));
                }
                if !problems.is_empty() {
                    exit_with(&problems.join("\n"));
                }
            }
            let res = if cfg!(windows) {
                std::env::current_exe()
                    .map_err(|e| e.to_string())
                    .and_then(|exe| {
                        daemon::service::install_task(
                            &daemon::service::task_name(),
                            &exe,
                            &config_path,
                            &daemon::service::task_dir(),
                            load,
                        )
                    })
            } else if cfg!(target_os = "linux") {
                daemon::service::install_systemd(
                    &daemon::service::units_dir(),
                    &config_path,
                    &daemon::service::systemd_log_dir(),
                    load,
                )
            } else {
                daemon::service::install(
                    &daemon::service::agents_dir(),
                    &config_path,
                    &daemon::service::log_dir(),
                    load,
                )
            };
            match res {
                Ok(p) => println!("installed {}", p.display()),
                Err(e) => exit_with(&e),
            }
        }
        "uninstall-service" => {
            let res = if cfg!(windows) {
                daemon::service::uninstall_task(
                    &daemon::service::task_name(),
                    &daemon::service::task_dir(),
                    load,
                )
            } else if cfg!(target_os = "linux") {
                daemon::service::uninstall_systemd(&daemon::service::units_dir(), load)
            } else {
                daemon::service::uninstall(&daemon::service::agents_dir(), load)
            };
            match res {
                Ok(p) => println!("removed {}", p.display()),
                Err(e) => exit_with(&e),
            }
        }
        // Windows: the scheduled task's daemon again, from the daemon file now on disk.
        "restart-service" if cfg!(windows) => {
            if let Err(e) = daemon::service::restart_task(
                &daemon::service::task_name(),
                &daemon::service::task_dir(),
            ) {
                exit_with(&e);
            }
        }
        "example-config" => print!("{}", config::EXAMPLE),
        _ => exit_with(USAGE),
    }
}

fn exit_with(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
}
