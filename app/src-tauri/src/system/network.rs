//! Network status (Wi-Fi or Ethernet), nearby Wi-Fi networks and connecting to one with a WPA2/WPA3-personal
//! profile, through the Native Wifi API (wlanapi) and IP Helper.

use super::log;
use serde::Serialize;
use std::ffi::c_void;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_MULTICAST, IF_TYPE_ETHERNET_CSMACD, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, dot11_radio_state_off, wlan_connection_mode_profile, wlan_interface_state_connected,
    wlan_intf_opcode_current_connection, wlan_intf_opcode_radio_state, WlanCloseHandle, WlanConnect, WlanEnumInterfaces, WlanFreeMemory,
    WlanGetAvailableNetworkList, WlanOpenHandle, WlanQueryInterface, WlanScan, WlanSetProfile, DOT11_AUTH_ALGORITHM,
    DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_RSNA, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA,
    DOT11_AUTH_ALGO_WPA3, DOT11_AUTH_ALGO_WPA3_ENT, DOT11_AUTH_ALGO_WPA3_SAE, DOT11_SSID, WLAN_AVAILABLE_NETWORK,
    WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_AVAILABLE_NETWORK_HAS_PROFILE, WLAN_AVAILABLE_NETWORK_LIST,
    WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST,
    WLAN_RADIO_STATE,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    /// "wifi", "ethernet" or "none"
    pub kind: &'static str,
    pub connected: bool,
    pub adapter: String,
    pub ssid: Option<String>,
    /// 0..100
    pub signal: Option<u32>,
    pub ip: Option<String>,
    /// The Wi-Fi radio: "on", "off" (turned off in Windows or by a switch) or "none" (no Wi-Fi adapter)
    pub wifi: &'static str,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Wifi {
    pub ssid: String,
    /// 0..100
    pub signal: u32,
    pub secure: bool,
    /// "open", "wpa2", "wpa3", "enterprise" or "other"
    pub auth: &'static str,
    pub connected: bool,
    /// Windows has a profile for it (connecting needs no password)
    pub saved: bool,
}

// ---------- wlanapi ----------

struct Client(HANDLE);

impl Client {
    fn open() -> Result<Self, String> {
        let (mut version, mut h) = (0u32, HANDLE::default());
        match unsafe { WlanOpenHandle(2, None, &mut version, &mut h) } {
            0 => Ok(Client(h)),
            e => Err(format!("Wi-Fi isn't available (error {e})")),
        }
    }

    fn interfaces(&self) -> Vec<WLAN_INTERFACE_INFO> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(self.0, None, &mut list) != 0 || list.is_null() {
                return Vec::new();
            }
            let items =
                std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), (*list).dwNumberOfItems as usize).to_vec();
            WlanFreeMemory(list as *const c_void);
            items
        }
    }

    fn connection(&self, guid: &GUID) -> Option<WLAN_CONNECTION_ATTRIBUTES> {
        unsafe {
            let (mut size, mut data) = (0u32, std::ptr::null_mut::<c_void>());
            if WlanQueryInterface(self.0, guid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None)
                != 0
                || data.is_null()
            {
                return None;
            }
            let a = *(data as *const WLAN_CONNECTION_ATTRIBUTES);
            WlanFreeMemory(data);
            Some(a)
        }
    }

    /// False when every radio on the interface is off in software (Windows' Wi-Fi switch) or hardware.
    fn radio_on(&self, guid: &GUID) -> bool {
        unsafe {
            let (mut size, mut data) = (0u32, std::ptr::null_mut::<c_void>());
            if WlanQueryInterface(self.0, guid, wlan_intf_opcode_radio_state, None, &mut size, &mut data, None) != 0
                || data.is_null()
            {
                return true;
            }
            let r = &*(data as *const WLAN_RADIO_STATE);
            let n = (r.dwNumberOfPhys as usize).min(r.PhyRadioState.len());
            let on = n == 0
                || r.PhyRadioState[..n].iter().any(|p| {
                    p.dot11SoftwareRadioState != dot11_radio_state_off && p.dot11HardwareRadioState != dot11_radio_state_off
                });
            WlanFreeMemory(data);
            on
        }
    }

    fn available(&self, guid: &GUID) -> Vec<WLAN_AVAILABLE_NETWORK> {
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(self.0, guid, 0, None, &mut list) != 0 || list.is_null() {
                return Vec::new();
            }
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), (*list).dwNumberOfItems as usize).to_vec();
            WlanFreeMemory(list as *const c_void);
            items
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

fn ssid_of(s: &DOT11_SSID) -> String {
    String::from_utf8_lossy(&s.ucSSID[..(s.uSSIDLength as usize).min(32)]).into_owned()
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

/// What we can connect to: personal WPA2 and WPA3, and open networks. WEP, WPA1, OWE and enterprise are "other" or
/// "enterprise" and need Windows' own settings.
fn auth_name(algo: DOT11_AUTH_ALGORITHM, secure: bool) -> &'static str {
    match algo {
        DOT11_AUTH_ALGO_80211_OPEN if !secure => "open",
        DOT11_AUTH_ALGO_RSNA_PSK => "wpa2",
        DOT11_AUTH_ALGO_WPA3_SAE => "wpa3",
        DOT11_AUTH_ALGO_WPA | DOT11_AUTH_ALGO_RSNA | DOT11_AUTH_ALGO_WPA3 | DOT11_AUTH_ALGO_WPA3_ENT => "enterprise",
        _ => "other",
    }
}

/// One entry per SSID (Windows lists a network once per profile too): the strongest signal and any flag set on any
/// entry; connected first, then by signal.
pub fn dedupe(list: Vec<Wifi>) -> Vec<Wifi> {
    let mut out: Vec<Wifi> = Vec::new();
    for w in list {
        match out.iter_mut().find(|o| o.ssid == w.ssid) {
            Some(o) => {
                o.signal = o.signal.max(w.signal);
                o.connected |= w.connected;
                o.saved |= w.saved;
                if o.auth == "other" {
                    o.auth = w.auth;
                }
            }
            None => out.push(w),
        }
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)).then(a.ssid.cmp(&b.ssid)));
    out
}

/// Nearby networks. `scan` asks the adapter for a fresh scan too; it finishes in a few seconds, so the list returned
/// now is the last scan's (Windows scans on its own as well) and a second call a few seconds later sees the new one.
pub fn wifi_list(scan: bool) -> Result<Vec<Wifi>, String> {
    let Ok(c) = Client::open() else { return Ok(Vec::new()) };
    let mut all = Vec::new();
    for i in c.interfaces() {
        if scan {
            let _ = unsafe { WlanScan(c.0, &i.InterfaceGuid, None, None, None) };
        }
        for n in c.available(&i.InterfaceGuid) {
            let ssid = ssid_of(&n.dot11Ssid);
            if ssid.is_empty() {
                continue;
            }
            let secure = n.bSecurityEnabled.as_bool();
            all.push(Wifi {
                ssid,
                signal: n.wlanSignalQuality.min(100),
                secure,
                auth: auth_name(n.dot11DefaultAuthAlgorithm, secure),
                connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                saved: n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
            });
        }
    }
    Ok(dedupe(all))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A WLAN profile for an open, WPA2-personal or WPA3-personal network. The profile is named after the SSID, as
/// Windows names the ones it makes.
pub fn profile_xml(ssid: &str, auth: &str, password: &str) -> Result<String, String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("a network name is 1 to 32 bytes".into());
    }
    let security = match auth {
        "open" => "<authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption>".to_string(),
        "wpa2" | "wpa3" => {
            let hex_key = password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit());
            if !hex_key && !(8..=63).contains(&password.chars().count()) {
                return Err("The password must be 8 to 63 characters.".into());
            }
            let method = if auth == "wpa2" { "WPA2PSK" } else { "WPA3SAE" };
            let key_type = if hex_key { "networkKey" } else { "passPhrase" };
            format!(
                "<authEncryption><authentication>{method}</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>\
                 <sharedKey><keyType>{key_type}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(password)
            )
        }
        _ => return Err("This network's security type isn't supported here. Use Windows' Wi-Fi settings.".into()),
    };
    let name = xml_escape(ssid);
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    Ok(format!(
        "<?xml version=\"1.0\"?>\
         <WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
         <name>{name}</name>\
         <SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>\
         <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>\
         <MSM><security>{security}</security></MSM>\
         </WLANProfile>"
    ))
}

/// Connects to a nearby network: with its saved profile when `password` is empty and Windows has one, otherwise
/// with a new profile built from the password. Returns once Windows has started connecting.
pub fn connect(ssid: &str, password: &str) -> Result<(), String> {
    let c = Client::open()?;
    for i in c.interfaces() {
        let Some(n) = c.available(&i.InterfaceGuid).into_iter().find(|n| ssid_of(&n.dot11Ssid) == ssid) else {
            continue;
        };
        let saved = n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0;
        let profile = if saved && password.is_empty() {
            wide(&n.strProfileName)
        } else {
            let auth = auth_name(n.dot11DefaultAuthAlgorithm, n.bSecurityEnabled.as_bool());
            let xml = HSTRING::from(profile_xml(ssid, auth, password)?);
            let mut reason = 0u32;
            let r = unsafe { WlanSetProfile(c.0, &i.InterfaceGuid, 0, &xml, PCWSTR::null(), true, None, &mut reason) };
            if r != 0 {
                return Err(format!("Windows didn't accept the network profile (error {r}, reason {reason})"));
            }
            ssid.to_string()
        };
        let name = HSTRING::from(profile.as_str());
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(name.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let r = unsafe { WlanConnect(c.0, &i.InterfaceGuid, &params, None) };
        log(&format!("network: connect to {ssid:?} with profile {profile:?}: {}", if r == 0 { "started" } else { "failed" }));
        return if r == 0 { Ok(()) } else { Err(format!("couldn't connect (error {r})")) };
    }
    Err(format!("{ssid} isn't in range"))
}

// ---------- status ----------

struct Adapter {
    guid: String,
    name: String,
    if_type: u32,
    up: bool,
    gateway: bool,
    ipv4: Option<String>,
}

fn adapters() -> Vec<Adapter> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut buf: Vec<u64> = vec![0; 2048];
    let mut size = (buf.len() * 8) as u32;
    for _ in 0..3 {
        let first = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let r = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) };
        if r == ERROR_BUFFER_OVERFLOW.0 {
            buf = vec![0; size as usize / 8 + 1];
            continue;
        }
        if r != 0 {
            return Vec::new();
        }
        let mut list = Vec::new();
        let mut p = first as *const IP_ADAPTER_ADDRESSES_LH;
        while !p.is_null() {
            let a = unsafe { &*p };
            let mut ipv4 = None;
            let mut u = a.FirstUnicastAddress;
            while !u.is_null() && ipv4.is_none() {
                let addr = unsafe { &*u }.Address.lpSockaddr;
                if !addr.is_null() && unsafe { (*addr).sa_family } == AF_INET {
                    let b = unsafe { (*(addr as *const SOCKADDR_IN)).sin_addr.S_un.S_addr }.to_ne_bytes();
                    ipv4 = Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
                }
                u = unsafe { &*u }.Next;
            }
            list.push(Adapter {
                guid: unsafe { a.AdapterName.to_string() }.unwrap_or_default(),
                name: unsafe { a.FriendlyName.to_string() }.unwrap_or_default(),
                if_type: a.IfType,
                up: a.OperStatus == IfOperStatusUp,
                gateway: !a.FirstGatewayAddress.is_null(),
                ipv4,
            });
            p = a.Next;
        }
        return list;
    }
    Vec::new()
}

fn same_guid(text: &str, guid: &GUID) -> bool {
    text.trim_matches(|c| c == '{' || c == '}').eq_ignore_ascii_case(&format!("{guid:?}"))
}

pub fn status() -> Result<Net, String> {
    let adapters = adapters();
    let mut wifi = "none";
    if let Ok(c) = Client::open() {
        for i in c.interfaces() {
            if wifi != "on" {
                wifi = if c.radio_on(&i.InterfaceGuid) { "on" } else { "off" };
            }
            if i.isState != wlan_interface_state_connected {
                continue;
            }
            let Some(a) = c.connection(&i.InterfaceGuid) else { continue };
            let ad = adapters.iter().find(|x| same_guid(&x.guid, &i.InterfaceGuid));
            return Ok(Net {
                kind: "wifi",
                connected: true,
                adapter: ad.map(|x| x.name.clone()).unwrap_or_else(|| wide(&i.strInterfaceDescription)),
                ssid: Some(ssid_of(&a.wlanAssociationAttributes.dot11Ssid)),
                signal: Some(a.wlanAssociationAttributes.wlanSignalQuality.min(100)),
                ip: ad.and_then(|x| x.ipv4.clone()),
                wifi: "on",
            });
        }
    }
    if let Some(e) = adapters.iter().find(|a| a.up && a.gateway && a.if_type == IF_TYPE_ETHERNET_CSMACD) {
        return Ok(Net { kind: "ethernet", connected: true, adapter: e.name.clone(), ip: e.ipv4.clone(), wifi, ..Default::default() });
    }
    Ok(Net { kind: "none", wifi, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpa2_profile_escapes_and_has_hex_ssid() {
        let xml = profile_xml("Café & <Home>", "wpa2", "p\"ss&w<rd'1").unwrap();
        assert!(xml.contains("<name>Café &amp; &lt;Home&gt;</name>"));
        assert!(xml.contains("<hex>436166C3A92026203C486F6D653E</hex>"));
        assert!(xml.contains("<authentication>WPA2PSK</authentication><encryption>AES</encryption>"));
        assert!(xml.contains("<keyType>passPhrase</keyType>"));
        assert!(xml.contains("<keyMaterial>p&quot;ss&amp;w&lt;rd&apos;1</keyMaterial>"));
        assert!(xml.contains("<connectionType>ESS</connectionType>"));
    }

    #[test]
    fn wpa3_and_open_profiles() {
        let xml = profile_xml("Lab", "wpa3", "correct horse").unwrap();
        assert!(xml.contains("<authentication>WPA3SAE</authentication><encryption>AES</encryption>"));
        assert!(xml.contains("<keyMaterial>correct horse</keyMaterial>"));
        let open = profile_xml("Cafe", "open", "ignored").unwrap();
        assert!(open.contains("<authentication>open</authentication><encryption>none</encryption>"));
        assert!(!open.contains("sharedKey"));
        let hex = profile_xml("Lab", "wpa2", &"ab".repeat(32)).unwrap();
        assert!(hex.contains("<keyType>networkKey</keyType>"));
    }

    #[test]
    fn bad_profiles_are_refused() {
        assert!(profile_xml("Lab", "wpa2", "short").is_err());
        assert!(profile_xml("Lab", "wpa2", &"x".repeat(64)).is_err());
        assert!(profile_xml("Lab", "enterprise", "whatever1").is_err());
        assert!(profile_xml("", "open", "").is_err());
        assert!(profile_xml(&"s".repeat(33), "open", "").is_err());
    }

    #[test]
    fn networks_dedupe_by_ssid_connected_first() {
        let w = |ssid: &str, signal, connected, saved| Wifi {
            ssid: ssid.into(),
            signal,
            secure: true,
            auth: "wpa2",
            connected,
            saved,
        };
        let list = dedupe(vec![w("B", 40, false, false), w("A", 90, false, false), w("C", 20, false, true), w("B", 70, true, false), w("C", 30, false, false)]);
        let names: Vec<_> = list.iter().map(|n| (n.ssid.as_str(), n.signal, n.connected, n.saved)).collect();
        assert_eq!(names, vec![("B", 70, true, false), ("A", 90, false, false), ("C", 30, false, true)]);
    }

    #[test]
    #[ignore = "real hardware: lists nearby Wi-Fi networks and the connection (never connects)"]
    fn real_wifi_list_and_status() {
        println!("status: {:?}", status().unwrap());
        let first = wifi_list(true).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(4));
        let list = wifi_list(false).unwrap();
        println!("{} networks right away, {} after the scan:", first.len(), list.len());
        for n in &list {
            println!("  {:3}% {:10} {}{}{}", n.signal, n.auth, n.ssid, if n.connected { " (connected)" } else { "" }, if n.saved { " (saved)" } else { "" });
        }
    }
}
