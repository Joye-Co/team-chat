//! Windows: a program in a pseudoconsole (ConPTY), for the `pty` runtime.
//!
//! The program gets a console of its own whose screen the daemon reads as a byte stream (with
//! terminal escape codes) and whose keyboard the daemon writes. Like a run's CLI it is started
//! suspended and put into a Job Object before it runs ([`crate::winjob`]).
//!
//! The standard library cannot attach a pseudoconsole to a process it starts, so this module
//! calls `CreateProcessW` itself: the command line and the environment block are built here.

use crate::winjob::Job;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
};

fn os_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// One argument as `CommandLineToArgvW` (and the C runtime) reads it back: in quotes when it
/// is empty or has a space, a tab or a quote; a quote gets a backslash, and backslashes
/// before a quote are doubled.
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// The command line of `program` with `args`.
pub fn command_line(program: &Path, args: &[String]) -> String {
    std::iter::once(quote_arg(&program.to_string_lossy()))
        .chain(args.iter().map(|a| quote_arg(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The environment block: `NAME=value` strings, each ending in NUL, sorted by name without
/// regard to case as Windows wants them, and one more NUL at the end.
pub fn environment_block(env: &[(String, String)]) -> Vec<u16> {
    let mut vars: Vec<&(String, String)> = env
        .iter()
        .filter(|(k, _)| !k.is_empty() && !k.contains(['=', '\0']))
        .collect();
    vars.sort_by_key(|(k, _)| k.to_uppercase());
    let mut block = vec![];
    for (k, v) in vars {
        block.extend(format!("{k}={}", v.replace('\0', "")).encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// A handle that is closed when dropped.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is ours and open.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn pipe() -> std::io::Result<(Owned, Owned)> {
    let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: both out pointers are valid; null attributes make handles that are not inherited.
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
        return Err(os_error());
    }
    Ok((Owned(read), Owned(write)))
}

/// A process handle of its own for a thread that waits for the program's end: a duplicate, so
/// it stays valid when the [`Pty`] is dropped while the thread still waits.
pub struct Waiter(Owned);

// SAFETY: a process handle may be used from any thread.
unsafe impl Send for Waiter {}

impl Waiter {
    /// Blocks until the program ends; returns its exit code.
    pub fn wait(self) -> Option<i64> {
        let h = self.0.0;
        // SAFETY: the handle is this value's own and open.
        if unsafe { WaitForSingleObject(h, INFINITE) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: as above; `code` is a valid out pointer.
        (unsafe { GetExitCodeProcess(h, &mut code) } != 0).then_some(i64::from(code as i32))
    }
}

/// A program running in a pseudoconsole.
pub struct Pty {
    pub pid: u32,
    process: Owned,
    console: Option<HPCON>,
    /// The program and everything it started; ended when the Pty drops.
    pub job: std::sync::Arc<Job>,
}

// SAFETY: process and pseudoconsole handles may be used from any thread.
unsafe impl Send for Pty {}

impl Pty {
    /// A handle to wait on, independent of this value's lifetime.
    pub fn waiter(&self) -> std::io::Result<Waiter> {
        use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut dup: HANDLE = std::ptr::null_mut();
        // SAFETY: the source handle is open; `dup` receives a new handle of this process.
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.process.0,
                GetCurrentProcess(),
                &mut dup,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            return Err(os_error());
        }
        Ok(Waiter(Owned(dup)))
    }

    /// Closes the pseudoconsole: the program loses its console (one that still runs ends), and
    /// the output stream ends once it has been read to its end. On Windows before 11 24H2 this
    /// blocks until the output is drained, so something must be reading it.
    pub fn close_console(&mut self) {
        if let Some(hpc) = self.console.take() {
            // SAFETY: the pseudoconsole is ours and open.
            unsafe { ClosePseudoConsole(hpc) }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // The job first: with the program gone the console has nothing left to draw.
        self.job.terminate();
        self.close_console();
    }
}

/// Starts `program` with `args` in a new pseudoconsole of `cols` x `rows` characters, in the
/// folder `cwd` with exactly the environment `env`. Returns the program, the stream to write
/// its keyboard input to and the stream of what it draws.
pub fn spawn(
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    (cols, rows): (i16, i16),
) -> std::io::Result<(Pty, std::fs::File, std::fs::File)> {
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut hpc: HPCON = 0;
    // SAFETY: both pipe ends are open; `hpc` receives the pseudoconsole.
    let hr = unsafe {
        CreatePseudoConsole(
            COORD { X: cols, Y: rows },
            input_read.0,
            output_write.0,
            0,
            &mut hpc,
        )
    };
    if hr != 0 {
        return Err(std::io::Error::other(format!(
            "CreatePseudoConsole failed: 0x{hr:08x}"
        )));
    }
    // The pseudoconsole holds its own copies of these two ends.
    drop((input_read, output_write));
    let mut pty_console = Some(hpc);
    let close = |c: &mut Option<HPCON>| {
        if let Some(h) = c.take() {
            // SAFETY: the pseudoconsole is ours and open.
            unsafe { ClosePseudoConsole(h) }
        }
    };

    // The attribute list that ties the new process to the pseudoconsole.
    let mut size = 0usize;
    // SAFETY: a size query.
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
    }
    let mut list = vec![0u64; size.div_ceil(8).max(1)];
    let list_ptr = list.as_mut_ptr().cast::<c_void>();
    // SAFETY: the buffer holds `size` bytes.
    if unsafe { InitializeProcThreadAttributeList(list_ptr, 1, 0, &mut size) } == 0 {
        let e = os_error();
        close(&mut pty_console);
        return Err(e);
    }
    // SAFETY: the list is initialized; for this attribute the value is the handle itself.
    let ok = unsafe {
        UpdateProcThreadAttribute(
            list_ptr,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpc as *const c_void,
            std::mem::size_of::<HPCON>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        let e = os_error();
        // SAFETY: the list was initialized above.
        unsafe { DeleteProcThreadAttributeList(list_ptr) };
        close(&mut pty_console);
        return Err(e);
    }

    // SAFETY: a plain C struct; zeroes are a valid value.
    let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    // No standard handles of the daemon: when the daemon's own output is a file or a pipe the
    // program would write there, not to its console.
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    si.lpAttributeList = list_ptr;

    let mut line: Vec<u16> = command_line(program, args)
        .encode_utf16()
        .chain([0])
        .collect();
    let block = environment_block(env);
    let dir: Vec<u16> = cwd.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a plain C struct; zeroes are a valid value.
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is to a NUL-terminated buffer or struct that outlives the call.
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            block.as_ptr().cast(),
            dir.as_ptr(),
            &raw const si.StartupInfo,
            &mut pi,
        )
    };
    let started = if ok == 0 { Err(os_error()) } else { Ok(()) };
    // SAFETY: the list was initialized above and the process has its copy.
    unsafe { DeleteProcThreadAttributeList(list_ptr) };
    if let Err(e) = started {
        close(&mut pty_console);
        return Err(e);
    }
    let process = Owned(pi.hProcess);
    let thread = Owned(pi.hThread);
    // Into a job before its first instruction; a failure ends it (the job, or TerminateProcess
    // through the drop of nothing: the suspended process is killed explicitly).
    let job = Job::new().and_then(|job| job.assign(process.0).map(|_| job));
    let job = match job {
        Ok(j) => j,
        Err(e) => {
            // SAFETY: the process handle is open; the process is still suspended.
            unsafe {
                windows_sys::Win32::System::Threading::TerminateProcess(process.0, 1);
            }
            close(&mut pty_console);
            return Err(e);
        }
    };
    // SAFETY: the thread handle is open.
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let e = os_error();
        job.terminate();
        close(&mut pty_console);
        return Err(e);
    }
    drop(thread);
    // SAFETY: each handle is open, owned here and given to exactly one File.
    let (input, output) = unsafe {
        let input = std::fs::File::from_raw_handle(input_write.0.cast());
        let output = std::fs::File::from_raw_handle(output_read.0.cast());
        std::mem::forget((input_write, output_read));
        (input, output)
    };
    Ok((
        Pty {
            pid: pi.dwProcessId,
            process,
            console: pty_console,
            job: std::sync::Arc::new(job),
        },
        input,
        output,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_the_way_the_c_runtime_reads_them() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("two words"), "\"two words\"");
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r"C:\dir\"), r"C:\dir\");
        assert_eq!(quote_arg(r"C:\my dir\"), r#""C:\my dir\\""#);
        assert_eq!(quote_arg(r#"a\\"b"#), r#""a\\\\\"b""#);
        assert_eq!(
            command_line(
                Path::new(r"C:\Program Files\x.exe"),
                &["-c".into(), "a b".into()]
            ),
            r#""C:\Program Files\x.exe" -c "a b""#
        );
    }

    #[test]
    fn the_environment_block_is_sorted_and_ends_with_two_nuls() {
        let env = vec![
            ("b".to_string(), "2".to_string()),
            ("A".to_string(), "1".to_string()),
            ("bad=name".to_string(), "x".to_string()),
        ];
        let block = String::from_utf16(&environment_block(&env)).unwrap();
        assert_eq!(block, "A=1\0b=2\0\0");
        assert_eq!(environment_block(&[]), vec![0, 0]);
    }
}
