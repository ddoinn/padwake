//! PlayStation and Switch pads over HID (row C1): DualSense (and Edge), DualShock 4 and Switch Pro, on USB and
//! Bluetooth. Each one becomes a pad slot from 4 up, merged by the poller (`xinput.rs`) like an XInput slot, with the
//! same button model: buttons by position (south is A, east is B), the PS / Home button is Guide.
//!
//! Event-driven: one thread per pad blocks on an overlapped `ReadFile`, and pads arriving later come from a CfgMgr32
//! device-interface notification. Nothing polls, and with no such pad there's no thread at all. A report updates the
//! shared state only when buttons, triggers or a stick step changed (the pads stream gyro data continuously).
//!
//! Report layouts: Linux `drivers/hid/hid-playstation.c`, `hid-sony.c` and `hid-nintendo.c`, SDL
//! `src/joystick/hidapi/`, and dekuNukem's Nintendo_Switch_Reverse_Engineering notes (2026-11).

use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_PropertyW, CM_Get_Parent, CM_Locate_DevNodeW, CM_Register_Notification,
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_LOCATE_DEVNODE_NORMAL, CM_NOTIFY_ACTION,
    CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
    CR_SUCCESS, HCMNOTIFICATION,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData, HidP_GetCaps, GUID_DEVINTERFACE_HID,
    HIDD_ATTRIBUTES, HIDP_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Devices::Properties::{DEVPKEY_Device_InstanceId, DEVPROPTYPE};
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::xinput::quantize;

const SONY: u16 = 0x054C;
const NINTENDO: u16 = 0x057E;
const TRIGGER_THRESHOLD: u8 = 30; // as XInput's, on the same 0..255 scale
/// A Switch Pro pad on USB sends nothing until it's told to (Steam and SDL do that). If it is silent this long after
/// opening, Padwake does it.
const SWITCH_WAKE: Duration = Duration::from_secs(1);

/// The pad families the UI draws hints for.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    #[default]
    Xbox,
    Playstation,
    Switch,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Xbox => "xbox",
            Family::Playstation => "playstation",
            Family::Switch => "switch",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    DualShock4,
    DualSense,
    SwitchPro,
}

impl Model {
    /// Sources: SDL `controller_list.h`, Linux `hid-ids.h`. 8BitDo and other pads in Switch mode report the Pro's IDs.
    pub fn from_ids(vid: u16, pid: u16) -> Option<Model> {
        match (vid, pid) {
            (SONY, 0x05C4 | 0x09CC | 0x0BA0) => Some(Model::DualShock4), // v1, v2, the USB wireless adapter
            (SONY, 0x0CE6 | 0x0DF2) => Some(Model::DualSense),          // DualSense, DualSense Edge
            (NINTENDO, 0x2009) => Some(Model::SwitchPro),
            _ => None,
        }
    }

    pub fn family(self) -> Family {
        match self {
            Model::DualShock4 | Model::DualSense => Family::Playstation,
            Model::SwitchPro => Family::Switch,
        }
    }

    /// The name Settings shows: plain words, no product names.
    pub fn label(self) -> &'static str {
        match self.family() {
            Family::Switch => "Switch controller",
            _ => "PlayStation controller",
        }
    }
}

/// One report, in the poller's terms: XInput button bits, digital triggers, sticks -1..1 with y down.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Report {
    pub buttons: u16,
    pub lt: bool,
    pub rt: bool,
    pub axes: [f32; 4],
}

// XInput's button bits (xinput.rs builds the Pad from these)
const UP: u16 = 0x0001;
const DOWN: u16 = 0x0002;
const LEFT: u16 = 0x0004;
const RIGHT: u16 = 0x0008;
const MENU: u16 = 0x0010;
const VIEW: u16 = 0x0020;
const L3: u16 = 0x0040;
const R3: u16 = 0x0080;
const LB: u16 = 0x0100;
const RB: u16 = 0x0200;
const GUIDE: u16 = 0x0400;
const A: u16 = 0x1000;
const B: u16 = 0x2000;
const X: u16 = 0x4000;
const Y: u16 = 0x8000;

/// A hat switch (0 up, clockwise to 7 up-left; anything else is centred) as D-pad bits.
fn hat(v: u8) -> u16 {
    match v {
        0 => UP,
        1 => UP | RIGHT,
        2 => RIGHT,
        3 => DOWN | RIGHT,
        4 => DOWN,
        5 => DOWN | LEFT,
        6 => LEFT,
        7 => UP | LEFT,
        _ => 0,
    }
}

/// Maps the bits set in `byte` through a table of (bit, XInput bit).
fn bits(byte: u8, table: &[(u8, u16)]) -> u16 {
    table.iter().filter(|(bit, _)| byte & bit != 0).fold(0, |acc, (_, x)| acc | x)
}

/// 0..255 with 128 at rest (PlayStation sticks, y already down) to -1..1.
fn byte_axis(v: u8) -> f32 {
    ((v as f32 - 128.0) / 127.0).clamp(-1.0, 1.0)
}

/// The PlayStation face, shoulder and system buttons: `b0` holds the hat and the shapes, `b1` the shoulders, `b2` PS.
fn playstation(sticks: &[u8], b0: u8, b1: u8, b2: u8, l2: u8, r2: u8) -> Report {
    let buttons = hat(b0 & 0x0F)
        | bits(b0, &[(0x10, X), (0x20, A), (0x40, B), (0x80, Y)]) // square, cross, circle, triangle
        | bits(b1, &[(0x01, LB), (0x02, RB), (0x10, VIEW), (0x20, MENU), (0x40, L3), (0x80, R3)]) // Create/Share, Options
        | bits(b2, &[(0x01, GUIDE)]); // the PS button; 0x02 is the touchpad click (later)
    Report {
        buttons,
        lt: l2 > TRIGGER_THRESHOLD,
        rt: r2 > TRIGGER_THRESHOLD,
        axes: [byte_axis(sticks[0]), byte_axis(sticks[1]), byte_axis(sticks[2]), byte_axis(sticks[3])],
    }
}

/// DualShock 4 order from `o`: sticks, b0 b1 b2, L2, R2. USB report 0x01 and Bluetooth's basic 0x01 at 1, Bluetooth's
/// full 0x11 at 3. DualSense's Bluetooth simple 0x01 has the same order.
fn ds4_order(r: &[u8], o: usize) -> Option<Report> {
    let d = r.get(o..o + 9)?;
    Some(playstation(&d[0..4], d[4], d[5], d[6], d[7], d[8]))
}

/// DualSense order from `o`: sticks, L2, R2, a counter, b0 b1 b2. USB report 0x01 at 1, Bluetooth's full 0x31 at 2.
fn ds5_order(r: &[u8], o: usize) -> Option<Report> {
    let d = r.get(o..o + 10)?;
    Some(playstation(&d[0..4], d[7], d[8], d[9], d[4], d[5]))
}

/// A 12-bit Nintendo stick value (0..4095, ~2048 at rest, up is larger) to -1..1. Factory calibration lives in the
/// pad's flash; without reading it, ±1600 around the middle is full travel (hid-nintendo's default range).
fn switch_axis(v: u16) -> f32 {
    ((v as f32 - 2048.0) / 1600.0).clamp(-1.0, 1.0)
}

/// Switch full report (0x30, and 0x21 replies, which carry the same state): byte 3 right buttons, 4 shared, 5 left,
/// 6..9 left stick and 9..12 right stick as two packed 12-bit values each.
fn switch_full(r: &[u8]) -> Option<Report> {
    let d = r.get(0..12)?;
    let stick = |s: &[u8]| {
        let x = u16::from(s[0]) | (u16::from(s[1] & 0x0F) << 8);
        let y = u16::from(s[1] >> 4) | (u16::from(s[2]) << 4);
        (switch_axis(x), -switch_axis(y))
    };
    let (lx, ly) = stick(&d[6..9]);
    let (rx, ry) = stick(&d[9..12]);
    let buttons = bits(d[3], &[(0x01, X), (0x02, Y), (0x04, A), (0x08, B), (0x40, RB)]) // Y X B A by position, R
        | bits(d[4], &[(0x01, VIEW), (0x02, MENU), (0x04, R3), (0x08, L3), (0x10, GUIDE)]) // − + sticks Home (0x20 Capture)
        | bits(d[5], &[(0x01, DOWN), (0x02, UP), (0x04, RIGHT), (0x08, LEFT), (0x40, LB)]);
    Some(Report { buttons, lt: d[5] & 0x80 != 0, rt: d[3] & 0x80 != 0, axes: [lx, ly, rx, ry] })
}

/// Switch simple report (0x3F, what a Pro sends over Bluetooth until told otherwise): two button bytes, a hat, then
/// four little-endian 16-bit axes with 0x8000 at rest and y down.
fn switch_simple(r: &[u8]) -> Option<Report> {
    let d = r.get(0..12)?;
    let axis = |i: usize| ((f32::from(u16::from_le_bytes([d[i], d[i + 1]])) - 32768.0) / 32767.0).clamp(-1.0, 1.0);
    let buttons = bits(d[1], &[(0x01, A), (0x02, B), (0x04, X), (0x08, Y), (0x10, LB), (0x20, RB)]) // B A Y X by position
        | bits(d[2], &[(0x01, VIEW), (0x02, MENU), (0x04, L3), (0x08, R3), (0x10, GUIDE)])
        | hat(d[3]);
    Some(Report { buttons, lt: d[1] & 0x40 != 0, rt: d[1] & 0x80 != 0, axes: [axis(4), axis(6), axis(8), axis(10)] })
}

/// One input report (report ID first) as buttons and sticks; None for reports that carry no pad state.
pub fn parse(model: Model, bluetooth: bool, r: &[u8]) -> Option<Report> {
    match (model, bluetooth, *r.first()?) {
        (Model::DualShock4, _, 0x01) => ds4_order(r, 1),
        (Model::DualShock4, true, 0x11) => ds4_order(r, 3),
        (Model::DualSense, false, 0x01) => ds5_order(r, 1),
        (Model::DualSense, true, 0x01) => ds4_order(r, 1),
        (Model::DualSense, true, 0x31) => ds5_order(r, 2),
        (Model::SwitchPro, _, 0x30 | 0x21) => switch_full(r),
        (Model::SwitchPro, _, 0x3F) => switch_simple(r),
        _ => None,
    }
}

/// A connected pad as the poller sees it. `seq` changes whenever its state does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Live {
    pub slot: u32,
    pub model: Model,
    pub bluetooth: bool,
    pub buttons: u16,
    pub lt: bool,
    pub rt: bool,
    /// stick steps of 0.05, y down
    pub axes: [i8; 4],
    pub seq: u32,
}

struct Entry {
    live: Live,
    /// the device interface path, or "test" for the test hook's pad
    path: String,
}

static PADS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn pads_lock() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    PADS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The connected pads now (cheap: the poller asks every poll).
pub fn pads() -> Vec<Live> {
    pads_lock().iter().map(|e| e.live).collect()
}

/// Adds a pad under the lowest free slot from 4; None if that path is already open.
fn add(path: &str, model: Model, bluetooth: bool) -> Option<u32> {
    let mut pads = pads_lock();
    if pads.iter().any(|e| e.path == path) {
        return None;
    }
    let slot = (4..).find(|s| !pads.iter().any(|e| e.live.slot == *s)).unwrap_or(4);
    let live = Live { slot, model, bluetooth, buttons: 0, lt: false, rt: false, axes: [0; 4], seq: 0 };
    pads.push(Entry { live, path: path.to_string() });
    Some(slot)
}

fn remove(slot: u32) {
    pads_lock().retain(|e| e.live.slot != slot);
}

/// Applies one report to a pad's state. True if anything the poller sees changed.
fn feed(slot: u32, report: &[u8]) -> bool {
    let mut pads = pads_lock();
    let Some(e) = pads.iter_mut().find(|e| e.live.slot == slot) else { return false };
    let p = &mut e.live;
    let Some(r) = parse(p.model, p.bluetooth, report) else { return false };
    let mut axes = p.axes;
    for (k, v) in r.axes.into_iter().enumerate() {
        axes[k] = quantize(axes[k], v);
    }
    let changed = (r.buttons, r.lt, r.rt, axes) != (p.buttons, p.lt, p.rt, p.axes);
    if changed {
        (p.buttons, p.lt, p.rt, p.axes) = (r.buttons, r.lt, r.rt, axes);
        p.seq = p.seq.wrapping_add(1);
    }
    changed
}

/// The test hook's pad (`report …` in `test_hook.rs`): a report through the same parser and state as a real pad's.
/// `None` takes the pad away.
pub fn feed_test(pad: Option<(Model, bool, &[u8])>) {
    let existing = pads_lock().iter().find(|e| e.path == "test").map(|e| (e.live.slot, e.live.model, e.live.bluetooth));
    match (pad, existing) {
        (None, Some((slot, ..))) => remove(slot),
        (None, None) => {}
        (Some((model, bt, report)), existing) => {
            let slot = match existing {
                Some((slot, m, b)) if (m, b) == (model, bt) => slot,
                other => {
                    if let Some((slot, ..)) = other {
                        remove(slot);
                    }
                    match add("test", model, bt) {
                        Some(slot) => slot,
                        None => return,
                    }
                }
            };
            feed(slot, report);
        }
    }
}

/// The pad's own stack: HID over USB or Bluetooth (a USB hub and a USB Bluetooth radio included).
const STACK: &[&str] = &["HID", "USB", "BTHENUM", "BTH", "BTHLE", "BTHLEDEVICE"];

/// A virtual pad (ViGEmBus's emulated DualShock 4, as DS4Windows makes): reading it too would double the real one.
/// Walking up from the pad, the first device outside its USB / Bluetooth stack is the bus it hangs on: PCI or ACPI
/// for a real pad, ROOT (or a software device) for a virtual one.
fn is_virtual(chain: &[String]) -> bool {
    let bus = chain.iter().skip(1).map(|id| id.to_ascii_uppercase()).find(|id| {
        let enumerator = id.split('\\').next().unwrap_or("");
        !STACK.contains(&enumerator)
    });
    bus.is_some_and(|id| id.starts_with("ROOT\\") || id.starts_with("SWD\\"))
}

/// The device instance IDs from this interface's device up to the root.
fn chain(path: &[u16]) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut ty = DEVPROPTYPE(0);
        let mut buf = [0u16; 400];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let key = &DEVPKEY_Device_InstanceId;
        if CM_Get_Device_Interface_PropertyW(PCWSTR(path.as_ptr()), key, &mut ty, Some(buf.as_mut_ptr().cast()), &mut size, 0)
            != CR_SUCCESS
        {
            return out;
        }
        let mut inst = 0u32;
        if CM_Locate_DevNodeW(&mut inst, PCWSTR(buf.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
            return out;
        }
        for _ in 0..16 {
            let mut id = [0u16; 400];
            if CM_Get_Device_IDW(inst, &mut id, 0) != CR_SUCCESS {
                break;
            }
            let len = id.iter().position(|&c| c == 0).unwrap_or(id.len());
            out.push(String::from_utf16_lossy(&id[..len]));
            let mut parent = 0u32;
            if CM_Get_Parent(&mut parent, inst, 0) != CR_SUCCESS {
                break;
            }
            inst = parent;
        }
    }
    out
}

/// A HID interface path that may be one of ours: Sony's or Nintendo's vendor ID appears in it on USB (`vid_054c`)
/// and Bluetooth (`vid&0002054c`). Opening is what confirms it; this only skips keyboards, mice and the rest.
fn maybe_ours(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains("054c") || p.contains("057e")
}

struct Device {
    handle: HANDLE,
    model: Model,
    bluetooth: bool,
    input_len: usize,
    output_len: usize,
}

// SAFETY: the handle is used by the one reader thread that owns the Device (writes included), then closed there.
unsafe impl Send for Device {}

/// Opens a HID interface if it is a pad we read (shared, so Steam or a game can read it too).
fn open(path: &[u16]) -> Option<Device> {
    let try_open = |access: u32| unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            None,
        )
        .ok()
    };
    let handle = try_open(GENERIC_READ.0 | GENERIC_WRITE.0).or_else(|| try_open(GENERIC_READ.0))?;
    let fail = |h: HANDLE| {
        unsafe { CloseHandle(h).ok() };
        None
    };
    let mut attrs = HIDD_ATTRIBUTES { Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
    if !unsafe { HidD_GetAttributes(handle, &mut attrs) } {
        return fail(handle);
    }
    let Some(model) = Model::from_ids(attrs.VendorID, attrs.ProductID) else { return fail(handle) };
    let mut data = PHIDP_PREPARSED_DATA::default();
    if !unsafe { HidD_GetPreparsedData(handle, &mut data) } {
        return fail(handle);
    }
    let mut caps = HIDP_CAPS::default();
    let ok = unsafe { HidP_GetCaps(data, &mut caps) }.is_ok();
    unsafe { HidD_FreePreparsedData(data) };
    // the gamepad (or joystick) collection only, not a pad's other collections
    if !ok || caps.UsagePage != 0x01 || !matches!(caps.Usage, 0x04 | 0x05) {
        return fail(handle);
    }
    Some(Device {
        handle,
        model,
        bluetooth: is_bluetooth(attrs.ProductID, caps.InputReportByteLength),
        input_len: caps.InputReportByteLength as usize,
        output_len: caps.OutputReportByteLength as usize,
    })
}

/// These pads' USB input reports are 64 bytes; over Bluetooth their descriptors declare longer ones. Sony's USB
/// adapter for the DualShock 4 looks like USB but is wireless.
fn is_bluetooth(pid: u16, input_len: u16) -> bool {
    input_len != 64 || pid == 0x0BA0
}

/// Opens the pad at `path` and reads it on its own thread until it goes away.
fn attach(path: Vec<u16>) {
    let text = String::from_utf16_lossy(&path[..path.len().saturating_sub(1)]);
    if !maybe_ours(&text) || pads_lock().iter().any(|e| e.path == text) {
        return;
    }
    if is_virtual(&chain(&path)) {
        crate::system::log(&format!("input: skipped a virtual pad (it mirrors a real one): {text}"));
        return;
    }
    let Some(dev) = open(&path) else { return };
    let Some(slot) = add(&text, dev.model, dev.bluetooth) else {
        unsafe { CloseHandle(dev.handle).ok() };
        return;
    };
    let link = if dev.bluetooth { "Bluetooth" } else { "USB" };
    crate::system::log(&format!("input: {} on {link} as pad {}", dev.model.label(), slot + 1));
    let spawned = std::thread::Builder::new().name(format!("hid-pad-{slot}")).spawn(move || {
        read_loop(&dev, slot);
        remove(slot);
        unsafe { CloseHandle(dev.handle).ok() };
        crate::system::log(&format!("input: pad {} went away", slot + 1));
    });
    if spawned.is_err() {
        remove(slot);
    }
}

/// Reads reports until the device fails (unplugged, switched off). Blocks in the kernel between reports.
fn read_loop(dev: &Device, slot: u32) {
    let Ok(event) = (unsafe { CreateEventW(None, true, false, None) }) else { return };
    let mut buf = vec![0u8; dev.input_len.max(1)];
    // a Switch Pro on USB stays silent until told to send
    let mut wake = dev.model == Model::SwitchPro && !dev.bluetooth;
    loop {
        let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
        if let Err(e) = unsafe { ReadFile(dev.handle, Some(buf.as_mut_slice()), None, Some(&mut ov)) } {
            if e.code() != ERROR_IO_PENDING.to_hresult() {
                break;
            }
        }
        // silent until the timeout, or only status packets (0x81) so far: send the start once
        if wake && unsafe { WaitForSingleObject(event, SWITCH_WAKE.as_millis() as u32) } == WAIT_TIMEOUT {
            switch_wake(dev);
            wake = false;
        }
        unsafe { WaitForSingleObject(event, INFINITE) };
        let mut n = 0u32;
        if unsafe { GetOverlappedResult(dev.handle, &ov, &mut n, false) }.is_err() {
            break;
        }
        let report = &buf[..(n as usize).min(buf.len())];
        if wake && parse(dev.model, dev.bluetooth, report).is_some() {
            wake = false; // it sends its state already (Steam or SDL started it)
        }
        feed(slot, report);
    }
    unsafe { CloseHandle(event).ok() };
}

/// The Switch Pro's USB start, as Linux's hid-nintendo does it: handshake, 3 Mbit, handshake, USB only (no timeout),
/// then input report mode 0x30 (full state at 120 Hz). Rumble bytes in the subcommand are the neutral pattern.
fn switch_wake(dev: &Device) {
    crate::system::log("input: a silent Switch controller on USB: sending its start");
    let mut packets: Vec<Vec<u8>> = vec![vec![0x80, 0x02], vec![0x80, 0x03], vec![0x80, 0x02], vec![0x80, 0x04]];
    packets.push(vec![0x01, 0x00, 0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40, 0x03, 0x30]);
    for p in packets {
        write(dev, &p);
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn write(dev: &Device, data: &[u8]) {
    let mut buf = data.to_vec();
    buf.resize(dev.output_len.max(data.len()), 0);
    let Ok(event) = (unsafe { CreateEventW(None, true, false, None) }) else { return };
    let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
    let sent = match unsafe { WriteFile(dev.handle, Some(buf.as_slice()), None, Some(&mut ov)) } {
        Ok(()) => true,
        Err(e) => e.code() == ERROR_IO_PENDING.to_hresult(),
    };
    if sent {
        // Windows owns `buf` and `ov` until the write completes: a write that hangs is cancelled and waited for
        if unsafe { WaitForSingleObject(event, 500) } == WAIT_TIMEOUT {
            let _ = unsafe { CancelIoEx(dev.handle, Some(&ov)) };
        }
        let mut n = 0u32;
        let _ = unsafe { GetOverlappedResult(dev.handle, &ov, &mut n, true) };
    }
    unsafe { CloseHandle(event).ok() };
}

/// The present HID interfaces' paths, each NUL-terminated.
fn present() -> Vec<Vec<u16>> {
    unsafe {
        let mut len = 0u32;
        let flags = CM_GET_DEVICE_INTERFACE_LIST_PRESENT;
        if CM_Get_Device_Interface_List_SizeW(&mut len, &GUID_DEVINTERFACE_HID, PCWSTR::null(), flags) != CR_SUCCESS {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize + 512];
        if CM_Get_Device_Interface_ListW(&GUID_DEVINTERFACE_HID, PCWSTR::null(), &mut buf, flags) != CR_SUCCESS {
            return vec![];
        }
        buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| s.iter().copied().chain(Some(0)).collect()).collect()
    }
}

unsafe extern "system" fn arrived(
    _: HCMNOTIFICATION,
    _: *const core::ffi::c_void,
    action: CM_NOTIFY_ACTION,
    data: *const CM_NOTIFY_EVENT_DATA,
    _: u32,
) -> u32 {
    if action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL && !data.is_null() {
        // SymbolicLink is a NUL-terminated string that runs past the struct's 1-element array
        let link = unsafe { PWSTR(std::ptr::addr_of!((*data).u.DeviceInterface.SymbolicLink) as *mut u16) };
        let wide = unsafe { link.as_wide() };
        // every HID device arriving comes here (mice, keyboards): only maybe-ours ones go further
        if maybe_ours(&String::from_utf16_lossy(wide)) {
            let path: Vec<u16> = wide.iter().copied().chain(Some(0)).collect();
            // opening can take a moment; never in the notification's own thread
            let _ = std::thread::Builder::new().name("hid-arrive".into()).spawn(move || attach(path));
        }
    }
    0
}

/// Opens the pads present now and asks Windows for the ones that arrive later.
pub fn start() {
    let spawned = std::thread::Builder::new().name("hid-pads".into()).spawn(|| {
        let filter = CM_NOTIFY_FILTER {
            cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            u: windows::Win32::Devices::DeviceAndDriverInstallation::CM_NOTIFY_FILTER_0 {
                DeviceInterface: windows::Win32::Devices::DeviceAndDriverInstallation::CM_NOTIFY_FILTER_0_0 {
                    ClassGuid: GUID_DEVINTERFACE_HID,
                },
            },
            ..Default::default()
        };
        let mut handle = HCMNOTIFICATION::default();
        // registered for the app's lifetime: never unregistered
        if unsafe { CM_Register_Notification(&filter, None, Some(arrived), &mut handle) } != CR_SUCCESS {
            crate::system::log("input: no pad arrival notifications: PlayStation and Switch pads plugged in later are missed");
        }
        for path in present() {
            attach(path);
        }
    });
    if let Err(e) = spawned {
        eprintln!("hid pads: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report of `len` zero bytes with the ID first, then (offset, byte) pairs written in.
    fn report(len: usize, id: u8, set: &[(usize, u8)]) -> Vec<u8> {
        let mut r = vec![0u8; len];
        r[0] = id;
        for &(i, v) in set {
            r[i] = v;
        }
        r
    }

    fn steps(r: &Report) -> [i8; 4] {
        r.axes.map(|v| quantize(0, v))
    }

    // ---- PlayStation: sticks at rest are 0x80, the hat at rest is 8

    /// DualShock 4 bytes from offset `o`: sticks, b0 b1 b2, L2, R2.
    fn ds4(len: usize, id: u8, o: usize, sticks: [u8; 4], b: [u8; 3], l2: u8, r2: u8) -> Vec<u8> {
        let mut set: Vec<(usize, u8)> = sticks.iter().enumerate().map(|(i, &v)| (o + i, v)).collect();
        set.extend([(o + 4, b[0]), (o + 5, b[1]), (o + 6, b[2]), (o + 7, l2), (o + 8, r2)]);
        report(len, id, &set)
    }

    /// DualSense bytes from offset `o`: sticks, L2, R2, counter, b0 b1 b2.
    fn ds5(len: usize, id: u8, o: usize, sticks: [u8; 4], b: [u8; 3], l2: u8, r2: u8) -> Vec<u8> {
        let mut set: Vec<(usize, u8)> = sticks.iter().enumerate().map(|(i, &v)| (o + i, v)).collect();
        set.extend([(o + 4, l2), (o + 5, r2), (o + 6, 0x5A), (o + 7, b[0]), (o + 8, b[1]), (o + 9, b[2])]);
        report(len, id, &set)
    }

    const REST: [u8; 4] = [0x80; 4];

    /// Every PlayStation layout: (model, bluetooth, builder). Each must read the same buttons from the same presses.
    fn playstation_layouts() -> Vec<(&'static str, Model, bool, Box<dyn Fn([u8; 4], [u8; 3], u8, u8) -> Vec<u8>>)> {
        vec![
            ("DualShock 4 USB 0x01", Model::DualShock4, false, Box::new(|s, b, l, r| ds4(64, 0x01, 1, s, b, l, r))),
            ("DualShock 4 Bluetooth basic 0x01", Model::DualShock4, true, Box::new(|s, b, l, r| ds4(10, 0x01, 1, s, b, l, r))),
            ("DualShock 4 Bluetooth full 0x11", Model::DualShock4, true, Box::new(|s, b, l, r| ds4(78, 0x11, 3, s, b, l, r))),
            ("DualSense USB 0x01", Model::DualSense, false, Box::new(|s, b, l, r| ds5(64, 0x01, 1, s, b, l, r))),
            ("DualSense Bluetooth simple 0x01", Model::DualSense, true, Box::new(|s, b, l, r| ds4(10, 0x01, 1, s, b, l, r))),
            ("DualSense Bluetooth full 0x31", Model::DualSense, true, Box::new(|s, b, l, r| ds5(78, 0x31, 2, s, b, l, r))),
        ]
    }

    #[test]
    fn playstation_reports_read_by_position_on_usb_and_bluetooth() {
        for (what, model, bt, build) in playstation_layouts() {
            let p = |s, b, l, r| parse(model, bt, &build(s, b, l, r)).unwrap_or_else(|| panic!("{what}: no state"));
            let rest = p(REST, [0x08, 0, 0], 0, 0);
            assert_eq!((rest.buttons, rest.lt, rest.rt, steps(&rest)), (0, false, false, [0; 4]), "{what}: at rest");
            // the four shapes: cross is south (A), circle east (B), square west (X), triangle north (Y)
            assert_eq!(p(REST, [0x08 | 0x20, 0, 0], 0, 0).buttons, A, "{what}: cross");
            assert_eq!(p(REST, [0x08 | 0x40, 0, 0], 0, 0).buttons, B, "{what}: circle");
            assert_eq!(p(REST, [0x08 | 0x10, 0, 0], 0, 0).buttons, X, "{what}: square");
            assert_eq!(p(REST, [0x08 | 0x80, 0, 0], 0, 0).buttons, Y, "{what}: triangle");
            // the hat, every way round
            let dirs = [UP, UP | RIGHT, RIGHT, DOWN | RIGHT, DOWN, DOWN | LEFT, LEFT, UP | LEFT];
            for (h, want) in dirs.into_iter().enumerate() {
                assert_eq!(p(REST, [h as u8, 0, 0], 0, 0).buttons, want, "{what}: hat {h}");
            }
            // shoulders, Create/Share, Options, stick clicks, PS
            assert_eq!(p(REST, [0x08, 0x01 | 0x02, 0], 0, 0).buttons, LB | RB, "{what}: L1 R1");
            assert_eq!(p(REST, [0x08, 0x10, 0], 0, 0).buttons, VIEW, "{what}: Create/Share is View");
            assert_eq!(p(REST, [0x08, 0x20, 0], 0, 0).buttons, MENU, "{what}: Options is Menu");
            assert_eq!(p(REST, [0x08, 0x40 | 0x80, 0], 0, 0).buttons, L3 | R3, "{what}: L3 R3");
            assert_eq!(p(REST, [0x08, 0, 0x01], 0, 0).buttons, GUIDE, "{what}: PS is Guide");
            assert_eq!(p(REST, [0x08, 0, 0x02], 0, 0).buttons, 0, "{what}: the touchpad click isn't mapped");
            // triggers by the analog value; the digital bits alone don't count
            let t = p(REST, [0x08, 0x04 | 0x08, 0], 31, 255);
            assert_eq!((t.lt, t.rt), (true, true), "{what}: L2 R2");
            let t = p(REST, [0x08, 0x04 | 0x08, 0], 30, 0);
            assert_eq!((t.lt, t.rt), (false, false), "{what}: under the threshold");
            // sticks: left full left and up, right full right and down
            let s = p([0x00, 0x00, 0xFF, 0xFF], [0x08, 0, 0], 0, 0);
            assert_eq!(steps(&s), [-20, -20, 20, 20], "{what}: sticks, y down");
            let s = p([0xC0, 0x80, 0x80, 0x40], [0x08, 0, 0], 0, 0);
            assert_eq!(steps(&s), [10, 0, 0, -10], "{what}: half way");
            // everything at once
            let all = p(REST, [0x00 | 0xF0, 0xFF, 0x01], 255, 255);
            assert_eq!(all.buttons, UP | A | B | X | Y | LB | RB | VIEW | MENU | L3 | R3 | GUIDE, "{what}: all");
        }
    }

    #[test]
    fn reports_without_pad_state_are_skipped() {
        assert_eq!(parse(Model::DualSense, false, &[0x05, 1, 2, 3]), None, "a feature or other report");
        assert_eq!(parse(Model::DualShock4, false, &[0x11; 78]), None, "0x11 is Bluetooth only");
        assert_eq!(parse(Model::DualSense, true, &[0x31, 0, 0x80]), None, "too short");
        assert_eq!(parse(Model::SwitchPro, false, &[0x81, 0x02]), None, "a USB reply");
        assert_eq!(parse(Model::SwitchPro, true, &[]), None, "empty");
    }

    // ---- Switch Pro

    /// A full 0x30 report: right, shared and left button bytes, and the sticks as 12-bit values.
    fn pro_full(right: u8, shared: u8, left: u8, l: (u16, u16), r: (u16, u16)) -> Vec<u8> {
        let pack = |(x, y): (u16, u16)| [(x & 0xFF) as u8, ((x >> 8) as u8 & 0x0F) | (((y & 0x0F) as u8) << 4), (y >> 4) as u8];
        let mut set = vec![(1, 0x42), (2, 0x8E), (3, right), (4, shared), (5, left)];
        set.extend(pack(l).into_iter().enumerate().map(|(i, v)| (6 + i, v)));
        set.extend(pack(r).into_iter().enumerate().map(|(i, v)| (9 + i, v)));
        report(64, 0x30, &set)
    }

    /// A simple 0x3F report: two button bytes, the hat, and four 16-bit axes.
    fn pro_simple(b1: u8, b2: u8, hat: u8, axes: [u16; 4]) -> Vec<u8> {
        let mut set = vec![(1, b1), (2, b2), (3, hat)];
        for (i, a) in axes.into_iter().enumerate() {
            set.push((4 + i * 2, (a & 0xFF) as u8));
            set.push((5 + i * 2, (a >> 8) as u8));
        }
        report(49, 0x3F, &set)
    }

    const MID: (u16, u16) = (2048, 2048);

    #[test]
    fn switch_full_reports_read_by_position_on_usb_and_bluetooth() {
        for bt in [false, true] {
            let p = |right, shared, left, l, r| parse(Model::SwitchPro, bt, &pro_full(right, shared, left, l, r)).unwrap();
            let rest = p(0, 0, 0, MID, MID);
            assert_eq!((rest.buttons, rest.lt, rest.rt, steps(&rest)), (0, false, false, [0; 4]), "at rest (bt {bt})");
            // B is south (A), A east (B), Y west (X), X north (Y)
            assert_eq!(p(0x04, 0, 0, MID, MID).buttons, A, "B confirms");
            assert_eq!(p(0x08, 0, 0, MID, MID).buttons, B, "A goes back");
            assert_eq!(p(0x01, 0, 0, MID, MID).buttons, X, "Y is west");
            assert_eq!(p(0x02, 0, 0, MID, MID).buttons, Y, "X is north");
            assert_eq!(p(0x40, 0, 0x40, MID, MID).buttons, LB | RB, "L R");
            let t = p(0x80, 0, 0x80, MID, MID);
            assert_eq!((t.lt, t.rt, t.buttons), (true, true, 0), "ZL ZR are triggers");
            assert_eq!(p(0, 0x01, 0, MID, MID).buttons, VIEW, "minus is View");
            assert_eq!(p(0, 0x02, 0, MID, MID).buttons, MENU, "plus is Menu");
            assert_eq!(p(0, 0x04 | 0x08, 0, MID, MID).buttons, L3 | R3, "stick clicks");
            assert_eq!(p(0, 0x10, 0, MID, MID).buttons, GUIDE, "Home is Guide");
            assert_eq!(p(0, 0x20, 0, MID, MID).buttons, 0, "Capture isn't mapped");
            assert_eq!(p(0, 0, 0x01 | 0x08, MID, MID).buttons, DOWN | LEFT, "D-pad");
            assert_eq!(p(0, 0, 0x02 | 0x04, MID, MID).buttons, UP | RIGHT, "D-pad");
            // up on a Nintendo stick is larger; ours is y down
            let s = p(0, 0, 0, (448, 3648), (3648, 448));
            assert_eq!(steps(&s), [-20, -20, 20, 20], "full travel");
            let s = p(0, 0, 0, (2848, 2048), (2048, 2848));
            assert_eq!(steps(&s), [10, 0, 0, -10], "half way");
            // a 0x21 reply carries the same state
            let mut reply = pro_full(0x04, 0, 0, MID, MID);
            reply[0] = 0x21;
            assert_eq!(parse(Model::SwitchPro, bt, &reply).unwrap().buttons, A, "0x21");
        }
    }

    #[test]
    fn switch_simple_reports_read_by_position() {
        let c = 0x8000;
        let p = |b1, b2, hat, axes| parse(Model::SwitchPro, true, &pro_simple(b1, b2, hat, axes)).unwrap();
        let rest = p(0, 0, 8, [c; 4]);
        assert_eq!((rest.buttons, rest.lt, rest.rt, steps(&rest)), (0, false, false, [0; 4]), "at rest");
        assert_eq!(p(0x01, 0, 8, [c; 4]).buttons, A, "B confirms");
        assert_eq!(p(0x02, 0, 8, [c; 4]).buttons, B, "A goes back");
        assert_eq!(p(0x04, 0, 8, [c; 4]).buttons, X, "Y is west");
        assert_eq!(p(0x08, 0, 8, [c; 4]).buttons, Y, "X is north");
        assert_eq!(p(0x10 | 0x20, 0, 8, [c; 4]).buttons, LB | RB, "L R");
        let t = p(0x40 | 0x80, 0, 8, [c; 4]);
        assert_eq!((t.lt, t.rt, t.buttons), (true, true, 0), "ZL ZR");
        assert_eq!(p(0, 0x01 | 0x02, 8, [c; 4]).buttons, VIEW | MENU, "minus plus");
        assert_eq!(p(0, 0x04 | 0x08, 8, [c; 4]).buttons, L3 | R3, "stick clicks");
        assert_eq!(p(0, 0x10, 8, [c; 4]).buttons, GUIDE, "Home is Guide");
        assert_eq!(p(0, 0, 2, [c; 4]).buttons, RIGHT, "hat");
        assert_eq!(p(0, 0, 5, [c; 4]).buttons, DOWN | LEFT, "hat");
        // y down already in simple mode
        assert_eq!(steps(&p(0, 0, 8, [0, 0, 0xFFFF, 0xFFFF])), [-20, -20, 20, 20], "full travel");
    }

    #[test]
    fn models_come_from_the_ids() {
        assert_eq!(Model::from_ids(0x054C, 0x0CE6), Some(Model::DualSense));
        assert_eq!(Model::from_ids(0x054C, 0x0DF2), Some(Model::DualSense));
        assert_eq!(Model::from_ids(0x054C, 0x05C4), Some(Model::DualShock4));
        assert_eq!(Model::from_ids(0x054C, 0x09CC), Some(Model::DualShock4));
        assert_eq!(Model::from_ids(0x057E, 0x2009), Some(Model::SwitchPro));
        assert_eq!(Model::from_ids(0x057E, 0x2006), None, "a single Joy-Con (later)");
        assert_eq!(Model::from_ids(0x045E, 0x0B13), None, "Xbox pads are XInput's");
        assert_eq!(Model::DualSense.label(), "PlayStation controller");
        assert_eq!(Model::SwitchPro.label(), "Switch controller");
        assert!(is_bluetooth(0x0CE6, 78) && !is_bluetooth(0x0CE6, 64) && is_bluetooth(0x0BA0, 64));
        assert!(maybe_ours(r"\\?\hid#vid_054c&pid_0ce6&mi_03#8&1&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
        assert!(maybe_ours(r"\\?\HID#{00001124-0000-1000-8000-00805f9b34fb}_VID&0002057e_PID&2009#9&2&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
        assert!(!maybe_ours(r"\\?\hid#vid_046d&pid_c52b&mi_00#7&1&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
    }

    #[test]
    fn a_virtual_pad_under_a_root_device_is_skipped() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let usb = s(&[r"HID\VID_054C&PID_0CE6&MI_03\8&1", r"USB\VID_054C&PID_0CE6&MI_03\7&2", r"USB\VID_054C&PID_0CE6\5&3", r"USB\ROOT_HUB30\4&1", r"PCI\VEN_8086&DEV_A0ED\3&1", r"ACPI\PNP0A08\0", r"ROOT\ACPI_HAL\0000", r"HTREE\ROOT\0"]);
        let bt = s(&[r"HID\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&09CC\9&1", r"BTHENUM\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&09CC\8&2", r"BTH\MS_BTHBRB\7&3", r"USB\VID_8087&PID_0026\6&4", r"PCI\VEN_8086&DEV_A0ED\3&1"]);
        let vigem = s(&[r"HID\VID_054C&PID_05C4\9&1", r"USB\VID_054C&PID_05C4\1", r"ROOT\SYSTEM\0001", r"HTREE\ROOT\0"]);
        // a real pad's chain reaches ROOT\ACPI_HAL too, but through PCI: the first bus above its stack decides
        assert!(!is_virtual(&usb), "a real USB pad");
        assert!(!is_virtual(&bt), "a real Bluetooth pad");
        assert!(is_virtual(&vigem), "ViGEm's DualShock 4");
        assert!(!is_virtual(&usb[..1]), "no parents known: read it");
    }

    /// The live check: `cargo test --lib real_hid_pads -- --ignored --nocapture`, with a pad plugged in or paired.
    #[test]
    #[ignore = "real hardware: opens the PlayStation and Switch pads present and reads a second of reports"]
    fn real_hid_pads() {
        let paths: Vec<_> = present().into_iter().filter(|p| maybe_ours(&String::from_utf16_lossy(p))).collect();
        if paths.is_empty() {
            println!("no Sony or Nintendo HID interface present");
        }
        for path in paths {
            let text = String::from_utf16_lossy(&path[..path.len() - 1]);
            let chain = chain(&path);
            println!("{text}\n  chain {chain:?}, virtual {}", is_virtual(&chain));
            let Some(dev) = open(&path) else {
                println!("  not a pad we read");
                continue;
            };
            println!("  {:?}, bluetooth {}, input {} bytes, output {}", dev.model, dev.bluetooth, dev.input_len, dev.output_len);
            unsafe { CloseHandle(dev.handle).ok() };
        }
    }

    #[test]
    fn several_pads_get_their_own_slots_and_changes() {
        // the test pad and two device paths share the registry; slots are the lowest free from 4
        let a = add("pad-a-test", Model::DualSense, false).unwrap();
        let b = add("pad-b-test", Model::SwitchPro, true).unwrap();
        assert_ne!(a, b);
        assert!(a >= 4 && b >= 4);
        assert_eq!(add("pad-a-test", Model::DualSense, false), None, "a path opens once");
        let cross = ds5(64, 0x01, 1, REST, [0x08 | 0x20, 0, 0], 0, 0);
        assert!(feed(a, &cross));
        assert!(!feed(a, &cross), "the same state again is no change");
        let mut noisy = cross.clone();
        noisy[16] = 0x77; // gyro bytes move all the time
        assert!(!feed(a, &noisy), "bytes Padwake doesn't read are no change");
        let seen = pads();
        let pa = seen.iter().find(|p| p.slot == a).unwrap();
        let pb = seen.iter().find(|p| p.slot == b).unwrap();
        assert_eq!((pa.buttons, pa.seq), (A, 1));
        assert_eq!((pb.buttons, pb.seq), (0, 0));
        remove(a);
        remove(b);
        assert!(pads().iter().all(|p| p.slot != a && p.slot != b), "gone when unplugged");
    }
}
