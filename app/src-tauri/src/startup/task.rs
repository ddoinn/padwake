//! "Start with Windows": a scheduled task named exactly "Padwake" that starts this exe when the current user
//! signs in. Made and removed with schtasks.exe; nothing here ever names another task.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const TASK: &str = "Padwake";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> Result<Output, String> {
    let exe = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("schtasks.exe"))
        .unwrap_or_else(|| PathBuf::from("schtasks.exe"));
    Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| format!("schtasks: {e}"))
}

fn text(o: &Output) -> String {
    let s = String::from_utf8_lossy(&o.stderr).trim().to_string();
    if s.is_empty() {
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    } else {
        s
    }
}

pub fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK]).is_ok_and(|o| o.status.success())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The task: a logon trigger for `user` only (so creating it needs no admin rights), run as that user with limited
/// rights while they're signed in, normal priority, no time limit, one instance.
pub fn task_xml(user: &str, exe: &Path) -> String {
    let user = xml_escape(user);
    let command = xml_escape(&exe.display().to_string());
    let dir = xml_escape(&exe.parent().map(|p| p.display().to_string()).unwrap_or_default());
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Padwake when you sign in to Windows.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn current_user() -> Result<String, String> {
    let name = std::env::var("USERNAME").map_err(|_| "USERNAME isn't set")?;
    Ok(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    })
}

/// Creates (`on`) or deletes the task; returns whether it exists afterwards.
pub fn set(on: bool) -> Result<bool, String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| format!("this exe: {e}"))?;
        let xml = task_xml(&current_user()?, &exe);
        // schtasks reads the XML as UTF-16 (BOM first); the file goes in the data dir, never %TEMP% on C:
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let file = crate::data::dir().join("padwake-task.xml");
        std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        let out = schtasks(&["/Create", "/TN", TASK, "/XML", &file.display().to_string(), "/F"]);
        let _ = std::fs::remove_file(&file);
        let out = out?;
        if !out.status.success() {
            return Err(format!("couldn't add the start-up task: {}", text(&out)));
        }
        crate::system::log(&format!("startup: task \"{TASK}\" created for {}", exe.display()));
    } else if exists() {
        let out = schtasks(&["/Delete", "/TN", TASK, "/F"])?;
        if !out.status.success() {
            return Err(format!("couldn't remove the start-up task: {}", text(&out)));
        }
        crate::system::log(&format!("startup: task \"{TASK}\" deleted"));
    }
    Ok(exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_is_a_per_user_logon_task_for_this_exe() {
        let xml = task_xml(r"DESKTOP\Player", Path::new(r"W:\Apps\R&D\padwake.exe"));
        assert!(xml.contains("<LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>DESKTOP\\Player</UserId>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(r"<Command>W:\Apps\R&amp;D\padwake.exe</Command>"));
        assert!(xml.contains(r"<WorkingDirectory>W:\Apps\R&amp;D</WorkingDirectory>"));
    }

    fn get_scheduled_task() -> String {
        let ps = "$t = Get-ScheduledTask -TaskName 'Padwake' -ErrorAction SilentlyContinue; \
                  if ($t) { \"$($t.TaskPath)$($t.TaskName) state=$($t.State) exec=$($t.Actions[0].Execute) trigger=$($t.Triggers[0].CimClass.CimClassName) user=$($t.Triggers[0].UserId)\" } \
                  else { 'Get-ScheduledTask: no task named Padwake' }";
        let o = Command::new("powershell").args(["-NoProfile", "-Command", ps]).output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// Real effect: the task appears, then goes. It runs this test exe, never starts it, and touches no other task.
    #[test]
    #[ignore = "real effect: creates and deletes the scheduled task \"Padwake\""]
    fn real_startup_task_create_and_delete() {
        println!("before: {}", get_scheduled_task());
        assert!(!exists(), "a \"Padwake\" task already exists; not touching it");
        assert!(set(true).unwrap());
        println!("created: {}", get_scheduled_task());
        assert!(!set(false).unwrap());
        println!("after:  {}", get_scheduled_task());
        assert!(!exists());
    }
}
