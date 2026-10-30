use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use windows::core::{s, w, PCSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Input::XboxController::{XInputSetState, XINPUT_VIBRATION};
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

// 4 ms only while a pad is connected and the window shows; otherwise ~30 Hz is enough (Guide over a game)
const POLL_FAST: Duration = Duration::from_millis(4);
const POLL_SLOW: Duration = Duration::from_millis(33);
// asking an empty slot is slow (XInput enumerates devices), so empty slots are rechecked once a second
const PROBE_EMPTY: Duration = Duration::from_secs(1);
const GUIDE_HOLD: Duration = Duration::from_millis(600);
const TRIGGER_THRESHOLD: u8 = 30; // XINPUT_GAMEPAD_TRIGGER_THRESHOLD

/// XInputGetStateEx fills the documented XINPUT_STATE plus a reserved dword.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct RawState {
    packet: u32,
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
    _reserved: u32,
}

type GetState = unsafe extern "system" fn(u32, *mut RawState) -> u32;

/// Pad state sent to the page. Sticks are -1..1 in steps of 0.05, x to the right and y down (screen direction).
#[derive(Serialize, Clone, Copy, PartialEq, Default)]
pub struct Pad {
    pub(crate) a: bool,
    pub(crate) b: bool,
    pub(crate) x: bool,
    pub(crate) y: bool,
    pub(crate) lb: bool,
    pub(crate) rb: bool,
    pub(crate) lt: bool,
    pub(crate) rt: bool,
    pub(crate) view: bool,
    pub(crate) menu: bool,
    pub(crate) guide: bool,
    pub(crate) up: bool,
    pub(crate) down: bool,
    pub(crate) left: bool,
    pub(crate) right: bool,
    pub(crate) l3: bool,
    pub(crate) r3: bool,
    pub(crate) lx: f32,
    pub(crate) ly: f32,
    pub(crate) rx: f32,
    pub(crate) ry: f32,
}

static LAST_PAD: AtomicU32 = AtomicU32::new(0);
static RUMBLE_UNTIL: Mutex<Option<(u32, Instant)>> = Mutex::new(None);

pub fn rumble(strength: f32, ms: u32) {
    let pad = LAST_PAD.load(Ordering::Relaxed);
    // the light, high-frequency motor alone feels like a tick
    let v = XINPUT_VIBRATION { wLeftMotorSpeed: 0, wRightMotorSpeed: (strength.clamp(0.0, 1.0) * 65535.0) as u16 };
    unsafe { XInputSetState(pad, &v) };
    let until = Instant::now() + Duration::from_millis(ms.into());
    *RUMBLE_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some((pad, until));
}

fn stop_rumble_when_due(now: Instant) {
    let mut due = RUMBLE_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((pad, until)) = *due {
        if now >= until {
            unsafe { XInputSetState(pad, &XINPUT_VIBRATION::default()) };
            *due = None;
        }
    }
}

fn load() -> Option<GetState> {
    unsafe {
        let dll = LoadLibraryW(w!("xinput1_4.dll")).ok()?;
        // ordinal 100 is XInputGetStateEx: XInputGetState plus the Guide button (0x0400)
        let f = GetProcAddress(dll, PCSTR(100 as *const u8)).or_else(|| GetProcAddress(dll, s!("XInputGetState")))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(f))
    }
}

pub fn start<R: Runtime>(app: AppHandle<R>) {
    let spawned = std::thread::Builder::new().name("xinput".into()).spawn(move || match load() {
        Some(get) => run(app, get),
        None => eprintln!("xinput1_4.dll is missing: no pad input"),
    });
    if let Err(e) = spawned {
        eprintln!("pad thread: {e}");
    }
}

/// Quantizes a -1..1 axis to 0.05 steps, holding the previous step until the value is clearly past the boundary,
/// so a resting stick never flickers between two steps.
fn quantize(prev: i8, v: f32) -> i8 {
    let steps = v * 20.0;
    if (steps - prev as f32).abs() < 0.7 {
        prev
    } else {
        steps.round() as i8
    }
}

fn axis(v: i16) -> f32 {
    (v as f32 / 32767.0).clamp(-1.0, 1.0)
}

/// Guide: released before 600 ms is a tap; held to 600 ms is a hold, reported once at 600 ms.
#[derive(Default)]
struct Guide {
    since: Option<Instant>,
    held: bool,
}

impl Guide {
    fn update(&mut self, down: bool, now: Instant) -> Option<&'static str> {
        match (down, self.since) {
            (true, None) => {
                self.since = Some(now);
                self.held = false;
                None
            }
            (true, Some(t)) if !self.held && now - t >= GUIDE_HOLD => {
                self.held = true;
                Some("hold")
            }
            (false, Some(_)) => {
                self.since = None;
                (!self.held).then_some("tap")
            }
            _ => None,
        }
    }
}

fn run<R: Runtime>(app: AppHandle<R>, get: GetState) {
    let mut hwnd: Option<isize> = None;
    let mut connected = [false; 4];
    let mut packets = [0u32; 4];
    let mut sticks = [[0i8; 4]; 4];
    let mut next_probe = Instant::now();
    let mut last = Pad::default();
    let mut was_shown = false;
    let mut guide = Guide::default();

    loop {
        let now = Instant::now();
        let shown = is_shown(&app, &mut hwnd);
        let probe = now >= next_probe;
        if probe {
            next_probe = now + PROBE_EMPTY;
        }

        let mut buttons = 0u16;
        let (mut lt, mut rt) = (false, false);
        let mut axes = [0i8; 4];
        for i in 0..4 {
            if !connected[i] && !probe {
                continue;
            }
            let mut st = RawState::default();
            connected[i] = unsafe { get(i as u32, &mut st) } == 0;
            if !connected[i] {
                sticks[i] = [0; 4];
                continue;
            }
            if st.packet != packets[i] {
                packets[i] = st.packet;
                LAST_PAD.store(i as u32, Ordering::Relaxed);
            }
            buttons |= st.buttons;
            lt |= st.lt > TRIGGER_THRESHOLD;
            rt |= st.rt > TRIGGER_THRESHOLD;
            let raw = [axis(st.lx), -axis(st.ly), axis(st.rx), -axis(st.ry)];
            for (k, v) in raw.into_iter().enumerate() {
                sticks[i][k] = quantize(sticks[i][k], v);
                if sticks[i][k].unsigned_abs() > axes[k].unsigned_abs() {
                    axes[k] = sticks[i][k];
                }
            }
        }

        // PADWAKE_TEST=1: the test hook's virtual pad, merged like a fifth slot
        let virt = super::test_hook::state(now);
        if let Some(v) = &virt {
            buttons |= v.buttons;
            lt |= v.lt;
            rt |= v.rt;
            for (k, step) in v.axes.into_iter().enumerate() {
                if step.unsigned_abs() > axes[k].unsigned_abs() {
                    axes[k] = step;
                }
            }
        }

        let b = |mask: u16| buttons & mask != 0;
        let pad = Pad {
            up: b(0x0001),
            down: b(0x0002),
            left: b(0x0004),
            right: b(0x0008),
            menu: b(0x0010),
            view: b(0x0020),
            l3: b(0x0040),
            r3: b(0x0080),
            lb: b(0x0100),
            rb: b(0x0200),
            guide: b(0x0400),
            a: b(0x1000),
            b: b(0x2000),
            x: b(0x4000),
            y: b(0x8000),
            lt,
            rt,
            lx: axes[0] as f32 / 20.0,
            ly: axes[1] as f32 / 20.0,
            rx: axes[2] as f32 / 20.0,
            ry: axes[3] as f32 / 20.0,
        };

        // mouse mode (system area) takes the whole pad while it is on: nothing reaches the page
        let taken = crate::system::mouse::take(&app, &pad, now);
        if !taken {
            // Guide down over a running game: the games area copies the screen now, for a tap known at the release
            if pad.guide && !last.guide {
                crate::games::guide_down(&app);
            }
            // View while a game is in front: a screenshot (the page is asleep then and gets nothing)
            if pad.view && !last.view {
                crate::games::view_pressed(&app);
            }
            // sent while hidden too: over a running game the games area wakes the window first and sends it itself
            if let Some(kind) = guide.update(pad.guide, now) {
                if !crate::games::guide(&app, kind) {
                    let _ = app.emit("guide", serde_json::json!({ "kind": kind }));
                }
            }

            // a hidden (suspended) page gets no input; it gets the current state when it shows again
            if shown && (pad != last || !was_shown) {
                let _ = app.emit("input", pad);
            }
        }
        last = pad;
        was_shown = shown && !taken;

        stop_rumble_when_due(now);
        // Rust's sleep uses a high-resolution waitable timer on Windows, so 4 ms is 4 ms
        let active = connected.contains(&true) || virt.is_some();
        std::thread::sleep(if taken || shown && active { POLL_FAST } else { POLL_SLOW });
    }
}

fn is_shown<R: Runtime>(app: &AppHandle<R>, hwnd: &mut Option<isize>) -> bool {
    if hwnd.is_none() {
        *hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize);
    }
    hwnd.is_some_and(|h| unsafe { IsWindowVisible(HWND(h as _)) }.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resting_stick_does_not_flicker_between_steps() {
        // noise around the 0 / 0.05 boundary (0.025) stays on the step it is on
        let mut q = 0;
        for v in [0.02, 0.03, 0.026, 0.034, 0.02] {
            q = quantize(q, v);
            assert_eq!(q, 0);
        }
        q = quantize(q, 0.05);
        assert_eq!(q, 1);
        q = quantize(q, 0.022);
        assert_eq!(q, 1);
        assert_eq!(quantize(q, 0.5), 10);
        assert_eq!(quantize(0, -1.0), -20);
    }

    #[test]
    fn guide_tap_and_hold() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut g = Guide::default();
        assert_eq!(g.update(true, ms(0)), None);
        assert_eq!(g.update(true, ms(300)), None);
        assert_eq!(g.update(false, ms(350)), Some("tap"));

        assert_eq!(g.update(true, ms(1000)), None);
        assert_eq!(g.update(true, ms(1599)), None);
        assert_eq!(g.update(true, ms(1600)), Some("hold"));
        assert_eq!(g.update(true, ms(2500)), None);
        assert_eq!(g.update(false, ms(2600)), None);
    }
}
