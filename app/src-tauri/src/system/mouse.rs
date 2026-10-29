//! Mouse mode (Control Center, row A7): the right stick moves the cursor, A clicks, X right-clicks and the left stick
//! scrolls; B or Guide ends it. Meanwhile no pad input reaches the page and the app sleeps, so another window (a store's
//! sign-in) can be used. The pad thread hands each reading to `take` first and polls at 4 ms while it is on.
//! Input goes through SendInput, so other apps see real mouse input; the cursor moves in absolute screen coordinates,
//! so Windows' pointer acceleration doesn't stack on ours.

use crate::input::Pad;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Runtime};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

/// Stick travel ignored around the centre.
const DEAD: f32 = 0.18;
/// Cursor speed at full tilt, in screen heights per second: a 1080 px screen crosses in about a second.
const SPEED: f32 = 1.0;
/// Held at full tilt, the speed ramps up to this many times SPEED, from 0.35 s to 1 s.
const BOOST: f32 = 2.2;
/// The cursor's velocity follows the stick with this time constant (s), so steps of the stick don't jerk it.
const SMOOTH: f32 = 0.05;
/// Scroll speed at full tilt, in wheel notches per second.
const WHEEL: f32 = 18.0;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum Mode {
    #[default]
    Off,
    On,
    /// ended; the pad stays ours until every button is up, so the press that ended it reaches nothing
    Draining,
}

#[derive(Default)]
struct Mouse {
    mode: Mode,
    /// buttons seen last time; None right after start, so a button already held then does nothing
    prev: Option<(bool, bool, bool, bool)>,
    left_down: bool,
    right_down: bool,
    last: Option<Instant>,
    full_since: Option<Instant>,
    v: (f32, f32),
    /// sub-pixel and sub-unit remainders
    rest: (f32, f32),
    wheel: (f32, f32),
}

static STATE: Mutex<Mouse> = Mutex::new(Mouse {
    mode: Mode::Off,
    prev: None,
    left_down: false,
    right_down: false,
    last: None,
    full_since: None,
    v: (0.0, 0.0),
    rest: (0.0, 0.0),
    wheel: (0.0, 0.0),
});

fn state() -> std::sync::MutexGuard<'static, Mouse> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts mouse mode: the app sleeps (with a game, the game comes to the front) and a toast says how to leave.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    {
        let mut s = state();
        if s.mode == Mode::On {
            return;
        }
        *s = Mouse { mode: Mode::On, ..Default::default() };
    }
    super::log("mouse mode: on");
    let _ = app.emit("mouse", serde_json::json!({ "on": true }));
    crate::games::to_background(app);
    super::overlay_toast(app, "Mouse mode", "B to exit.", Some("glyph:mouse".into()));
}

/// Ends mouse mode (B or Guide, or the page): buttons it holds are let go, and without a game the app comes back.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    let (left, right) = {
        let mut s = state();
        if s.mode != Mode::On {
            return;
        }
        s.mode = Mode::Draining;
        let held = (s.left_down, s.right_down);
        s.left_down = false;
        s.right_down = false;
        held
    };
    if left {
        send(&[mouse(0, 0, 0, MOUSEEVENTF_LEFTUP)]);
    }
    if right {
        send(&[mouse(0, 0, 0, MOUSEEVENTF_RIGHTUP)]);
    }
    super::log("mouse mode: off");
    let _ = app.emit("mouse", serde_json::json!({ "on": false }));
    crate::games::from_background(app);
}

/// One pad reading from the pad thread. True while mouse mode has the pad (the page gets nothing).
pub fn take<R: Runtime>(app: &AppHandle<R>, pad: &Pad, now: Instant) -> bool {
    let mode = state().mode;
    match mode {
        Mode::Off => false,
        Mode::Draining => {
            if !any_button(pad) {
                state().mode = Mode::Off;
            }
            true
        }
        Mode::On => {
            let buttons = (pad.a, pad.x, pad.b, pad.guide);
            let prev = state().prev.replace(buttons);
            let pressed = |now: bool, before: Option<bool>| now && before == Some(false);
            if pressed(pad.b, prev.map(|p| p.2)) || pressed(pad.guide, prev.map(|p| p.3)) {
                stop(app);
                return true;
            }
            step(pad, now, prev.map(|p| (p.0, p.1)));
            true
        }
    }
}

fn any_button(p: &Pad) -> bool {
    p.a || p.b || p.x || p.y || p.lb || p.rb || p.lt || p.rt || p.view || p.menu || p.guide || p.up || p.down || p.left
        || p.right || p.l3 || p.r3
}

/// The stick shaped for fine control: nothing inside DEAD, then a curve that is gentle near the centre.
fn shape(m: f32) -> f32 {
    if m <= DEAD {
        0.0
    } else {
        ((m - DEAD) / (1.0 - DEAD)).min(1.0).powf(2.2)
    }
}

/// Cursor velocity the stick asks for, in px/s (y down), with the full-tilt boost.
fn target(rx: f32, ry: f32, held_full: f32, screen_h: f32) -> (f32, f32) {
    let m = rx.hypot(ry);
    if m <= DEAD {
        return (0.0, 0.0);
    }
    let boost = 1.0 + (BOOST - 1.0) * ((held_full - 0.35) / 0.65).clamp(0.0, 1.0);
    let speed = shape(m) * SPEED * screen_h * boost;
    (rx / m * speed, ry / m * speed)
}

fn step(pad: &Pad, now: Instant, prev: Option<(bool, bool)>) {
    let mut out = Vec::new();
    {
        let mut s = state();
        let dt = s.last.map_or(0.0, |t| (now - t).as_secs_f32().min(0.05));
        s.last = Some(now);
        let full = pad.rx.hypot(pad.ry) > 0.9;
        if !full {
            s.full_since = None;
        } else if s.full_since.is_none() {
            s.full_since = Some(now);
        }
        let held_full = s.full_since.map_or(0.0, |t| (now - t).as_secs_f32());
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
        let (tx, ty) = target(pad.rx, pad.ry, held_full, screen_h);
        let k = 1.0 - (-dt / SMOOTH).exp();
        s.v.0 += (tx - s.v.0) * k;
        s.v.1 += (ty - s.v.1) * k;
        if tx == 0.0 && ty == 0.0 && s.v.0.hypot(s.v.1) < 5.0 {
            s.v = (0.0, 0.0);
            s.rest = (0.0, 0.0);
        }
        s.rest.0 += s.v.0 * dt;
        s.rest.1 += s.v.1 * dt;
        let (dx, dy) = (s.rest.0.trunc() as i32, s.rest.1.trunc() as i32);
        s.rest.0 -= dx as f32;
        s.rest.1 -= dy as f32;
        if dx != 0 || dy != 0 {
            if let Some(m) = move_to(dx, dy) {
                out.push(m);
            }
        }

        // left stick: scroll (y down = toward you = negative wheel), x = horizontal wheel
        let notch = |v: f32| shape(v.abs()) * v.signum() * WHEEL * 120.0;
        s.wheel.0 += notch(pad.lx) * dt;
        s.wheel.1 += -notch(pad.ly) * dt;
        if pad.lx.abs() <= DEAD {
            s.wheel.0 = 0.0;
        }
        if pad.ly.abs() <= DEAD {
            s.wheel.1 = 0.0;
        }
        let (wx, wy) = (s.wheel.0.trunc() as i32, s.wheel.1.trunc() as i32);
        s.wheel.0 -= wx as f32;
        s.wheel.1 -= wy as f32;
        if wy != 0 {
            out.push(mouse(0, 0, wy as u32, MOUSEEVENTF_WHEEL));
        }
        if wx != 0 {
            out.push(mouse(0, 0, wx as u32, MOUSEEVENTF_HWHEEL));
        }

        // A = left button, X = right button; down and up follow the button, so dragging works
        if let Some((a0, x0)) = prev {
            if pad.a && !a0 {
                out.push(mouse(0, 0, 0, MOUSEEVENTF_LEFTDOWN));
                s.left_down = true;
            } else if !pad.a && a0 && s.left_down {
                out.push(mouse(0, 0, 0, MOUSEEVENTF_LEFTUP));
                s.left_down = false;
            }
            if pad.x && !x0 {
                out.push(mouse(0, 0, 0, MOUSEEVENTF_RIGHTDOWN));
                s.right_down = true;
            } else if !pad.x && x0 && s.right_down {
                out.push(mouse(0, 0, 0, MOUSEEVENTF_RIGHTUP));
                s.right_down = false;
            }
        }
    }
    if !out.is_empty() {
        send(&out);
    }
}

/// An absolute move to the cursor plus (dx, dy), kept on the desktop.
fn move_to(dx: i32, dy: i32) -> Option<INPUT> {
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2));
        let x = (p.x + dx).clamp(vx, vx + vw - 1);
        let y = (p.y + dy).clamp(vy, vy + vh - 1);
        let nx = ((x - vx) as f64 * 65535.0 / f64::from(vw - 1)).round() as i32;
        let ny = ((y - vy) as f64 * 65535.0 / f64::from(vh - 1)).round() as i32;
        Some(mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK))
    }
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stick_has_a_deadzone_and_a_gentle_centre() {
        assert_eq!(target(0.1, 0.1, 0.0, 1080.0), (0.0, 0.0));
        let (slow, _) = target(0.4, 0.0, 0.0, 1080.0);
        let (fast, _) = target(1.0, 0.0, 0.0, 1080.0);
        assert!(slow > 0.0 && slow < fast * 0.15, "{slow} vs {fast}");
        assert!((fast - 1080.0).abs() < 1.0);
        let (_, down) = target(0.0, 1.0, 0.0, 1080.0);
        assert!(down > 0.0, "stick y is screen direction (down)");
    }

    #[test]
    fn holding_full_tilt_speeds_up() {
        let (v0, _) = target(1.0, 0.0, 0.2, 1080.0);
        let (v1, _) = target(1.0, 0.0, 0.7, 1080.0);
        let (v2, _) = target(1.0, 0.0, 3.0, 1080.0);
        assert_eq!(v0, 1080.0);
        assert!(v1 > v0 && v1 < v2);
        assert!((v2 - 1080.0 * BOOST).abs() < 1.0);
    }
}
