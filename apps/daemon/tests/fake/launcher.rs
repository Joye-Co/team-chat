//! Starts a fake CLI of the tests on Windows, where a Python script cannot be started by its
//! `#!` line: `fake-cli.exe <args>` runs `python %FAKE_CLI_SCRIPT% <args>` with the same
//! standard streams and returns its exit code. Without the variable, `fake-cli.exe --version`
//! prints a version. `fake-cli --read-only-check <folder>` is a check of its own (see
//! [`read_only_check`]). Built only with the `test-fakes` feature.

/// `fake-cli --read-only-check <folder>`: what a side chat's CLI can do in its checkout, as
/// the user this is started as (CI starts it as a user without administrator rights, whom
/// no privilege lets past an access list). Makes a checkout in `<folder>`, makes it read-only
/// the way the daemon does, and tries every kind of change. Exit code 0 when all were
/// refused and the checkout is writable again afterwards.
fn read_only_check(folder: &std::path::Path) -> i32 {
    let work = folder.join("checkout");
    let sub = work.join("src");
    std::fs::create_dir_all(&sub).expect("make the checkout");
    std::fs::write(sub.join("a.rs"), "x").expect("write a file");
    std::fs::write(work.join("b.txt"), "x").expect("write a file");
    daemon::workspace::set_read_only(&work, true);
    let tries: Vec<(&str, std::io::Result<()>)> = vec![
        ("change a file", std::fs::write(sub.join("a.rs"), "y")),
        ("make a file", std::fs::write(work.join("new.txt"), "y")),
        (
            "make a file in a folder",
            std::fs::write(sub.join("new.rs"), "y"),
        ),
        ("make a folder", std::fs::create_dir(work.join("made"))),
        ("remove a file", std::fs::remove_file(work.join("b.txt"))),
        (
            "remove a file in a folder",
            std::fs::remove_file(sub.join("a.rs")),
        ),
        ("remove a folder", std::fs::remove_dir_all(&sub)),
        (
            "rename a file",
            std::fs::rename(work.join("b.txt"), work.join("c.txt")),
        ),
    ];
    let mut allowed = 0;
    for (what, result) in &tries {
        match result {
            Ok(()) => {
                allowed += 1;
                println!("ALLOWED  {what}");
            }
            Err(e) => println!("refused  {what}: {e}"),
        }
    }
    daemon::workspace::set_read_only(&work, false);
    let back = std::fs::write(sub.join("a.rs"), "z").and_then(|()| std::fs::remove_dir_all(&work));
    if let Err(e) = &back {
        println!("NOT WRITABLE AGAIN: {e}");
    }
    i32::from(allowed > 0 || back.is_err())
}

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--read-only-check") {
        let folder = args.get(2).expect("a folder");
        std::process::exit(read_only_check(std::path::Path::new(folder)));
    }
    let Some(script) = std::env::var_os("FAKE_CLI_SCRIPT") else {
        // The daemon asks a CLI for its version without the runtime's `env`: the fakes say
        // they are a current Claude Code (Windows gates only runs of a new enough one).
        if std::env::args().nth(1).as_deref() == Some("--version") {
            println!("2.1.999 (fake)");
            return;
        }
        eprintln!("fake-cli: FAKE_CLI_SCRIPT is not set");
        std::process::exit(2);
    };
    let python = std::env::var_os("FAKE_CLI_PYTHON").unwrap_or_else(|| "python".into());
    let status = std::process::Command::new(python)
        .arg(script)
        .args(std::env::args_os().skip(1))
        // The fakes write JSON with text outside ASCII; a pipe would get the ANSI code page.
        .env("PYTHONUTF8", "1")
        .status();
    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("fake-cli: cannot start python: {e}");
            std::process::exit(2);
        }
    }
}
