//! Test hook, only with `PADWAKE_TEST=1` (otherwise no thread and no pipe exist): a virtual pad driven through the
//! named pipe `\\.\pipe\padwake-test`, merged into the poller like a fifth slot, so Guide, buttons and sticks
//! behave exactly as a real pad's would (tap/hold timing, mouse mode, the page). One command per line:
//!   guide tap | guide hold           Guide down for 120 ms / 800 ms
//!   press <button> [ms]              a b x y lb rb lt rt menu view guide up down left right l3 r3, for ms (default 80)
//!   pad {"rx": 0.6, "a": true}       hold this state until the next `pad` line (sticks -1..1, y down); `pad {}` lets go
//!   report ds5 usb 01807f...          one raw HID input report (hex, report ID first) from a PlayStation (ds4, ds5) or
//!                                    Switch (pro) pad on usb or bt, through `hid.rs`'s parser as pad 5; `report off`
//!                                    takes that pad away. The plain test pad stands in for an Xbox pad.
//! Client: `tools/app/pad.ps1 'guide tap'`.

use std::io::{BufRead, BufReader};
use std::os::windows::io::FromRawHandle;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};

/// What the virtual pad adds this poll: XInput button bits, triggers, and stick steps (0.05 each, y down).
#[derive(Default, Clone, Copy, PartialEq)]
pub struct Virtual {
    pub buttons: u16,
    pub lt: bool,
    pub rt: bool,
    pub axes: [i8; 4],
}

#[derive(Default)]
struct Hook {
    held: Virtual,
    presses: Vec<(u16, Instant)>,
}

static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

pub fn enabled() -> bool {
    std::env::var("PADWAKE_TEST").is_ok_and(|v| v == "1")
}

/// Starts the pipe server when the hook is enabled.
pub fn start() {
    if !enabled() {
        return;
    }
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Hook::default());
    let spawned = std::thread::Builder::new().name("test-pad".into()).spawn(serve);
    if let Err(e) = spawned {
        eprintln!("test pad: {e}");
    }
    crate::system::log("input: test hook on (\\\\.\\pipe\\padwake-test)");
}

/// The virtual pad's state now; None when the hook is off or adds nothing.
pub fn state(now: Instant) -> Option<Virtual> {
    let mut guard = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let hook = guard.as_mut()?;
    hook.presses.retain(|(_, until)| *until > now);
    let mut v = hook.held;
    for (mask, _) in &hook.presses {
        v.buttons |= mask;
    }
    let idle = v.buttons == 0 && !v.lt && !v.rt && v.axes == [0; 4];
    (!idle).then_some(v)
}

fn serve() {
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                w!(r"\\.\pipe\padwake-test"),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                0,
                4096,
                0,
                None,
            )
        };
        if pipe.is_invalid() {
            eprintln!("test pad: no pipe");
            return;
        }
        // blocks until a client connects (an error here can mean it connected first, which is fine)
        let _ = unsafe { ConnectNamedPipe(pipe, None) };
        let file = unsafe { std::fs::File::from_raw_handle(pipe.0) };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Err(e) = command(line.trim(), Instant::now()) {
                crate::system::log(&format!("input: test hook: {e}"));
            }
        }
        // the File closed the handle; a new pipe instance takes the next client
    }
}

fn bit(name: &str) -> Option<u16> {
    Some(match name {
        "up" => 0x0001,
        "down" => 0x0002,
        "left" => 0x0004,
        "right" => 0x0008,
        "menu" => 0x0010,
        "view" => 0x0020,
        "l3" => 0x0040,
        "r3" => 0x0080,
        "lb" => 0x0100,
        "rb" => 0x0200,
        "guide" => 0x0400,
        "a" => 0x1000,
        "b" => 0x2000,
        "x" => 0x4000,
        "y" => 0x8000,
        _ => return None,
    })
}

fn command(line: &str, now: Instant) -> Result<(), String> {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    if verb == "report" {
        return report(rest); // hid.rs keeps that pad, under its own lock
    }
    let mut guard = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let hook = guard.as_mut().ok_or("hook off")?;
    match verb {
        "" => {}
        "guide" => {
            let ms = match rest.trim() {
                "tap" => 120,
                "hold" => 800,
                other => return Err(format!("guide {other}?")),
            };
            hook.presses.push((0x0400, now + Duration::from_millis(ms)));
        }
        "press" => {
            let mut words = rest.split_whitespace();
            let name = words.next().ok_or("press what?")?;
            let ms = words.next().and_then(|m| m.parse().ok()).unwrap_or(80);
            let mask = bit(name).ok_or_else(|| format!("no button {name}"))?;
            hook.presses.push((mask, now + Duration::from_millis(ms)));
        }
        "pad" => {
            let v: serde_json::Value = serde_json::from_str(if rest.trim().is_empty() { "{}" } else { rest })
                .map_err(|e| format!("pad: {e}"))?;
            let mut held = Virtual::default();
            for (k, val) in v.as_object().ok_or("pad wants an object")? {
                match k.as_str() {
                    "lt" => held.lt = val.as_bool().unwrap_or(false),
                    "rt" => held.rt = val.as_bool().unwrap_or(false),
                    "lx" | "ly" | "rx" | "ry" => {
                        let i = ["lx", "ly", "rx", "ry"].iter().position(|a| a == k).unwrap_or(0);
                        held.axes[i] = (val.as_f64().unwrap_or(0.0).clamp(-1.0, 1.0) * 20.0).round() as i8;
                    }
                    name => {
                        let mask = bit(name).ok_or_else(|| format!("no button {name}"))?;
                        if val.as_bool().unwrap_or(false) {
                            held.buttons |= mask;
                        }
                    }
                }
            }
            hook.held = held;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// `report <ds4|ds5|pro> <usb|bt> <hex>` or `report off`.
fn report(rest: &str) -> Result<(), String> {
    use super::hid::{feed_test, Model};
    let mut words = rest.split_whitespace();
    let model = match words.next() {
        Some("off") => {
            feed_test(None);
            return Ok(());
        }
        Some("ds4") => Model::DualShock4,
        Some("ds5") => Model::DualSense,
        Some("pro") => Model::SwitchPro,
        other => return Err(format!("report: which pad? {other:?}")),
    };
    let bluetooth = match words.next() {
        Some("usb") => false,
        Some("bt") => true,
        other => return Err(format!("report: usb or bt? {other:?}")),
    };
    let hex = words.next().ok_or("report: no bytes")?;
    if hex.len() % 2 != 0 {
        return Err("report: odd hex".into());
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("report: {e}")))
        .collect::<Result<Vec<u8>, _>>()?;
    feed_test(Some((model, bluetooth, &bytes)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_drive_the_virtual_pad() {
        *HOOK.lock().unwrap() = Some(Hook::default());
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(state(t0).is_none());
        command("guide tap", t0).unwrap();
        assert_eq!(state(ms(100)).unwrap().buttons, 0x0400);
        assert!(state(ms(121)).is_none(), "released after 120 ms");
        command("press a 50", t0).unwrap();
        assert_eq!(state(ms(10)).unwrap().buttons, 0x1000);
        command(r#"pad {"rx": 0.62, "ly": -1, "b": true}"#, t0).unwrap();
        let v = state(ms(500)).unwrap();
        assert_eq!((v.axes, v.buttons), ([0, -20, 12, 0], 0x2000));
        command("pad {}", t0).unwrap();
        assert!(state(ms(500)).is_none());
        assert!(command("press nothing", t0).is_err());
        *HOOK.lock().unwrap() = None;
        assert!(state(t0).is_none(), "inert when off");
    }

    #[test]
    fn a_report_drives_a_pad_through_the_hid_parser() {
        use crate::input::hid_pads;
        use super::super::hid::Model;
        let t0 = Instant::now();
        // a Switch Pro on Bluetooth, simple report: B (south) down, sticks at rest
        let b_down = "3f01000800800080008000800000000000";
        command(&format!("report pro bt {b_down}"), t0).unwrap();
        let ours = || hid_pads().into_iter().find(|p| p.model == Model::SwitchPro && p.bluetooth && p.buttons == 0x1000);
        let p = ours().expect("the test pad reads B as A");
        assert!(p.slot >= 4, "a HID slot");
        command("report off", t0).unwrap();
        assert!(ours().is_none(), "report off takes it away");
        assert!(command("report joycon bt 3f", t0).is_err());
        assert!(command("report ds5 wifi 01", t0).is_err());
        assert!(command("report ds5 usb 0", t0).is_err(), "odd hex");
        assert!(command("report ds5 usb zz", t0).is_err());
    }
}
