//! Starts a fake CLI of the tests on Windows, where a Python script cannot be started by its
//! `#!` line: `fake-cli.exe <args>` runs `python %FAKE_CLI_SCRIPT% <args>` with the same
//! standard streams and returns its exit code. Without the variable, `fake-cli.exe --version`
//! prints a version. Built only with the `test-fakes` feature.

fn main() {
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
