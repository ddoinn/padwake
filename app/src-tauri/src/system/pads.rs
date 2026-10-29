//! Connected pads: battery, how each one connects, and its product name, for Settings › Controllers and Control
//! Center. Asked only when those open (no polling).
//!
//! XInput alone can't tell a 2.4 GHz USB receiver from a cable: receivers such as 8BitDo's emulate a wired Xbox 360
//! pad, so XInput says `BATTERY_TYPE_WIRED` and reports no battery. So each slot's USB VID/PID comes from the
//! undocumented `XInputGetCapabilitiesEx` (xinput1_4.dll ordinal 108, as SDL uses it), and the USB device's
//! bus-reported description (the product string, e.g. "8BitDo Ultimate 2C Wireless Controller") from CfgMgr32.
//! `classify` turns those into wired / wireless / receiver.

use serde::Serialize;
use std::sync::OnceLock;
use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW, CM_Locate_DevNodeW,
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT, CM_LOCATE_DEVNODE_NORMAL, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{DEVPKEY_Device_BusReportedDeviceDesc, DEVPROPTYPE, DEVPROP_TYPE_STRING};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Input::XboxController::{
    XInputGetBatteryInformation, BATTERY_DEVTYPE_GAMEPAD, BATTERY_TYPE_ALKALINE,
    BATTERY_TYPE_DISCONNECTED, BATTERY_TYPE_NIMH, BATTERY_TYPE_WIRED, XINPUT_BATTERY_INFORMATION, XINPUT_CAPABILITIES,
    XINPUT_CAPS_WIRELESS,
};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    /// a USB cable
    Wired,
    /// XInput reports a battery: an Xbox Wireless Adapter, an Xbox 360 receiver, Bluetooth
    Wireless,
    /// a 2.4 GHz USB receiver that XInput sees as a wired pad (no battery reported)
    Receiver,
}

/// A connected pad. `level` is 0 (empty) to 3 (full) and means something only when `kind` isn't "wired".
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PadBattery {
    pub slot: u32,
    /// XInput's battery type: wired | alkaline | nimh | unknown
    pub kind: &'static str,
    pub level: u8,
    pub connection: Connection,
    /// the product name, when the device reports a real one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// USB VID/PID (0 when unknown), for the live check; not sent to the page
    #[serde(skip)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub vid: u16,
    #[serde(skip)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub pid: u16,
}

use Connection::{Receiver, Wired};

/// Pads XInput sees as wired whose link we know. Sources: Linux `drivers/input/joystick/xpad.c` and SDL
/// `src/joystick/controller_list.h` (2026-10). PIDs a pad also uses on a cable (8BitDo 2DC8:3106, "Ultimate Wireless /
/// Pro 2 Wired") are left out on purpose: the description decides those.
const KNOWN: &[(u16, u16, Connection, &str)] = &[
    // 8BitDo receivers (XInput mode)
    (0x2DC8, 0x310A, Receiver, "8BitDo Ultimate 2C Wireless Controller"), // xpad, SDL; verified with PnP
    (0x2DC8, 0x310B, Receiver, "8BitDo Ultimate 2 Wireless Controller"),  // xpad
    (0x2DC8, 0x3109, Receiver, "8BitDo Ultimate Wireless Controller"),    // xpad: "Ultimate Wireless Bluetooth"
    // 8BitDo cable-only pads: always wired
    (0x2DC8, 0x2000, Wired, "8BitDo Pro 2 Wired Controller for Xbox"),
    (0x2DC8, 0x201E, Wired, "8BitDo Pro 2 Wired Controller for Xbox"),
    (0x2DC8, 0x2002, Wired, "8BitDo Ultimate Wired Controller for Xbox"),
    // other dongles xpad names as such
    (0x1532, 0x0A59, Receiver, "Razer Wolverine V3 Pro"),
    (0x1B1C, 0x2B2B, Receiver, "Corsair Novablade Pro"),
    (0x20BC, 0x5134, Receiver, "BETOP BTP-KP50B"),
    (0x20BC, 0x514A, Receiver, "BETOP BTP-KP50C"),
    (0x03F0, 0x048D, Receiver, "HyperX Clutch"),
];

const RECEIVER_WORDS: &[&str] = &["wireless", "receiver", "2.4g", "dongle", "adapter"];

/// How a pad connects. `battery` is XInput's battery type, `caps_wireless` the XINPUT_CAPS_WIRELESS flag, `vid`/`pid`
/// from XInputGetCapabilitiesEx (0 when unknown), `desc` the USB bus-reported description.
pub fn classify(battery: u8, caps_wireless: bool, vid: u16, pid: u16, desc: Option<&str>) -> Connection {
    if battery != BATTERY_TYPE_WIRED.0 || caps_wireless {
        return Connection::Wireless;
    }
    if let Some(&(_, _, link, _)) = KNOWN.iter().find(|k| k.0 == vid && k.1 == pid) {
        return link;
    }
    let desc = desc.unwrap_or("").to_ascii_lowercase();
    if RECEIVER_WORDS.iter().any(|w| desc.contains(w)) {
        return Receiver;
    }
    Wired
}

/// A product name worth showing: the description without the "Controller (…)" wrapper of HID children, and not a
/// generic one ("Controller", "IDLE", …); else the known table's name.
pub fn product_name(vid: u16, pid: u16, desc: Option<&str>) -> Option<String> {
    let clean = desc.map(str::trim).map(|d| {
        d.strip_prefix("Controller (").and_then(|s| s.strip_suffix(')')).unwrap_or(d).trim()
    });
    const GENERIC: &[&str] =
        &["", "controller", "gamepad", "idle", "usb gamepad", "xbox 360 controller", "xbox 360 for windows"];
    clean
        .filter(|d| d.len() > 2 && !GENERIC.contains(&d.to_ascii_lowercase().as_str()))
        .map(String::from)
        .or_else(|| KNOWN.iter().find(|k| k.0 == vid && k.1 == pid).map(|k| k.3.to_string()))
}

/// XINPUT_CAPABILITIES plus what XInputGetCapabilitiesEx adds (SDL's XINPUT_CAPABILITIES_EX).
#[repr(C)]
#[derive(Default)]
struct CapsEx {
    caps: XINPUT_CAPABILITIES,
    vendor_id: u16,
    product_id: u16,
    product_version: u16,
    _unk1: u16,
    _unk2: u32,
}

/// (reserved, must be 1; user index; flags; out)
type GetCapsEx = unsafe extern "system" fn(u32, u32, u32, *mut CapsEx) -> u32;

fn get_caps_ex() -> Option<GetCapsEx> {
    static F: OnceLock<Option<GetCapsEx>> = OnceLock::new();
    *F.get_or_init(|| unsafe {
        let dll = LoadLibraryW(w!("xinput1_4.dll")).ok()?;
        let f = GetProcAddress(dll, PCSTR(108 as *const u8))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, GetCapsEx>(f))
    })
}

/// Present USB device IDs, e.g. `USB\VID_2DC8&PID_310A\DC0F57A65B`.
fn usb_device_ids() -> Vec<String> {
    let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_ID_List_SizeW(&mut len, w!("USB"), flags) != CR_SUCCESS {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize + 256]; // room for a device that arrives in between
        if CM_Get_Device_ID_ListW(w!("USB"), &mut buf, flags) != CR_SUCCESS {
            return vec![];
        }
        buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

fn bus_description(id: &str) -> Option<String> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut inst = 0u32;
        if CM_Locate_DevNodeW(&mut inst, PCWSTR(wide.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
            return None;
        }
        let mut ty = DEVPROPTYPE(0);
        let mut buf = [0u16; 256];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let key = &DEVPKEY_Device_BusReportedDeviceDesc;
        if CM_Get_DevNode_PropertyW(inst, key, &mut ty, Some(buf.as_mut_ptr().cast()), &mut size, 0) != CR_SUCCESS
            || ty != DEVPROP_TYPE_STRING
        {
            return None;
        }
        let text = String::from_utf16_lossy(&buf[..(size as usize / 2).min(buf.len())]);
        Some(text.trim_end_matches('\0').trim().to_string())
    }
}

/// The bus-reported description of the present USB device with this VID/PID: the device itself first, then its
/// interfaces (`&MI_xx`, `&IG_xx`).
fn usb_description(ids: &[String], vid: u16, pid: u16) -> Option<String> {
    let prefix = format!("USB\\VID_{vid:04X}&PID_{pid:04X}");
    let mut matches: Vec<&String> = ids.iter().filter(|id| id.to_ascii_uppercase().starts_with(&prefix)).collect();
    matches.sort_by_key(|id| !id[prefix.len()..].starts_with('\\'));
    matches.into_iter().find_map(|id| bus_description(id).filter(|d| !d.is_empty()))
}

pub fn pads() -> Vec<PadBattery> {
    let mut ids: Option<Vec<String>> = None; // listed once, and only when a pad is connected
    (0..4)
        .filter_map(|slot| {
            let mut info = XINPUT_BATTERY_INFORMATION::default();
            if unsafe { XInputGetBatteryInformation(slot, BATTERY_DEVTYPE_GAMEPAD, &mut info) } != 0 {
                return None;
            }
            let kind = match info.BatteryType {
                BATTERY_TYPE_DISCONNECTED => return None,
                BATTERY_TYPE_WIRED => "wired",
                BATTERY_TYPE_ALKALINE => "alkaline",
                BATTERY_TYPE_NIMH => "nimh",
                _ => "unknown",
            };
            let mut caps = CapsEx::default();
            let has_caps = get_caps_ex().is_some_and(|f| unsafe { f(1, slot, 0, &mut caps) } == 0);
            let (vid, pid) = if has_caps { (caps.vendor_id, caps.product_id) } else { (0, 0) };
            let desc = if vid != 0 {
                usb_description(ids.get_or_insert_with(usb_device_ids), vid, pid)
            } else {
                None
            };
            let caps_wireless = has_caps && caps.caps.Flags.0 & XINPUT_CAPS_WIRELESS.0 != 0;
            Some(PadBattery {
                slot,
                kind,
                level: info.BatteryLevel.0.min(3),
                connection: classify(info.BatteryType.0, caps_wireless, vid, pid, desc.as_deref()),
                name: product_name(vid, pid, desc.as_deref()),
                vid,
                pid,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use Connection::Wireless;

    const WIRED: u8 = 1;
    const ALKALINE: u8 = 2;
    const NIMH: u8 = 3;
    const UNKNOWN: u8 = 255;

    #[test]
    fn caps_ex_matches_sdls_layout() {
        assert_eq!(std::mem::size_of::<XINPUT_CAPABILITIES>(), 20);
        assert_eq!(std::mem::size_of::<CapsEx>(), 32);
        assert_eq!(std::mem::offset_of!(CapsEx, vendor_id), 20);
    }

    #[test]
    fn classify_table() {
        let cases: &[(&str, u8, bool, u16, u16, Option<&str>, Connection)] = &[
            ("8BitDo 2C on its receiver", WIRED, false, 0x2DC8, 0x310A, None, Receiver),
            ("same, with its description", WIRED, false, 0x2DC8, 0x310A, Some("8BitDo Ultimate 2C Wireless Controller"), Receiver),
            ("8BitDo Ultimate 2 receiver", WIRED, false, 0x2DC8, 0x310B, None, Receiver),
            ("8BitDo Pro 2 Wired for Xbox", WIRED, false, 0x2DC8, 0x2000, None, Wired),
            ("8BitDo Ultimate Wired for Xbox", WIRED, false, 0x2DC8, 0x2002, Some("8BitDo Ultimate Wired Controller for Xbox"), Wired),
            ("8BitDo 3106 on a cable", WIRED, false, 0x2DC8, 0x3106, Some("8BitDo Pro 2 Wired Controller"), Wired),
            ("8BitDo 3106 on its 2.4G dongle", WIRED, false, 0x2DC8, 0x3106, Some("8BitDo Ultimate Wireless Controller"), Receiver),
            ("unknown pad, description says receiver", WIRED, false, 0x1234, 0x5678, Some("Gamepad 2.4G Receiver"), Receiver),
            ("unknown pad, description says dongle", WIRED, false, 0x1234, 0x5678, Some("XInput DONGLE"), Receiver),
            ("Razer dongle", WIRED, false, 0x1532, 0x0A59, None, Receiver),
            ("official Xbox One pad on USB", WIRED, false, 0x045E, 0x02EA, Some("Controller"), Wired),
            ("no VID/PID, no description", WIRED, false, 0, 0, None, Wired),
            ("Xbox Wireless Adapter, NiMH", NIMH, false, 0x045E, 0x02FE, None, Wireless),
            ("Bluetooth Xbox pad, alkaline", ALKALINE, false, 0x045E, 0x0B13, None, Wireless),
            ("battery type unknown", UNKNOWN, false, 0, 0, None, Wireless),
            ("caps says wireless", WIRED, true, 0x045E, 0x0719, None, Wireless),
            ("battery wins over a wired table entry", ALKALINE, false, 0x2DC8, 0x2000, None, Wireless),
        ];
        for &(what, battery, caps, vid, pid, desc, want) in cases {
            assert_eq!(classify(battery, caps, vid, pid, desc), want, "{what}");
        }
    }

    #[test]
    fn product_names() {
        let d = |s| product_name(0x2DC8, 0x310A, Some(s));
        assert_eq!(d("8BitDo Ultimate 2C Wireless Controller").as_deref(), Some("8BitDo Ultimate 2C Wireless Controller"));
        assert_eq!(d("Controller (8BitDo Ultimate 2C Wireless Controller)").as_deref(), Some("8BitDo Ultimate 2C Wireless Controller"));
        // generic descriptions fall back to the table, and to nothing for pads it doesn't know
        assert_eq!(d("IDLE").as_deref(), Some("8BitDo Ultimate 2C Wireless Controller"));
        assert_eq!(product_name(0x045E, 0x02EA, Some("Controller")), None);
        assert_eq!(product_name(0x045E, 0x02EA, Some("  ")), None);
        assert_eq!(product_name(0, 0, None), None);
    }

    /// The live check: `cargo test --lib real_pads -- --ignored --nocapture`
    #[test]
    #[ignore = "real hardware: reads the connected pads"]
    fn real_pads() {
        let pads = pads();
        let ids = usb_device_ids();
        // USB devices from the known vendors (a receiver shows here even while its pad is off)
        for id in ids.iter().filter(|id| KNOWN.iter().any(|k| id.starts_with(&format!("USB\\VID_{:04X}&", k.0)))) {
            println!("present: {id} = {:?}", bus_description(id));
        }
        if pads.is_empty() {
            println!("no pad connected (switch one on and run again)");
        }
        for p in pads {
            println!(
                "slot {}: {:04X}:{:04X} battery {} level {}, description {:?} -> {:?}, name {:?}",
                p.slot, p.vid, p.pid, p.kind, p.level, usb_description(&ids, p.vid, p.pid), p.connection, p.name
            );
        }
    }
}
