//! What a session logs beyond its time (row B1b), gathered without polling anything that costs:
//! - `Clock`: where the time went (the game, Home, Control Center or another app in front), moved by foreground
//!   events (`front.rs`), Guide, Back to game and the page's top screen.
//! - `Idle`: stretches of 5+ min with no pad, keyboard or mouse input, checked once a minute (the pad thread notes its
//!   last change; `GetLastInputInfo` has the keyboard's and mouse's).
//! - `snapshot`: the pad, display and power, as the session starts and ends.

use crate::library::sessions::{Display, Front, Pad, Power, Snapshot};
use std::time::{Duration, Instant};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// No input this long makes a stretch idle (all of it, from the last input).
pub const IDLE_AFTER_MS: u64 = 5 * 60 * 1000;
/// How often idle is checked (and the session under way saved) while a session runs.
pub const MINUTE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Game,
    Home,
    ControlCenter,
    Other,
}

/// Time by what was in front. Starts on the game (its launch screen counts as the game loading).
pub struct Clock {
    at: Where,
    since: Instant,
    spent: [Duration; 4],
}

impl Clock {
    pub fn new(now: Instant) -> Clock {
        Clock { at: Where::Game, since: now, spent: [Duration::ZERO; 4] }
    }

    /// Now `at` is in front. True if that's a change.
    pub fn to(&mut self, at: Where, now: Instant) -> bool {
        if at == self.at {
            return false;
        }
        self.spent[self.at as usize] += now.saturating_duration_since(self.since);
        (self.at, self.since) = (at, now);
        true
    }

    /// Seconds so far, each rounded.
    pub fn front(&self, now: Instant) -> Front {
        let mut spent = self.spent;
        spent[self.at as usize] += now.saturating_duration_since(self.since);
        let s = |w: Where| spent[w as usize].as_secs_f64().round() as u64;
        Front { game: s(Where::Game), home: s(Where::Home), control_center: s(Where::ControlCenter), other: s(Where::Other) }
    }
}

/// Idle time. Times are Unix milliseconds. A stretch that ended counts up to the last check that saw it idle, so it
/// never counts time after the player came back (it may miss up to a minute).
pub struct Idle {
    start: u64,
    total: u64,
    /// the stretch going on: since the last input before it
    open: Option<u64>,
    /// the last check that saw it idle
    seen: u64,
}

impl Idle {
    pub fn new(start: u64) -> Idle {
        Idle { start, total: 0, open: None, seen: start }
    }

    /// A check at `now`; `last_input` is the latest pad, keyboard or mouse input (before the session counts as its start).
    pub fn check(&mut self, now: u64, last_input: u64) {
        let last = last_input.max(self.start);
        if let Some(from) = self.open {
            if last > from {
                self.total += self.seen.saturating_sub(from);
                self.open = None;
            }
        }
        if now.saturating_sub(last) >= IDLE_AFTER_MS {
            self.open.get_or_insert(last);
            self.seen = now;
        }
    }

    /// Idle seconds up to `now`, counting a stretch still going on.
    pub fn secs(&self) -> u64 {
        let open = self.open.map_or(0, |from| self.seen.saturating_sub(from));
        (self.total + open) / 1000
    }
}

/// The latest keyboard or mouse input anywhere, as Unix ms.
pub fn last_key_or_mouse(now: u64) -> u64 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // both are the 32-bit tick count, which wraps every 49.7 days
    let ago = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
    now.saturating_sub(u64::from(ago))
}

/// The pad, display and power now. `slot` is the pad used last. `ran`: the display as the session ran, read before
/// its display profile was put back (row B2); otherwise the display now.
pub fn snapshot(slot: u32, ran: Option<(crate::system::Mode, bool)>) -> Snapshot {
    use crate::system::Connection;
    let pads = crate::system::pads();
    let pad = pads.iter().find(|p| p.slot == slot).or(pads.first()).map(|p| {
        let link = match p.connection {
            Connection::Wired => "wired",
            Connection::Wireless => "wireless",
            Connection::Receiver => "receiver",
        };
        pad(link, p.kind, p.level, p.name.clone())
    });
    let display = ran.or_else(crate::system::display_now).map(|(m, hdr)| Display { width: m.width, height: m.height, hz: m.hz, hdr });
    let mut st = SYSTEM_POWER_STATUS::default();
    let power = unsafe { GetSystemPowerStatus(&mut st) }.ok().and_then(|_| power(st.ACLineStatus, st.BatteryFlag, st.BatteryLifePercent));
    Snapshot { pad, display, power }
}

/// A pad's line: XInput's battery `kind` and `level` (0–3) become a word, nothing for a wired pad.
fn pad(connection: &str, kind: &str, level: u8, name: Option<String>) -> Pad {
    let battery = (kind != "wired").then(|| ["empty", "low", "medium", "full"][level.min(3) as usize].to_string());
    Pad { name, connection: connection.to_string(), battery }
}

/// From `SYSTEM_POWER_STATUS`: AC line 1 is mains, 0 battery, 255 unknown; battery flag 128 is no battery; a percent of
/// 255 is unknown.
fn power(ac_line: u8, flag: u8, percent: u8) -> Option<Power> {
    let ac = match ac_line {
        0 => false,
        1 => true,
        _ => return None,
    };
    let battery = (flag & 128 == 0 && percent <= 100).then_some(percent);
    Some(Power { ac, battery })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_splits_the_time_by_what_was_in_front() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_millis(s);
        let mut c = Clock::new(t0);
        c.to(Where::Game, at(1_000)); // the game's window came up: still the game
        c.to(Where::ControlCenter, at(10_000)); // Guide tap
        c.to(Where::Home, at(13_000)); // B to Home over the game
        c.to(Where::Game, at(15_400)); // Resume
        c.to(Where::Other, at(20_000)); // alt-tab
        c.to(Where::Game, at(26_600));
        c.to(Where::Game, at(28_000)); // still the game: nothing moves
        assert_eq!(c.front(at(30_000)), Front { game: 18, home: 2, control_center: 3, other: 7 });
        assert_eq!(c.front(at(30_000)), c.front(at(30_000)), "reading it changes nothing");
    }

    #[test]
    fn idle_is_only_stretches_of_five_minutes() {
        let m = 60_000;
        let mut i = Idle::new(0);
        // input every few minutes: never idle
        for t in 1..=4 {
            i.check(t * m, t * m - 10_000);
        }
        assert_eq!(i.secs(), 0);
        // the last input at 4 min; 4:59 later isn't idle yet, 5 min later is, and it counts from the input
        i.check(8 * m + 59_000, 4 * m);
        assert_eq!(i.secs(), 0);
        i.check(9 * m, 4 * m);
        assert_eq!(i.secs(), 300);
        i.check(12 * m, 4 * m);
        assert_eq!(i.secs(), 480, "still going: up to this check");
        // the player comes back at 12:30; the next check closes it at the last check that saw it idle
        i.check(13 * m, 12 * m + 30_000);
        assert_eq!(i.secs(), 480);
        // and a second stretch adds up
        i.check(18 * m, 12 * m + 30_000);
        assert_eq!(i.secs(), 480 + 330);
    }

    #[test]
    fn input_before_the_session_counts_from_its_start() {
        let mut i = Idle::new(1_000_000);
        i.check(1_000_000 + IDLE_AFTER_MS - 1, 0);
        assert_eq!(i.secs(), 0, "an old input doesn't make the session idle from its first minute");
        i.check(1_000_000 + IDLE_AFTER_MS, 0);
        assert_eq!(i.secs(), 300);
    }

    #[test]
    fn a_pad_line_has_a_battery_word_unless_wired() {
        let name = Some("Xbox Wireless Controller".to_string());
        assert_eq!(
            pad("wireless", "nimh", 2, name.clone()),
            Pad { name: name.clone(), connection: "wireless".into(), battery: Some("medium".into()) }
        );
        assert_eq!(pad("wireless", "alkaline", 0, None).battery.as_deref(), Some("empty"));
        assert_eq!(pad("receiver", "unknown", 3, None).battery.as_deref(), Some("full"));
        assert_eq!(pad("wired", "wired", 0, name).battery, None);
    }

    #[test]
    fn power_is_mains_or_battery_with_its_percent() {
        assert_eq!(power(1, 8, 100), Some(Power { ac: true, battery: Some(100) }), "charging, full");
        assert_eq!(power(0, 2, 23), Some(Power { ac: false, battery: Some(23) }));
        assert_eq!(power(1, 128, 255), Some(Power { ac: true, battery: None }), "a desktop: no battery");
        assert_eq!(power(1, 0, 255), Some(Power { ac: true, battery: None }), "percent unknown");
        assert_eq!(power(255, 0, 50), None, "AC unknown");
    }

    #[test]
    #[ignore = "real PC: reads this PC's pad, display and power"]
    fn real_snapshot() {
        let s = snapshot(0, None);
        println!("{}", serde_json::to_string(&s).unwrap());
        assert!(s.display.is_some() && s.power.is_some());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let last = last_key_or_mouse(now);
        println!("last key or mouse input {} ms ago", now - last);
        assert!(last > 0 && last <= now);
    }
}
