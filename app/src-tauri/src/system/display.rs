//! The primary display: its modes (resolution and refresh rate), a trial mode change that reverts on its own unless
//! kept, and HDR on/off. A trial is a dynamic change (not saved); keeping it saves it to the registry.

use super::log;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsExW, CDS_TYPE, CDS_UPDATEREGISTRY, DEVMODEW,
    DISPLAY_DEVICEW, DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT,
    DM_PELSWIDTH, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE,
};

/// How long a trial mode lasts before it reverts on its own.
pub const TRIAL: Duration = Duration::from_secs(15);

/// Sends the `display` event: {state: "trial" | "kept" | "reverted", ...}.
pub type Emit = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Hdr {
    pub supported: bool,
    pub enabled: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// GDI name, e.g. `\\.\DISPLAY1`
    pub device: String,
    /// The monitor's name, or "Built-in display"
    pub name: String,
    pub current: Mode,
    /// Usable modes, largest first
    pub modes: Vec<Mode>,
    pub hdr: Hdr,
    /// A trial mode is on screen and will revert unless kept
    pub trial: bool,
}

/// A mode as the driver lists it, before filtering.
#[derive(Clone, Copy, Debug)]
pub struct RawMode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub bpp: u32,
    pub interlaced: bool,
}

/// The modes worth offering: 32-bit colour, progressive, at least 800 wide, a real refresh rate (0 and 1 mean
/// "hardware default"); one entry per (width, height, hz), largest first. EnumDisplaySettingsEx without
/// EDS_ROTATEDMODE already lists only the current orientation.
pub fn usable_modes(raw: &[RawMode]) -> Vec<Mode> {
    let mut modes: Vec<Mode> = raw
        .iter()
        .filter(|m| m.bpp == 32 && !m.interlaced && m.width >= 800 && m.hz > 1)
        .map(|m| Mode { width: m.width, height: m.height, hz: m.hz })
        .collect();
    modes.sort_by(|a, b| b.cmp(a));
    modes.dedup();
    modes
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

type DeviceName = [u16; 32];

fn primary() -> Result<DeviceName, String> {
    for i in 0.. {
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            break;
        }
        if dd.StateFlags.0 & DISPLAY_DEVICE_PRIMARY_DEVICE.0 != 0 {
            return Ok(dd.DeviceName);
        }
    }
    Err("no primary display".into())
}

fn devmode(dev: &DeviceName, index: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe { EnumDisplaySettingsExW(PCWSTR(dev.as_ptr()), index, &mut dm, ENUM_DISPLAY_SETTINGS_FLAGS(0)) }
        .as_bool()
        .then_some(dm)
}

fn mode_of(dm: &DEVMODEW) -> Mode {
    Mode { width: dm.dmPelsWidth, height: dm.dmPelsHeight, hz: dm.dmDisplayFrequency }
}

fn raw_modes(dev: &DeviceName) -> Vec<RawMode> {
    let mut list = Vec::new();
    let mut i = 0;
    while let Some(dm) = devmode(dev, ENUM_DISPLAY_SETTINGS_MODE(i)) {
        list.push(RawMode {
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency,
            bpp: dm.dmBitsPerPel,
            interlaced: unsafe { dm.Anonymous2.dmDisplayFlags } & 2 != 0, // DM_INTERLACED
        });
        i += 1;
    }
    list
}

fn current(dev: &DeviceName) -> Result<DEVMODEW, String> {
    devmode(dev, ENUM_CURRENT_SETTINGS).ok_or_else(|| "can't read the current display mode".into())
}

/// Applies `m` over `base` (the device's current DEVMODE). `flags` 0 = dynamic only, CDS_UPDATEREGISTRY = saved.
fn apply(dev: &DeviceName, base: &DEVMODEW, m: Mode, flags: CDS_TYPE) -> Result<(), String> {
    let mut dm = *base;
    dm.dmPelsWidth = m.width;
    dm.dmPelsHeight = m.height;
    dm.dmDisplayFrequency = m.hz;
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    let r = unsafe { ChangeDisplaySettingsExW(PCWSTR(dev.as_ptr()), Some(&dm as *const _), None, flags, None) };
    if r == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(format!("{}×{} at {} Hz didn't apply (error {})", m.width, m.height, m.hz, r.0))
    }
}

// ---------- DisplayConfig: the monitor's name and HDR ----------

#[derive(Clone, Copy)]
struct Target {
    adapter: LUID,
    id: u32,
    internal: bool,
}

fn header<T>(kind: DISPLAYCONFIG_DEVICE_INFO_TYPE, adapter: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: kind, size: std::mem::size_of::<T>() as u32, adapterId: adapter, id }
}

/// The active path whose source is `device` (the GDI name).
fn target_for(device: &str) -> Option<Target> {
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
            != ERROR_SUCCESS
        {
            return None;
        }
        paths.truncate(np as usize);
        paths.iter().find_map(|p| {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    p.sourceInfo.adapterId,
                    p.sourceInfo.id,
                ),
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut src as *mut _ as *mut _) != 0 || wide(&src.viewGdiDeviceName) != device {
                return None;
            }
            let tech = p.targetInfo.outputTechnology;
            let internal = [
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            ]
            .contains(&tech);
            Some(Target { adapter: p.targetInfo.adapterId, id: p.targetInfo.id, internal })
        })
    }
}

fn monitor_name(t: &Target) -> Option<String> {
    let mut n = DISPLAYCONFIG_TARGET_DEVICE_NAME {
        header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, t.adapter, t.id),
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut n as *mut _ as *mut _) } != 0 {
        return None;
    }
    Some(wide(&n.monitorFriendlyDeviceName)).filter(|s| !s.is_empty())
}

// Windows 11 24H2 split HDR from wide colour; these two aren't in the windows crate's metadata yet (wingdi.h).
const GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(15);
const SET_HDR_STATE: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(16);

#[repr(C)]
#[derive(Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// bit 4 highDynamicRangeSupported, bit 5 highDynamicRangeUserEnabled
    value: u32,
    color_encoding: i32,
    bits_per_color_channel: u32,
    /// DISPLAYCONFIG_ADVANCED_COLOR_MODE: 0 SDR, 1 WCG, 2 HDR
    active_color_mode: i32,
}

#[repr(C)]
#[derive(Default)]
struct SetHdrState {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    /// bit 0 enableHdr
    value: u32,
}

fn hdr(t: &Target) -> Hdr {
    unsafe {
        let mut i2 = AdvancedColorInfo2 {
            header: header::<AdvancedColorInfo2>(GET_ADVANCED_COLOR_INFO_2, t.adapter, t.id),
            ..Default::default()
        };
        if DisplayConfigGetDeviceInfo(&mut i2 as *mut _ as *mut _) == 0 {
            return Hdr { supported: i2.value & 0x10 != 0, enabled: i2.value & 0x20 != 0 || i2.active_color_mode == 2 };
        }
        // before 24H2, "advanced colour" meant HDR: bit 0 supported, bit 1 enabled, bit 3 force-disabled
        let mut i = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
                DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                t.adapter,
                t.id,
            ),
            ..Default::default()
        };
        if DisplayConfigGetDeviceInfo(&mut i as *mut _ as *mut _) != 0 {
            return Hdr::default();
        }
        let v = i.Anonymous.value;
        Hdr { supported: v & 1 != 0 && v & 8 == 0, enabled: v & 2 != 0 }
    }
}

pub fn set_hdr(on: bool) -> Result<(), String> {
    let dev = primary()?;
    let t = target_for(&wide(&dev)).ok_or("can't find the display's output")?;
    if !hdr(&t).supported {
        return Err("HDR is not supported on this display".into());
    }
    let r = unsafe {
        let s = SetHdrState { header: header::<SetHdrState>(SET_HDR_STATE, t.adapter, t.id), value: on as u32 };
        match DisplayConfigSetDeviceInfo(&s as *const _ as *const _) {
            0 => 0,
            _ => {
                let mut old = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
                    header: header::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>(
                        DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
                        t.adapter,
                        t.id,
                    ),
                    ..Default::default()
                };
                old.Anonymous.value = on as u32;
                DisplayConfigSetDeviceInfo(&old as *const _ as *const _)
            }
        }
    };
    if r != 0 {
        return Err(format!("couldn't turn HDR {} (error {r})", if on { "on" } else { "off" }));
    }
    log(&format!("display: HDR {}", if on { "on" } else { "off" }));
    Ok(())
}

// ---------- info and the trial ----------

pub fn info() -> Result<Display, String> {
    let dev = primary()?;
    let device = wide(&dev);
    let current = mode_of(&current(&dev)?);
    let modes = usable_modes(&raw_modes(&dev));
    let (name, hdr) = match target_for(&device) {
        Some(t) => {
            let name = monitor_name(&t).unwrap_or_else(|| if t.internal { "Built-in display".into() } else { "Display".into() });
            (name, hdr(&t))
        }
        None => (device.clone(), Hdr::default()),
    };
    Ok(Display { device, name, current, modes, hdr, trial: state().trial.is_some() })
}

struct Trial {
    device: DeviceName,
    saved: DEVMODEW,
    mode: Mode,
}

struct State {
    /// bumped by every trial, keep and revert, so a stale timer does nothing
    generation: u64,
    trial: Option<Trial>,
}

static STATE: Mutex<State> = Mutex::new(State { generation: 0, trial: None });

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn restore(t: &Trial) -> Result<(), String> {
    apply(&t.device, &t.saved, mode_of(&t.saved), CDS_TYPE(0))
}

fn describe(m: Mode) -> String {
    format!("{}x{} {} Hz", m.width, m.height, m.hz)
}

/// Shows `m` until `keep` (saved) or `revert`; after `after` it reverts on its own.
pub fn try_mode(m: Mode, after: Duration, emit: Emit) -> Result<(), String> {
    let dev = primary()?;
    if !usable_modes(&raw_modes(&dev)).contains(&m) {
        return Err(format!("{} isn't a mode of this display", describe(m)));
    }
    let generation = {
        let mut st = state();
        // a second trial keeps the mode from before the first one, so a revert goes all the way back
        let saved = match &st.trial {
            Some(t) => t.saved,
            None => current(&dev)?,
        };
        apply(&dev, &saved, m, CDS_TYPE(0))?;
        st.generation += 1;
        st.trial = Some(Trial { device: dev, saved, mode: m });
        log(&format!("display: trying {} (was {})", describe(m), describe(mode_of(&saved))));
        st.generation
    };
    emit(json!({ "state": "trial", "seconds": after.as_secs(), "mode": m }));
    std::thread::Builder::new()
        .name("display-revert".into())
        .spawn(move || {
            std::thread::sleep(after);
            let mut st = state();
            if st.generation != generation {
                return;
            }
            if let Some(t) = st.trial.take() {
                st.generation += 1;
                let result = restore(&t);
                drop(st);
                log(&format!("display: no answer in {} s, reverted to {}", after.as_secs(), describe(mode_of(&t.saved))));
                emit(json!({ "state": "reverted", "reason": "timeout", "mode": mode_of(&t.saved), "error": result.err() }));
            }
        })
        .map_err(|e| format!("display timer: {e}"))?;
    Ok(())
}

/// Keeps the trial mode and saves it.
pub fn keep(emit: &Emit) -> Result<(), String> {
    let mut st = state();
    let t = st.trial.take().ok_or("no display change to keep")?;
    st.generation += 1;
    let result = apply(&t.device, &t.saved, t.mode, CDS_UPDATEREGISTRY);
    drop(st);
    result?;
    log(&format!("display: kept {}", describe(t.mode)));
    emit(json!({ "state": "kept", "mode": t.mode }));
    Ok(())
}

/// Goes back to the mode from before the trial now.
pub fn revert(emit: &Emit) -> Result<(), String> {
    let mut st = state();
    let t = st.trial.take().ok_or("no display change to revert")?;
    st.generation += 1;
    let result = restore(&t);
    drop(st);
    result?;
    log(&format!("display: reverted to {}", describe(mode_of(&t.saved))));
    emit(json!({ "state": "reverted", "reason": "asked", "mode": mode_of(&t.saved) }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(width: u32, height: u32, hz: u32, bpp: u32, interlaced: bool) -> RawMode {
        RawMode { width, height, hz, bpp, interlaced }
    }

    #[test]
    fn usable_modes_are_filtered_deduped_and_largest_first() {
        let list = [
            raw(1280, 720, 60, 32, false),
            raw(1920, 1080, 60, 32, false),
            raw(1920, 1080, 60, 32, false), // the same mode with another scaling/fixed output
            raw(1920, 1080, 144, 32, false),
            raw(1920, 1080, 60, 16, false),  // 16-bit colour
            raw(1920, 1080, 30, 32, true),   // interlaced
            raw(640, 480, 60, 32, false),    // too small
            raw(1600, 900, 1, 32, false),    // "default" refresh
            raw(1920, 1200, 60, 32, false),
        ];
        let m = |width, height, hz| Mode { width, height, hz };
        assert_eq!(
            usable_modes(&list),
            vec![m(1920, 1200, 60), m(1920, 1080, 144), m(1920, 1080, 60), m(1280, 720, 60)]
        );
    }

    #[test]
    #[ignore = "real hardware: reads the display's modes and HDR (changes nothing)"]
    fn real_display_modes_and_hdr() {
        let d = info().unwrap();
        println!("display {} ({}), current {}", d.device, d.name, describe(d.current));
        let sizes: Vec<String> = {
            let mut s: Vec<(u32, u32)> = d.modes.iter().map(|m| (m.width, m.height)).collect();
            s.dedup();
            s.iter().map(|(w, h)| format!("{w}x{h}")).collect()
        };
        println!("{} modes; resolutions: {}", d.modes.len(), sizes.join(", "));
        let rates: Vec<u32> =
            d.modes.iter().filter(|m| m.width == d.current.width && m.height == d.current.height).map(|m| m.hz).collect();
        println!("refresh rates at {}x{}: {:?} Hz", d.current.width, d.current.height, rates);
        println!("HDR: supported {}, enabled {}", d.hdr.supported, d.hdr.enabled);
        assert!(d.modes.contains(&d.current));
    }

    /// VISIBLE: changes the resolution for 15 s, then the timer must bring the original mode back.
    #[test]
    #[ignore = "real hardware: changes the screen resolution for 15 s (take the screen lock)"]
    fn real_display_trial_reverts_on_timeout() {
        let before = info().unwrap().current;
        println!("before: {}", describe(before));
        let modes = info().unwrap().modes;
        // TRIAL_RATE=1 tries another refresh rate at the same resolution instead of another resolution
        let rate = std::env::var("TRIAL_RATE").is_ok_and(|v| v == "1");
        let other = modes
            .iter()
            .copied()
            .find(|m| {
                let same_size = (m.width, m.height) == (before.width, before.height);
                if rate {
                    same_size && m.hz != before.hz
                } else {
                    m.hz == before.hz && !same_size && m.width < before.width
                }
            })
            .or_else(|| modes.iter().copied().find(|m| *m != before))
            .expect("another mode");
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let sink = events.clone();
        let emit: Emit = Arc::new(move |v| {
            println!("event: {v}");
            sink.lock().unwrap().push(v);
        });
        let t0 = std::time::Instant::now();
        try_mode(other, TRIAL, emit).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        let during = info().unwrap();
        println!("during trial (+2 s): {} trial={}", describe(during.current), during.trial);
        assert_eq!(during.current, other);
        assert!(during.trial);
        while events.lock().unwrap().len() < 2 && t0.elapsed() < Duration::from_secs(25) {
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("reverted after {:.1} s", t0.elapsed().as_secs_f32());
        let after = info().unwrap();
        println!("after: {} trial={}", describe(after.current), after.trial);
        assert_eq!(after.current, before);
        assert!(!after.trial);
        assert_eq!(events.lock().unwrap()[1]["state"], "reverted");
    }
}
