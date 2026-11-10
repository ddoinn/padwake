use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::hid::Family;
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
/// A muted XInput mirror that moves while every PlayStation / Switch pad has been still this long, and stays the only
/// one moving for `MIRROR_GRACE` (the HID pad's own report may reach us after the mirror's echo), is a pad of its own.
const MIRROR_QUIET: Duration = Duration::from_millis(500);
const MIRROR_GRACE: Duration = Duration::from_millis(100);
/// The XInput slots the poller leaves out now (bit per slot), for Settings' pad list.
static MUTED: AtomicU8 = AtomicU8::new(0);

/// Whether XInput slot `slot` is a mirror of a PlayStation or Switch pad, left out now.
pub fn muted(slot: u32) -> bool {
    slot < 4 && MUTED.load(Ordering::Relaxed) & (1 << slot) != 0
}

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
    /// the family of the pad used last ("xbox", "playstation", "switch"), for the page's button hints (row C1)
    pub(crate) family: &'static str,
}

static LAST_PAD: AtomicU32 = AtomicU32::new(0);
static LAST_FAMILY: AtomicU8 = AtomicU8::new(0);

fn set_last(slot: Option<u32>, family: Family) {
    if let Some(slot) = slot {
        LAST_PAD.store(slot, Ordering::Relaxed);
    }
    LAST_FAMILY.store(family as u8, Ordering::Relaxed);
}

/// The family of the pad used last.
pub fn last_family() -> Family {
    match LAST_FAMILY.load(Ordering::Relaxed) {
        x if x == Family::Playstation as u8 => Family::Playstation,
        x if x == Family::Switch as u8 => Family::Switch,
        _ => Family::Xbox,
    }
}
/// When the pad state last changed (any pad, or the test pad), as Unix ms: the session log's idle check (row B1b).
static LAST_CHANGE: AtomicU64 = AtomicU64::new(0);

/// The slot of the pad used last: 0–3 XInput, 4 and up a PlayStation or Switch pad (`hid.rs`).
pub fn last_pad() -> u32 {
    LAST_PAD.load(Ordering::Relaxed)
}

/// When any pad's buttons or sticks last moved, as Unix ms (0: not since the start).
pub fn last_change() -> u64 {
    LAST_CHANGE.load(Ordering::Relaxed)
}
static RUMBLE_UNTIL: Mutex<Option<(u32, Instant)>> = Mutex::new(None);

pub fn rumble(strength: f32, ms: u32) {
    let pad = LAST_PAD.load(Ordering::Relaxed);
    if pad >= 4 {
        return; // PlayStation and Switch rumble: later (row C1)
    }
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
pub(super) fn quantize(prev: i8, v: f32) -> i8 {
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

    /// This press was part of a chord: when it ends it is neither a tap nor a hold.
    fn swallow(&mut self, now: Instant) {
        self.since = Some(now);
        self.held = true;
    }
}

/// XInput slots that are a PlayStation or Switch pad shown again (row C1): DS4Windows' emulated Xbox 360 pad
/// (ViGEmBus, 045E:028E) or Steam Input's virtual pad (Valve, 28DE). While such a pad is read over HID, those slots
/// are left out, so one press counts once (a mirror may also map buttons differently). A muted slot that moves while
/// every HID pad has been still for `MIRROR_QUIET`, and no HID pad moves in the `MIRROR_GRACE` after, is a real pad
/// after all (a wired Xbox 360 pad has the same IDs): it counts from then until it disconnects.
#[derive(Default)]
struct Mirrors {
    ids: [Option<(u16, u16)>; 4],
    freed: [bool; 4],
    /// when it moved alone, waiting out the grace
    alone: [Option<Instant>; 4],
}

fn looks_virtual((vid, pid): (u16, u16)) -> bool {
    vid == 0x28DE || (vid, pid) == (0x045E, 0x028E)
}

impl Mirrors {
    /// Slot `i` just connected, with these USB IDs (from XInputGetCapabilitiesEx).
    fn connected(&mut self, i: usize, ids: Option<(u16, u16)>) {
        self.ids[i] = ids;
        self.freed[i] = false;
        self.alone[i] = None;
    }

    /// Whether slot `i` is left out this poll. `hid_on`: a PlayStation or Switch pad is connected; `hid_changed`: when
    /// any of them last changed; `moved`: slot `i` changed this poll.
    fn muted(&mut self, i: usize, hid_on: bool, hid_changed: Option<Instant>, moved: bool, now: Instant) -> bool {
        if !hid_on || self.freed[i] || !self.ids[i].is_some_and(looks_virtual) {
            return false;
        }
        let quiet = hid_changed.is_none_or(|t| now.saturating_duration_since(t) >= MIRROR_QUIET);
        if moved && quiet && self.alone[i].is_none() {
            self.alone[i] = Some(now);
        }
        if let Some(t) = self.alone[i] {
            if hid_changed.is_some_and(|h| h >= t) {
                self.alone[i] = None; // the HID pad's report came after its echo: a mirror
            } else if now.saturating_duration_since(t) >= MIRROR_GRACE {
                self.alone[i] = None;
                self.freed[i] = true;
                return false;
            }
        }
        true
    }
}

/// Guide and View are both down now, and weren't both down last time: the screenshot chord, in either order.
fn chord(pad: &Pad, last: &Pad) -> bool {
    pad.guide && pad.view && !(last.guide && last.view)
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
    let mut mirrors = Mirrors::default();
    let mut slot_state = [(0u16, false, false, [0i8; 4]); 4];
    let mut hid_seqs: Vec<(u32, u32)> = Vec::new();
    let mut hid_changed: Option<Instant> = None;
    let mut last_virt = None;
    let mut muted_now = 0u8;

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
        let merge_axes = |axes: &mut [i8; 4], steps: [i8; 4]| {
            for (k, step) in steps.into_iter().enumerate() {
                if step.unsigned_abs() > axes[k].unsigned_abs() {
                    axes[k] = step;
                }
            }
        };

        // PlayStation and Switch pads (hid.rs), first: whether one just changed decides the XInput mirrors below
        let hids = super::hid::pads();
        hid_seqs.retain(|(slot, _)| hids.iter().any(|p| p.slot == *slot));
        for p in &hids {
            let seen = hid_seqs.iter_mut().find(|(slot, _)| *slot == p.slot);
            let prev = seen.as_ref().map_or(0, |(_, seq)| *seq);
            if p.seq != prev {
                set_last(Some(p.slot), p.model.family());
                hid_changed = Some(now);
            }
            match seen {
                Some(entry) => entry.1 = p.seq,
                None => hid_seqs.push((p.slot, p.seq)),
            }
            buttons |= p.buttons;
            lt |= p.lt;
            rt |= p.rt;
            merge_axes(&mut axes, p.axes);
        }
        let mut muted_bits = 0u8;
        for i in 0..4 {
            if !connected[i] && !probe {
                continue;
            }
            let mut st = RawState::default();
            let was = connected[i];
            connected[i] = unsafe { get(i as u32, &mut st) } == 0;
            if !connected[i] {
                sticks[i] = [0; 4];
                slot_state[i] = Default::default(); // a pad connects at rest: only a real move frees a mirror
                continue;
            }
            if !was {
                mirrors.connected(i, crate::system::xinput_ids(i as u32));
            }
            let raw = [axis(st.lx), -axis(st.ly), axis(st.rx), -axis(st.ry)];
            for (k, v) in raw.into_iter().enumerate() {
                sticks[i][k] = quantize(sticks[i][k], v);
            }
            let state = (st.buttons, st.lt > TRIGGER_THRESHOLD, st.rt > TRIGGER_THRESHOLD, sticks[i]);
            let moved = state != slot_state[i];
            slot_state[i] = state;
            if mirrors.muted(i, !hids.is_empty(), hid_changed, moved, now) {
                muted_bits |= 1 << i;
                continue;
            }
            if st.packet != packets[i] {
                packets[i] = st.packet;
                set_last(Some(i as u32), Family::Xbox);
            }
            buttons |= state.0;
            lt |= state.1;
            rt |= state.2;
            merge_axes(&mut axes, state.3);
        }

        if muted_bits != muted_now {
            muted_now = muted_bits;
            MUTED.store(muted_now, Ordering::Relaxed);
        }

        // PADWAKE_TEST=1: the test hook's virtual pad, merged like a fifth slot
        let virt = super::test_hook::state(now);
        if let Some(v) = &virt {
            buttons |= v.buttons;
            lt |= v.lt;
            rt |= v.rt;
            merge_axes(&mut axes, v.axes);
            // the test pad stands in for an Xbox pad (its `report` command drives a PlayStation or Switch one)
            if last_virt != virt {
                set_last(None, Family::Xbox);
            }
        }
        last_virt = virt;

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
            family: last_family().as_str(),
        };

        // mouse mode (system area) takes the whole pad while it is on: nothing reaches the page
        let taken = crate::system::mouse::take(&app, &pad, now);
        if !taken {
            // Guide down over a running game: the games area copies the screen now, for a tap known at the release
            if pad.guide && !last.guide {
                crate::games::guide_down(&app);
            }
            // Guide + View while a game is in front: a screenshot. View alone stays the game's (maps, scoreboards), and
            // XInput can't hide a press from it. `shown`: the launch screen is up, or the page handles the buttons
            if !shown && chord(&pad, &last) && crate::games::screenshot(&app) {
                guide.swallow(now);
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
        if pad != last {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            LAST_CHANGE.store(ms, Ordering::Relaxed);
        }
        last = pad;
        was_shown = shown && !taken;

        stop_rumble_when_due(now);
        // Rust's sleep uses a high-resolution waitable timer on Windows, so 4 ms is 4 ms
        let active = connected.contains(&true) || virt.is_some() || !hids.is_empty();
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
    fn a_virtual_xinput_pad_is_muted_while_a_hid_pad_is_read() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        // the HID pad last changed at 1000 ms: "busy" polls are right after, "quiet" ones 5 s later
        let hid = Some(ms(1000));
        let (busy, quiet) = (ms(1004), ms(6000));
        let mut m = Mirrors::default();
        m.connected(0, Some((0x045E, 0x028E))); // DS4Windows' emulated Xbox 360 pad
        m.connected(1, Some((0x28DE, 0x11FF))); // Steam Input's virtual pad
        m.connected(2, Some((0x045E, 0x0B12))); // an Xbox Series pad
        m.connected(3, None);
        // no PlayStation / Switch pad: nothing is muted
        assert!((0..4).all(|i| !m.muted(i, false, hid, true, busy)));
        // with one: both mirrors are left out, from their very first press (it echoes the HID pad's)
        assert!(m.muted(0, true, hid, true, busy), "the mirror's echo of a press");
        assert!(m.muted(1, true, hid, true, busy), "Steam's echo");
        assert!(m.muted(0, true, hid, false, quiet), "a mirror at rest stays muted");
        assert!(!m.muted(2, true, hid, true, busy), "a real Xbox pad still counts beside a DualSense");
        assert!(!m.muted(3, true, hid, true, busy), "IDs unknown: it counts");
        // the echo arrives one poll before the HID pad's own report: still a mirror
        let t = ms(6000);
        assert!(m.muted(0, true, hid, true, t), "moved alone: muted through the grace");
        let hid2 = Some(ms(6004));
        assert!(m.muted(0, true, hid2, false, ms(6004)), "the HID pad moved right after: an echo");
        assert!(m.muted(0, true, hid2, false, ms(6200)), "and it stays muted after the grace");
        // the 045E:028E slot moves on its own, and no HID pad follows: a real wired 360 pad
        assert!(m.muted(0, true, hid2, true, ms(7000)), "alone, in the grace");
        assert!(m.muted(0, true, hid2, false, ms(7050)));
        assert!(!m.muted(0, true, hid2, false, ms(7100)), "the grace passed with the HID pad still");
        assert!(!m.muted(0, true, Some(ms(7200)), true, ms(7204)), "and it keeps counting");
        // until it reconnects
        m.connected(0, Some((0x045E, 0x028E)));
        assert!(m.muted(0, true, Some(ms(7200)), true, ms(7204)));
        // Steam's pad is muted however long the HID pad is still, as long as it doesn't move
        assert!(m.muted(1, true, hid, false, quiet));
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

    #[test]
    fn screenshot_needs_guide_and_view_together() {
        let (none, view, guide, both) = (
            Pad::default(),
            Pad { view: true, ..Pad::default() },
            Pad { guide: true, ..Pad::default() },
            Pad { guide: true, view: true, ..Pad::default() },
        );
        assert!(!chord(&view, &none), "View alone belongs to the game");
        assert!(!chord(&guide, &none), "Guide alone is a tap or a hold");
        assert!(chord(&both, &guide), "Guide, then View");
        assert!(chord(&both, &view), "View, then Guide");
        assert!(chord(&both, &none), "both in one poll");
        assert!(!chord(&both, &both), "one shot per press, however long it is held");
    }

    #[test]
    fn a_chord_is_neither_a_tap_nor_a_hold() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut g = Guide::default();
        assert_eq!(g.update(true, ms(0)), None);
        g.swallow(ms(100));
        assert_eq!(g.update(true, ms(100)), None);
        assert_eq!(g.update(true, ms(900)), None, "no hold");
        assert_eq!(g.update(false, ms(1000)), None, "no tap");

        // both pressed in the same poll: swallowed before Guide's first update
        g.swallow(ms(2000));
        assert_eq!(g.update(true, ms(2000)), None);
        assert_eq!(g.update(false, ms(2050)), None);

        // and the next press is an ordinary tap
        assert_eq!(g.update(true, ms(3000)), None);
        assert_eq!(g.update(false, ms(3100)), Some("tap"));
    }
}
