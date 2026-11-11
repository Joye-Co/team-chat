//! The update helper of the Windows app. See `lib.rs`.

// No console window: the app starts it in the background.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    std::process::exit(update_helper::main(args));
}
