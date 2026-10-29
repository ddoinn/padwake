//! Power: sleep, restart, shut down and exit to the desktop. `PADWAKE_DRY_POWER=1` makes every action only
//! log, for tests on the dev laptop.

use super::log;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{
    ExitWindowsEx, EWX_FORCEIFHUNG, EWX_POWEROFF, EWX_REBOOT, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub const ACTIONS: [&str; 4] = ["sleep", "restart", "shutdown", "exit"];

pub fn dry_run() -> bool {
    std::env::var("PADWAKE_DRY_POWER").is_ok_and(|v| v == "1")
}

/// Runs a power action. `exit` is how the app quits (the caller passes `app.exit(0)`).
pub fn run(action: &str, exit: impl FnOnce()) -> Result<(), String> {
    if !ACTIONS.contains(&action) {
        return Err(format!("unknown power action {action}"));
    }
    if dry_run() {
        log(&format!("power: {action} (dry run)"));
        return Ok(());
    }
    log(&format!("power: {action}"));
    match action {
        "sleep" => {
            // returns after the PC wakes up again
            if !unsafe { SetSuspendState(false, false, false) } {
                return Err(format!("sleep failed: {:?}", unsafe { GetLastError() }));
            }
        }
        "restart" | "shutdown" => {
            enable_shutdown_privilege()?;
            let how = if action == "restart" { EWX_REBOOT } else { EWX_POWEROFF };
            unsafe { ExitWindowsEx(how | EWX_FORCEIFHUNG, SHTDN_REASON_FLAG_PLANNED | SHTDN_REASON_MAJOR_OTHER) }
                .map_err(|e| format!("{action} failed: {e}"))?;
        }
        _ => exit(),
    }
    Ok(())
}

fn enable_shutdown_privilege() -> Result<(), String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)
            .map_err(|e| format!("process token: {e}"))?;
        let mut luid = LUID::default();
        let looked_up = LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid);
        let result = looked_up.and_then(|()| {
            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
            };
            AdjustTokenPrivileges(token, false, Some(&tp as *const _), 0, None, None)
        });
        let _ = CloseHandle(token);
        result.map_err(|e| format!("shutdown privilege: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_actions_are_refused() {
        assert!(run("hibernate", || {}).is_err());
    }

    /// Run with PADWAKE_DRY_POWER=1 and PADWAKE_DATA set: each action only logs.
    #[test]
    #[ignore = "needs PADWAKE_DRY_POWER=1; otherwise it would really sleep, restart or shut down the PC"]
    fn real_power_dry_run_logs_only() {
        assert!(dry_run(), "set PADWAKE_DRY_POWER=1");
        let mut exited = false;
        for a in ACTIONS {
            run(a, || exited = true).unwrap();
        }
        assert!(!exited, "exit must not run in a dry run");
        let log = std::fs::read_to_string(crate::data::dir().join("padwake.log")).unwrap();
        for line in log.lines().filter(|l| l.contains("power:")).rev().take(4).collect::<Vec<_>>().iter().rev() {
            println!("{line}");
        }
    }
}
