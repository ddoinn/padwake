//! Sound output: the active playback devices, the default one (set through Windows' undocumented but stable
//! IPolicyConfig, as the Sound control panel does) and the default device's master volume.

use super::{com, err};
use serde::Serialize;
use std::ffi::c_void;
use windows::core::{Interface, GUID, HRESULT, HSTRING, IUnknown, PCWSTR, PWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutput {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// 0..1, as the Windows volume slider shows it
    pub level: f32,
    pub muted: bool,
}

// PKEY_Device_FriendlyName ("Speakers (Realtek(R) Audio)")
const FRIENDLY_NAME: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

// CPolicyConfigClient / IPolicyConfig (Windows 7 and later)
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

/// IPolicyConfig's vtable up to SetDefaultEndpoint: IUnknown's 3 methods, then GetMixFormat, GetDeviceFormat,
/// ResetDeviceFormat, SetDeviceFormat, GetProcessingPeriod, SetProcessingPeriod, GetShareMode, SetShareMode,
/// GetPropertyValue and SetPropertyValue, which we never call.
#[repr(C)]
struct PolicyConfigVtbl {
    _unknown: [usize; 3],
    _unused: [usize; 10],
    set_default_endpoint: unsafe extern "system" fn(this: *mut c_void, id: PCWSTR, role: ERole) -> HRESULT,
}

fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    com();
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)
}

fn id_of(d: &IMMDevice) -> Result<String, String> {
    unsafe { d.GetId() }.map(take).map_err(err)
}

fn name_of(d: &IMMDevice) -> Option<String> {
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let mut v = store.GetValue(&FRIENDLY_NAME).ok()?;
        let name = PropVariantToStringAlloc(&v).map(take).ok();
        let _ = PropVariantClear(&mut v);
        name.filter(|n| !n.is_empty())
    }
}

/// Active playback devices, in Windows' order.
pub fn outputs() -> Result<Vec<AudioOutput>, String> {
    let e = enumerator()?;
    let default = unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) }.ok().and_then(|d| id_of(&d).ok());
    let all = unsafe { e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }.map_err(err)?;
    let n = unsafe { all.GetCount() }.map_err(err)?;
    let mut list = Vec::new();
    for i in 0..n {
        let d = unsafe { all.Item(i) }.map_err(err)?;
        let id = id_of(&d)?;
        let name = name_of(&d).unwrap_or_else(|| "Audio device".into());
        list.push(AudioOutput { is_default: default.as_deref() == Some(id.as_str()), id, name });
    }
    Ok(list)
}

/// Makes `id` the default playback device for every role (console, multimedia, communications).
pub fn set_default(id: &str) -> Result<(), String> {
    if !outputs()?.iter().any(|o| o.id == id) {
        return Err(format!("no active audio output {id}"));
    }
    com();
    let unknown: IUnknown = unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }.map_err(err)?;
    let mut raw: *mut c_void = std::ptr::null_mut();
    unsafe { unknown.query(&IID_POLICY_CONFIG, &mut raw) }.ok().map_err(err)?;
    // owns the reference QueryInterface added, and releases it on drop
    let _policy = unsafe { IUnknown::from_raw(raw) };
    let vtbl = unsafe { &**(raw as *const *const PolicyConfigVtbl) };
    let wide = HSTRING::from(id);
    for role in [eConsole, eMultimedia, eCommunications] {
        unsafe { (vtbl.set_default_endpoint)(raw, PCWSTR(wide.as_ptr()), role) }.ok().map_err(err)?;
    }
    super::log(&format!("audio: default output {id}"));
    Ok(())
}

fn endpoint_volume() -> Result<IAudioEndpointVolume, String> {
    let e = enumerator()?;
    let d = unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(|e| format!("no audio output: {e}"))?;
    unsafe { d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.map_err(err)
}

pub fn volume() -> Result<Volume, String> {
    let v = endpoint_volume()?;
    let level = unsafe { v.GetMasterVolumeLevelScalar() }.map_err(err)?;
    let muted = unsafe { v.GetMute() }.map_err(err)?.as_bool();
    Ok(Volume { level, muted })
}

pub fn set_volume(level: f32) -> Result<(), String> {
    if !level.is_finite() {
        return Err("volume must be a number".into());
    }
    let v = endpoint_volume()?;
    unsafe { v.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()) }.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_id() -> String {
        outputs().unwrap().into_iter().find(|o| o.is_default).expect("a default output").id
    }

    /// Real effect on this PC: switches the default output and back, or (one output) moves the volume and back.
    #[test]
    #[ignore = "real hardware: changes the default sound output or the volume, then restores it"]
    fn real_audio_switch_and_restore() {
        let list = outputs().unwrap();
        for o in &list {
            println!("output: {}{} [{}]", o.name, if o.is_default { " (default)" } else { "" }, o.id);
        }
        if list.len() >= 2 {
            let before = default_id();
            let other = list.iter().find(|o| o.id != before).unwrap().id.clone();
            set_default(&other).unwrap();
            assert_eq!(default_id(), other);
            println!("switched default to {other}");
            set_default(&before).unwrap();
            assert_eq!(default_id(), before);
            println!("switched back to {before}");
        } else {
            // one output: IPolicyConfig can still be exercised by making the default the default again
            let id = default_id();
            set_default(&id).unwrap();
            assert_eq!(default_id(), id);
            println!("set default to the same output again: ok (IPolicyConfig::SetDefaultEndpoint returned S_OK)");
            let before = volume().unwrap();
            println!("volume before: {:.2} muted {}", before.level, before.muted);
            let target = if before.level > 0.5 { before.level - 0.2 } else { before.level + 0.2 };
            set_volume(target).unwrap();
            let now = volume().unwrap().level;
            println!("volume set to {target:.2}: reads {now:.2}");
            assert!((now - target).abs() < 0.01);
            set_volume(before.level).unwrap();
            let back = volume().unwrap().level;
            println!("volume restored: reads {back:.2}");
            assert!((back - before.level).abs() < 0.01);
        }
    }
}
