//! A session's processes, without Tauri: the spawned child in a Windows job object, plus processes run from a
//! tracked folder (launcher stubs that relaunch the game outside the job).
//! - Tracking is an OS wait: the job's completion port reports "no active processes". Then the folder is checked
//!   once for processes from it, and those are waited on in turn. The session ends when none remain.
//! - Closing asks first (WM_CLOSE to the session's top-level windows, like their X button), waits, then ends only
//!   processes in the job or whose image is under the folder.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, OpenThread, QueryFullProcessImageNameW, ResumeThread, TerminateProcess,
    WaitForMultipleObjects, WaitForSingleObject, CREATE_SUSPENDED, INFINITE, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, THREAD_SUSPEND_RESUME,
};
use windows::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, GW_OWNER, WM_CLOSE,
};

/// JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO (in SystemServices, not worth a feature for one number).
const MSG_ACTIVE_PROCESS_ZERO: u32 = 4;
/// Job messages are notifications Windows may drop, so the waiter also looks at the job's count this often.
const RECHECK: u32 = 60_000;

/// A kernel handle, closed on drop. Handles may be used from any thread.
pub(crate) struct Owned(isize);
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}
impl Owned {
    fn new(h: HANDLE) -> Self {
        Owned(h.0 as isize)
    }
    pub(crate) fn get(&self) -> HANDLE {
        HANDLE(self.0 as _)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.get());
        }
    }
}

/// The processes of one session.
pub struct Tracked {
    job: Owned,
    port: Owned,
    /// false if Windows refused to put the child in the job (then the child's own handle is waited on)
    in_job: bool,
    /// the spawned child; keeping its handle open keeps its pid from being reused
    child: Owned,
    pub pid: u32,
    pub folder: Option<PathBuf>,
}

impl Tracked {
    /// Spawns `cmd` suspended, puts it in a new job and lets it run. Its children join the job by themselves; a child
    /// may break away (some launchers ask to), which the folder check then catches.
    pub fn spawn(cmd: &mut Command, folder: Option<PathBuf>) -> Result<Tracked, String> {
        let folder = folder.filter(|f| trackable(f));
        unsafe {
            let job = Owned::new(CreateJobObjectW(None, PCWSTR::null()).map_err(|e| format!("job: {e}"))?);
            let port =
                Owned::new(CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1).map_err(|e| format!("port: {e}"))?);
            let assoc = JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey: std::ptr::null_mut(), CompletionPort: port.get() };
            set_info(&job, JobObjectAssociateCompletionPortInformation, &assoc)?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // without this a launcher asking CreateProcess to break away would fail to start the game at all
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            set_info(&job, JobObjectExtendedLimitInformation, &limits)?;

            let child = cmd
                .creation_flags(CREATE_SUSPENDED.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let pid = child.id();
            let process = HANDLE(child.as_raw_handle() as _);
            let in_job = AssignProcessToJobObject(job.get(), process).is_ok();
            resume_threads(pid);
            // keep a handle of our own: Child closes its handle on drop
            let own = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map_err(|e| format!("opening {pid}: {e}"))?;
            drop(child);
            Ok(Tracked { job, port, in_job, child: Owned::new(own), pid, folder })
        }
    }

    /// Blocks until the session is over: the job is empty (or, outside a job, the child has exited) and no process runs
    /// from the folder. An OS wait throughout.
    pub fn wait(&self) {
        loop {
            if self.in_job {
                self.wait_job_empty();
            } else {
                unsafe { WaitForSingleObject(self.child.get(), INFINITE) };
            }
            let Some(folder) = &self.folder else { return };
            // a launcher stub may have started the game outside the job and exited: wait for those too
            let left = processes_under(folder);
            if left.is_empty() {
                return;
            }
            let handles: Vec<Owned> = left
                .iter()
                .filter_map(|p| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, p.pid) }.ok().map(Owned::new))
                .collect();
            if handles.is_empty() {
                return;
            }
            let raw: Vec<HANDLE> = handles.iter().take(64).map(Owned::get).collect();
            unsafe { WaitForMultipleObjects(&raw, true, INFINITE) };
        }
    }

    fn wait_job_empty(&self) {
        loop {
            if self.active_in_job() == 0 {
                return;
            }
            let (mut msg, mut key, mut ov) = (0u32, 0usize, std::ptr::null_mut::<OVERLAPPED>());
            let got = unsafe { GetQueuedCompletionStatus(self.port.get(), &mut msg, &mut key, &mut ov, RECHECK) };
            if got.is_ok() && msg == MSG_ACTIVE_PROCESS_ZERO {
                return;
            }
        }
    }

    fn active_in_job(&self) -> u32 {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                Some(self.job.get()),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as _,
                std::mem::size_of_val(&info) as u32,
                None,
            )
        };
        if ok.is_ok() {
            info.ActiveProcesses
        } else {
            0
        }
    }

    /// The pids in the job now.
    pub fn job_pids(&self) -> Vec<u32> {
        if !self.in_job {
            return if unsafe { WaitForSingleObject(self.child.get(), 0) } == WAIT_TIMEOUT { vec![self.pid] } else { vec![] };
        }
        // a header of two u32s, then one usize per pid
        let mut buf = vec![0usize; 1026];
        let ok = unsafe {
            QueryInformationJobObject(
                Some(self.job.get()),
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr() as _,
                (buf.len() * std::mem::size_of::<usize>()) as u32,
                None,
            )
        };
        if ok.is_err() {
            return vec![];
        }
        let n = (buf[0] >> 32) as usize; // NumberOfProcessIdsInList, the second u32
        buf[1..=n.min(1025)].iter().map(|&p| p as u32).collect()
    }

    /// Every process of the session now: the job's and the folder's, never this app.
    pub fn pids(&self) -> Vec<u32> {
        let mut pids = self.job_pids();
        if let Some(folder) = &self.folder {
            pids.extend(processes_under(folder).into_iter().map(|p| p.pid));
        }
        let me = unsafe { GetCurrentProcessId() };
        pids.retain(|&p| p != me && p != 0);
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// True if `pid` belongs to the session: in the job, or run from the folder.
    pub fn contains(&self, pid: u32) -> bool {
        let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
        let h = Owned::new(h);
        if self.in_job {
            let mut inside = BOOL(0);
            if unsafe { IsProcessInJob(h.get(), Some(self.job.get()), &mut inside) }.is_ok() && inside.as_bool() {
                return true;
            }
        } else if pid == self.pid {
            return true;
        }
        match (&self.folder, image_of(h.get())) {
            (Some(folder), Some(image)) => is_under(&image, folder),
            _ => false,
        }
    }

    /// Closes the session like its windows' X buttons: WM_CLOSE to every visible top-level window it owns, then up to
    /// `grace` for its processes to exit, then ends what is left (the job, and processes run from the folder).
    pub fn close(&self, grace: Duration) -> Closed {
        let pids = self.pids();
        let windows = top_windows(&pids);
        for w in &windows {
            unsafe {
                let _ = PostMessageW(Some(HWND(w.hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let handles: Vec<Owned> = pids
            .iter()
            .filter_map(|&p| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, p) }.ok().map(Owned::new))
            .collect();
        let raw: Vec<HANDLE> = handles.iter().take(64).map(Owned::get).collect();
        let asked = raw.is_empty()
            || unsafe { WaitForMultipleObjects(&raw, true, grace.as_millis().min(u32::MAX as u128) as u32) } != WAIT_TIMEOUT;
        let mut ended = Vec::new();
        if !asked {
            if self.in_job {
                ended.extend(self.job_pids());
                unsafe {
                    let _ = TerminateJobObject(self.job.get(), 1);
                }
            } else if unsafe { WaitForSingleObject(self.child.get(), 0) } == WAIT_TIMEOUT {
                if let Ok(h) = unsafe { OpenProcess(PROCESS_TERMINATE, false, self.pid) } {
                    let h = Owned::new(h);
                    if unsafe { TerminateProcess(h.get(), 1) }.is_ok() {
                        ended.push(self.pid);
                    }
                }
            }
            if let Some(folder) = &self.folder {
                ended.extend(end_under(folder));
            }
        }
        Closed { windows: windows.len(), asked, ended }
    }

    /// The window to bring back: `hint` if it is still a visible window of the session, else its largest visible
    /// top-level window.
    pub fn main_window(&self, hint: Option<isize>) -> Option<isize> {
        if let Some(h) = hint {
            let hwnd = HWND(h as _);
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 && unsafe { IsWindowVisible(hwnd) }.as_bool() && self.contains(pid) {
                return Some(h);
            }
        }
        top_windows(&self.pids()).into_iter().max_by_key(|w| w.area).map(|w| w.hwnd)
    }
}

/// What `close` did: windows asked, whether everything left within the grace time, and the pids it ended.
#[derive(Debug)]
pub struct Closed {
    pub windows: usize,
    pub asked: bool,
    pub ended: Vec<u32>,
}

fn set_info<T>(job: &Owned, class: windows::Win32::System::JobObjects::JOBOBJECTINFOCLASS, info: &T) -> Result<(), String> {
    unsafe { SetInformationJobObject(job.get(), class, info as *const T as _, std::mem::size_of::<T>() as u32) }
        .map_err(|e| format!("job settings: {e}"))
}

/// Resumes the threads of a process started suspended (it has one).
fn resume_threads(pid: u32) {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return };
        let snap = Owned::new(snap);
        let mut t = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut more = Thread32First(snap.get(), &mut t).is_ok();
        while more {
            if t.th32OwnerProcessID == pid {
                if let Ok(h) = OpenThread(THREAD_SUSPEND_RESUME, false, t.th32ThreadID) {
                    let h = Owned::new(h);
                    ResumeThread(h.get());
                }
            }
            more = Thread32Next(snap.get(), &mut t).is_ok();
        }
    }
}

// ---------- processes and folders ----------

/// A running process and its image path.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub image: PathBuf,
}

/// Every process whose image path can be read (most can, with limited query rights), except this app.
pub fn processes() -> Vec<Proc> {
    let me = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let snap = Owned::new(snap);
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap.get(), &mut e).is_ok();
        while more {
            let pid = e.th32ProcessID;
            if pid != 0 && pid != 4 && pid != me {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let h = Owned::new(h);
                    if let Some(image) = image_of(h.get()) {
                        out.push(Proc { pid, image });
                    }
                }
            }
            more = Process32NextW(snap.get(), &mut e).is_ok();
        }
    }
    out
}

fn image_of(h: HANDLE) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

pub fn processes_under(folder: &Path) -> Vec<Proc> {
    select_under(processes(), folder)
}

/// The processes whose image is inside `folder` (at any depth).
pub fn select_under(all: Vec<Proc>, folder: &Path) -> Vec<Proc> {
    all.into_iter().filter(|p| is_under(&p.image, folder)).collect()
}

/// True if `image` is inside `folder`: compared case-insensitively, component by component, so `Games\Hades` does
/// not take `Games\Hades II`.
pub fn is_under(image: &Path, folder: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
        let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
        s.trim_end_matches('\\').to_string()
    };
    let (image, folder) = (norm(image), norm(folder));
    !folder.is_empty() && image.len() > folder.len() + 1 && image.starts_with(&folder) && image[folder.len()..].starts_with('\\')
}

/// A folder is only tracked if it is a real game folder: never a drive root, where "processes from the folder" would be
/// everything on the drive.
pub fn trackable(folder: &Path) -> bool {
    use std::path::Component;
    folder.is_absolute() && folder.components().filter(|c| matches!(c, Component::Normal(_))).count() >= 1
}

/// Ends the processes run from `folder` (re-checking each one's image after opening it, in case its pid was reused).
fn end_under(folder: &Path) -> Vec<u32> {
    let mut ended = Vec::new();
    for p in processes_under(folder) {
        let Ok(h) = (unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, p.pid) }) else {
            continue;
        };
        let h = Owned::new(h);
        if image_of(h.get()).is_some_and(|i| is_under(&i, folder)) && unsafe { TerminateProcess(h.get(), 1) }.is_ok() {
            ended.push(p.pid);
        }
    }
    ended
}

// ---------- windows ----------

#[derive(Debug, Clone, Copy)]
pub struct TopWindow {
    pub hwnd: isize,
    pub area: i64,
}

/// The visible, unowned top-level windows of `pids`: the ones with an X button a player would press.
pub fn top_windows(pids: &[u32]) -> Vec<TopWindow> {
    struct Find<'a> {
        pids: &'a [u32],
        found: Vec<TopWindow>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let f = unsafe { &mut *(lp.0 as *mut Find) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if f.pids.contains(&pid)
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && unsafe { GetWindow(hwnd, GW_OWNER) }.map_or(true, |o| o.0.is_null())
        {
            let mut r = Default::default();
            let area = if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
                i64::from(r.right - r.left) * i64::from(r.bottom - r.top)
            } else {
                0
            };
            f.found.push(TopWindow { hwnd: hwnd.0 as isize, area });
        }
        BOOL(1)
    }
    let mut f = Find { pids, found: Vec::new() };
    if !pids.is_empty() {
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Find as isize));
        }
    }
    f.found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn folder_matching_is_by_component_and_case_insensitive() {
        let f = Path::new(r"W:\games-test\Hades");
        assert!(is_under(Path::new(r"W:\games-test\Hades\Hades.exe"), f));
        assert!(is_under(Path::new(r"w:\GAMES-TEST\hades\x64\bin\Hades.exe"), f));
        assert!(is_under(Path::new(r"\\?\W:\games-test\Hades\Hades.exe"), f));
        assert!(is_under(Path::new(r"W:\games-test\Hades\Hades.exe"), Path::new(r"W:\games-test\Hades\")));
        assert!(is_under(Path::new("W:/games-test/Hades/Hades.exe"), f));
        // a sibling whose name starts the same is another game
        assert!(!is_under(Path::new(r"W:\games-test\Hades II\Hades2.exe"), f));
        assert!(!is_under(Path::new(r"W:\games-test\Hades"), f), "the folder itself is not a process in it");
        assert!(!is_under(Path::new(r"C:\Windows\explorer.exe"), f));
        assert!(!is_under(Path::new(r"W:\games-test\Hades\x.exe"), Path::new("")));
    }

    #[test]
    fn select_under_keeps_only_the_folder() {
        let p = |pid, s: &str| Proc { pid, image: PathBuf::from(s) };
        let all = vec![
            p(1, r"W:\Games\Celeste\Celeste.exe"),
            p(2, r"W:\Games\Celeste\launcher\stub.exe"),
            p(3, r"W:\Games\Celeste 2\Celeste.exe"),
            p(4, r"C:\Program Files\Steam\steam.exe"),
        ];
        let got: Vec<u32> = select_under(all, Path::new(r"W:\Games\Celeste")).iter().map(|p| p.pid).collect();
        assert_eq!(got, vec![1, 2]);
    }

    #[test]
    fn drive_roots_are_never_tracked() {
        assert!(!trackable(Path::new(r"C:\")));
        assert!(!trackable(Path::new("relative")));
        assert!(trackable(Path::new(r"C:\Games\Hades")));
    }

    /// A scratch folder on W: (TEMP points there on the dev laptop) with copies of Windows tools as "game" exes.
    fn scratch(name: &str, exes: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ch-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for exe in exes {
            std::fs::copy(Path::new(r"C:\Windows\System32").join(exe), dir.join(exe)).unwrap();
        }
        dir
    }

    fn ping(exe: &Path, secs: u32) -> Command {
        let mut c = Command::new(exe);
        c.args(["-n", &(secs + 1).to_string(), "127.0.0.1"]);
        c
    }

    fn alive(pid: u32) -> bool {
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
            .map(|h| {
                let h = Owned::new(h);
                (unsafe { WaitForSingleObject(h.get(), 0) }) == WAIT_TIMEOUT
            })
            .unwrap_or(false)
    }

    #[test]
    fn the_job_tracks_children_and_the_folder_tracks_what_left_it() {
        let dir = scratch("track", &["cmd.exe", "PING.EXE"]);
        // the "launcher" (cmd in the folder) starts a child and waits for it: both are in the job
        let t0 = Instant::now();
        let mut launcher = Command::new(dir.join("cmd.exe"));
        launcher.raw_arg(format!("/c \"\"{}\" -n 3 127.0.0.1 >nul\"", dir.join("PING.EXE").display()));
        let t = Tracked::spawn(&mut launcher, Some(dir.clone())).unwrap();
        assert!(t.in_job);
        t.wait();
        let took = t0.elapsed();
        assert!(took >= Duration::from_millis(1800), "ended when the child ended, not before: {took:?}");

        // a game started outside the job (as a launcher stub that relaunches would) keeps the session alive
        let outside = ping(&dir.join("PING.EXE"), 3).stdout(Stdio::null()).spawn().unwrap();
        let t0 = Instant::now();
        let t = Tracked::spawn(Command::new(dir.join("cmd.exe")).args(["/c", "exit"]), Some(dir.clone())).unwrap();
        assert!(t.contains(outside.id()), "a process run from the folder belongs to the session");
        t.wait();
        let took = t0.elapsed();
        assert!(took >= Duration::from_millis(2500), "waited for the process outside the job: {took:?}");
        assert!(!alive(outside.id()));

        // no folder: the job alone
        let t0 = Instant::now();
        let t = Tracked::spawn(Command::new(dir.join("cmd.exe")).args(["/c", "exit"]), None).unwrap();
        t.wait();
        assert!(t0.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A launcher stub (`W:\tmp\ch-lc\make-launcher.ps1` makes one) starts the real game outside the job and exits at
    /// once; the session must last until the real game is closed.
    #[test]
    #[ignore = "real windows: set CH_LAUNCHER to a launcher-stub folder; shows a full-screen black window for 3 s"]
    fn real_launcher_stub_keeps_the_session() {
        let dir = PathBuf::from(std::env::var("CH_LAUNCHER").expect("set CH_LAUNCHER"));
        let t0 = Instant::now();
        let t = std::sync::Arc::new(Tracked::spawn(&mut Command::new(dir.join("Launcher.exe")), Some(dir.clone())).unwrap());
        let waiter = {
            let t = t.clone();
            std::thread::spawn(move || {
                t.wait();
                t0.elapsed()
            })
        };
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!alive(t.pid), "the launcher exited");
        let pids = t.pids();
        println!("after 1.5 s: job {:?}, session {:?}", t.job_pids(), pids);
        assert!(!pids.is_empty(), "the real game, outside the job, is in the session");
        assert!(!waiter.is_finished(), "the session is still running");
        std::thread::sleep(Duration::from_millis(1500));
        let closed = t.close(Duration::from_secs(5));
        println!("close at {:?}: {closed:?}", t0.elapsed());
        assert!(closed.windows >= 1 && closed.asked, "WM_CLOSE closed its window");
        let ended = waiter.join().unwrap();
        println!("session ended after {ended:?}");
        assert!(ended >= Duration::from_secs(3));
    }

    #[test]
    fn close_ends_only_the_job_and_the_folder() {
        let dir = scratch("close", &["PING.EXE"]);
        let other = scratch("close-other", &["PING.EXE"]);
        // in the job
        let t = Tracked::spawn(&mut ping(&dir.join("PING.EXE"), 60), Some(dir.clone())).unwrap();
        // run from the folder, outside the job
        let stray = ping(&dir.join("PING.EXE"), 60).stdout(Stdio::null()).spawn().unwrap();
        // neither: same exe name, another folder
        let mut bystander = ping(&other.join("PING.EXE"), 60).stdout(Stdio::null()).spawn().unwrap();
        let mut pids = t.pids();
        pids.sort();
        let mut want = vec![t.pid, stray.id()];
        want.sort();
        assert_eq!(pids, want);
        assert!(!t.contains(bystander.id()));

        let t0 = Instant::now();
        let closed = t.close(Duration::from_millis(500));
        // ping has no window: nothing to ask, so it is ended after the grace time
        assert_eq!(closed.windows, 0);
        assert!(!closed.asked);
        assert!(t0.elapsed() >= Duration::from_millis(450));
        assert!(closed.ended.contains(&t.pid) && closed.ended.contains(&stray.id()), "{closed:?}");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!alive(t.pid) && !alive(stray.id()));
        assert!(alive(bystander.id()), "a process outside the job and the folder is never touched");
        t.wait();
        let _ = bystander.kill();
        let _ = bystander.wait();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }
}
