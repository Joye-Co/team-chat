//! Windows: the process tree of a run is a Job Object.
//!
//! The CLI is started suspended, put into a new job and then let go, so every process it
//! starts is in the job from its first instruction, also one that makes a process group or a
//! console of its own. The job does not allow breakaway and is killed when its last handle
//! closes: the daemon holds the only handle, so a daemon that crashes takes its runs' process
//! trees with it. Ending the job ends every process in it at once; nothing has to be looked up
//! the way [`crate::proctree`] does on Unix.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, PROCESS_SYNCHRONIZE, ResumeThread, SuspendThread,
    THREAD_SUSPEND_RESUME, WaitForSingleObject,
};

/// `CreateProcess` flags of a run's CLI: its own process group (so `CTRL_BREAK_EVENT` can be
/// sent to it alone) and suspended until it is in its job.
pub const CREATION_FLAGS: u32 = windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP
    | windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

/// A job: the processes of one run.
#[derive(Debug)]
pub struct Job {
    handle: HANDLE,
    /// Threads that [`Job::suspend`] suspended and [`Job::resume`] has not resumed yet.
    suspended: Mutex<HashSet<u32>>,
}

// SAFETY: a job handle may be used from any thread; the kernel object is thread-safe.
unsafe impl Send for Job {}
// SAFETY: as above.
unsafe impl Sync for Job {}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and still open. Closing the last handle kills the job.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

impl Job {
    /// A new, empty job: kill on close, no breakaway (the default: neither breakaway flag is
    /// set), not inheritable, without a name.
    pub fn new() -> std::io::Result<Job> {
        // SAFETY: null attributes and name are allowed.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(last_error());
        }
        let job = Job {
            handle,
            suspended: Mutex::new(HashSet::new()),
        };
        // SAFETY: a plain C struct; zeroes are a valid value.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the pointer and size describe `info`.
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        Ok(job)
    }

    /// Puts the process behind `process` (a handle with `PROCESS_SET_QUOTA` and
    /// `PROCESS_TERMINATE` access, such as a child's) into the job. A handle that is not one
    /// is an error from the kernel, not undefined behaviour.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn assign(&self, process: HANDLE) -> std::io::Result<()> {
        // SAFETY: both handles are open.
        if unsafe { AssignProcessToJobObject(self.handle, process) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    /// How many processes of the job are alive.
    pub fn active(&self) -> u32 {
        // SAFETY: a plain C struct; zeroes are a valid value.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the pointer and size describe `info`.
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle,
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 { 0 } else { info.ActiveProcesses }
    }

    /// The ids of the job's live processes.
    pub fn pids(&self) -> Vec<u32> {
        let head = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
        let mut room = 64usize;
        loop {
            // u64 elements keep the buffer aligned for the struct.
            let bytes = head + room * std::mem::size_of::<usize>();
            let mut buf = vec![0u64; bytes.div_ceil(8)];
            // SAFETY: the buffer holds at least `bytes` bytes.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle,
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr().cast::<c_void>(),
                    bytes as u32,
                    std::ptr::null_mut(),
                )
            };
            let list = buf.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            // SAFETY: the buffer is at least as large as the struct's header and aligned.
            let (assigned, listed) = unsafe {
                (
                    (*list).NumberOfAssignedProcesses as usize,
                    (*list).NumberOfProcessIdsInList as usize,
                )
            };
            // ERROR_MORE_DATA: the list did not fit.
            if ok == 0 && assigned > room && room < 1 << 16 {
                room = assigned * 2;
                continue;
            }
            if ok == 0 && listed == 0 {
                return vec![];
            }
            let n = listed.min(room);
            // SAFETY: `n` ids follow the header inside the buffer.
            let ids = unsafe {
                std::slice::from_raw_parts(buf.as_ptr().cast::<u8>().add(head).cast::<usize>(), n)
            };
            return ids.iter().map(|p| *p as u32).collect();
        }
    }

    /// Ends every process of the job, now.
    pub fn terminate(&self) {
        // SAFETY: the handle is open.
        unsafe {
            TerminateJobObject(self.handle, 1);
        }
    }

    /// Suspends every thread of every process of the job (接管). Looks again until a look
    /// finds no thread it has not suspended, so a process started meanwhile stops too.
    pub fn suspend(&self) {
        let mut suspended = self.suspended.lock().unwrap_or_else(|e| e.into_inner());
        for _ in 0..50 {
            let pids: HashSet<u32> = self.pids().into_iter().collect();
            let new: Vec<u32> = threads_of(&pids)
                .into_iter()
                .filter(|t| !suspended.contains(t))
                .collect();
            if new.is_empty() {
                return;
            }
            for tid in new {
                if with_thread(tid, |h| {
                    // SAFETY: the handle has THREAD_SUSPEND_RESUME access.
                    unsafe { SuspendThread(h) != u32::MAX }
                }) {
                    suspended.insert(tid);
                }
            }
        }
    }

    /// Lets the threads that [`Job::suspend`] suspended run again. Only threads that still
    /// belong to a process of the job: a thread id can be reused.
    pub fn resume(&self) {
        let mut suspended = self.suspended.lock().unwrap_or_else(|e| e.into_inner());
        if suspended.is_empty() {
            return;
        }
        let pids: HashSet<u32> = self.pids().into_iter().collect();
        for tid in threads_of(&pids) {
            if suspended.contains(&tid) {
                with_thread(tid, |h| {
                    // SAFETY: the handle has THREAD_SUSPEND_RESUME access.
                    unsafe { ResumeThread(h) != u32::MAX }
                });
            }
        }
        suspended.clear();
    }
}

fn with_thread(tid: u32, f: impl FnOnce(HANDLE) -> bool) -> bool {
    // SAFETY: plain call; a null handle means the thread is gone or not ours.
    let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, tid) };
    if h.is_null() {
        return false;
    }
    let ok = f(h);
    // SAFETY: the handle is ours.
    unsafe {
        CloseHandle(h);
    }
    ok
}

/// The ids of the threads of the processes `pids`.
fn threads_of(pids: &HashSet<u32>) -> Vec<u32> {
    if pids.is_empty() {
        return vec![];
    }
    // SAFETY: plain call.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return vec![];
    }
    let mut out = vec![];
    // SAFETY: a plain C struct; zeroes are a valid value.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    // SAFETY: `entry` is valid and has its size set.
    let mut more = unsafe { Thread32First(snap, &mut entry) } != 0;
    while more {
        if pids.contains(&entry.th32OwnerProcessID) {
            out.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snap, &mut entry) } != 0;
    }
    // SAFETY: the snapshot handle is ours.
    unsafe {
        CloseHandle(snap);
    }
    out
}

/// Lets a process that was started suspended run: resumes its threads (it has one).
pub fn resume_process(pid: u32) -> bool {
    let mut any = false;
    for tid in threads_of(&HashSet::from([pid])) {
        any |= with_thread(tid, |h| {
            // SAFETY: the handle has THREAD_SUSPEND_RESUME access.
            unsafe { ResumeThread(h) != u32::MAX }
        });
    }
    any
}

/// Sends `CTRL_BREAK_EVENT` to the process group `pid` leads (a process started with
/// `CREATE_NEW_PROCESS_GROUP`): the request to stop that a console program can handle. False
/// when it could not be sent: the daemon has no console, or shares none with the group.
pub fn ctrl_break(pid: u32) -> bool {
    // Group 0 is every process of the daemon's own console: never.
    if pid == 0 {
        return false;
    }
    // SAFETY: plain call.
    unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0 }
}

/// Whether a process with this id is running. A pid can be reused: use it only for a process
/// that was alive a moment ago.
pub fn pid_alive(pid: u32) -> bool {
    // SAFETY: plain call; a null handle means no such process (or no access).
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if h.is_null() {
        return false;
    }
    // SAFETY: the handle has SYNCHRONIZE access; a zero timeout does not block.
    let state = unsafe { WaitForSingleObject(h, 0) };
    // SAFETY: the handle is ours.
    unsafe {
        CloseHandle(h);
    }
    state == WAIT_TIMEOUT
}
