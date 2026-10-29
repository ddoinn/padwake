//! Game Bar's claim on the Guide button. Windows opens Xbox Game Bar on a Guide press unless
//! `HKCU\Software\Microsoft\GameBar\UseNexusForGameBarEnabled` is 0 (Settings > Gaming > Xbox Game Bar, "Open Xbox
//! Game Bar using this button on a controller"). At each start: if the value is there and not 0, it becomes 0; if it
//! isn't there but Game Bar is installed (a missing value means on), it is written as 0; without Game Bar nothing is
//! touched. Every case is logged.

use crate::system::log;
use windows::core::{w, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    REG_DWORD, RRF_RT_REG_DWORD,
};

#[derive(Debug, PartialEq)]
pub enum Step {
    AlreadyOff,
    TurnOff(u32),
    Write,
    NoGameBar,
}

/// What to do, from the value (if any) and whether Game Bar is installed.
pub fn step(value: Option<u32>, installed: bool) -> Step {
    match value {
        Some(0) => Step::AlreadyOff,
        Some(v) => Step::TurnOff(v),
        None if installed => Step::Write,
        None => Step::NoGameBar,
    }
}

pub fn release_guide() {
    let s = step(value(), installed());
    let line = match s {
        Step::AlreadyOff => "Game Bar: UseNexusForGameBarEnabled is already 0, the Guide button is ours".to_string(),
        Step::NoGameBar => "Game Bar: not installed, nothing to change".to_string(),
        _ => match (set_off(), &s) {
            (false, _) => format!("Game Bar: couldn't set UseNexusForGameBarEnabled = 0 ({s:?})"),
            (true, Step::TurnOff(v)) => {
                format!("Game Bar: UseNexusForGameBarEnabled was {v}, set to 0 so the Guide button is ours")
            }
            (true, _) => "Game Bar: installed and UseNexusForGameBarEnabled missing (on), wrote 0 so the Guide button \
                          is ours"
                .to_string(),
        },
    };
    log(&line);
}

fn value() -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\GameBar"),
            w!("UseNexusForGameBarEnabled"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    (r == ERROR_SUCCESS).then_some(data)
}

fn set_off() -> bool {
    let zero = 0u32;
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\GameBar"),
            w!("UseNexusForGameBarEnabled"),
            REG_DWORD.0,
            Some(&zero as *const u32 as *const _),
            4,
        )
    };
    r == ERROR_SUCCESS
}

/// Game Bar (the Store package Microsoft.XboxGamingOverlay) is installed for this user.
fn installed() -> bool {
    const PACKAGES: windows::core::PCWSTR =
        w!(r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages");
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PACKAGES, None, KEY_READ, &mut key) } != ERROR_SUCCESS {
        return false;
    }
    let mut found = false;
    for i in 0.. {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        if String::from_utf16_lossy(&name[..len as usize]).starts_with("Microsoft.XboxGamingOverlay_") {
            found = true;
            break;
        }
    }
    let _ = unsafe { RegCloseKey(key) };
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_bar_steps() {
        assert_eq!(step(Some(0), true), Step::AlreadyOff);
        assert_eq!(step(Some(1), true), Step::TurnOff(1));
        assert_eq!(step(Some(1), false), Step::TurnOff(1), "the value is there: Windows reads it");
        assert_eq!(step(None, true), Step::Write);
        assert_eq!(step(None, false), Step::NoGameBar);
    }

    /// Reads only: what this PC has, and what a start would do.
    #[test]
    fn reads_this_pc() {
        let (v, i) = (value(), installed());
        println!("UseNexusForGameBarEnabled = {v:?}, Game Bar installed = {i}, step {:?}", step(v, i));
    }
}
