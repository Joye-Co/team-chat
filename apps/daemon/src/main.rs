use daemon::config::{self, Config};
use std::path::PathBuf;

const USAGE: &str = "usage: daemon <run|doctor|install-service|uninstall-service|example-config> [--config <file>] [--no-load]";

fn main() {
    // The same binary is `chat-run` inside a run (a link named chat-run on the run's PATH).
    let argv0 = std::env::args_os().next().unwrap_or_default();
    if std::path::Path::new(&argv0).file_name()
        == Some(std::ffi::OsStr::new(daemon::chat_run::NAME))
    {
        std::process::exit(daemon::chat_run::main(std::env::args().skip(1).collect()));
    }
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let mut config_path = config::default_config_path();
    let mut load = true;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => match args.next() {
                Some(p) => config_path = PathBuf::from(p),
                None => exit_with(USAGE),
            },
            "--no-load" => load = false,
            _ => exit_with(USAGE),
        }
    }
    let load_cfg = || Config::load(&config_path).unwrap_or_else(|e| exit_with(&e));
    match cmd.as_str() {
        "run" => {
            let cfg = load_cfg();
            let problems = cfg.problems();
            if !problems.is_empty() {
                exit_with(&problems.join("\n"));
            }
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| exit_with(&e.to_string()));
            let res = rt.block_on(daemon::daemon::run(cfg, async {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal handler");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
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
            let _ = load_cfg();
            match daemon::service::install(
                &daemon::service::agents_dir(),
                &config_path,
                &daemon::service::log_dir(),
                load,
            ) {
                Ok(p) => println!("installed {}", p.display()),
                Err(e) => exit_with(&e),
            }
        }
        "uninstall-service" => {
            match daemon::service::uninstall(&daemon::service::agents_dir(), load) {
                Ok(p) => println!("removed {}", p.display()),
                Err(e) => exit_with(&e),
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
