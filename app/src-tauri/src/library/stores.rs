//! Games installed with a store's launcher: Steam, Epic Games, GOG, Xbox (Game Pass) and the EA app. Each is read from
//! what the launcher leaves on the PC, with no account and no network:
//! - Steam: `libraryfolders.vdf` and each library's `appmanifest_<appid>.acf`, under `SteamPath` (HKCU\Software\Valve\Steam).
//! - Epic Games: the `.item` manifests in `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`.
//! - GOG: the `<id>_is1` uninstall entries (publisher GOG.com) and each game's `goggame-<id>.info`.
//! - Xbox: the packages in HKCU's AppModel repository whose folder has a `MicrosoftGame.config` (PC Game Pass and
//!   Microsoft Store games made with the GDK).
//! - EA app: uninstall entries whose folder has `__Installer\installerdata.xml`.
//!
//! Steam, Epic and Xbox games start through their launcher (a URI); GOG and EA games from their exe (the EA app's
//! `origin2://` launch is reported broken, and an EA game's exe hands itself to the EA app when it needs to).
//! A store that isn't installed, or a file that can't be read, adds nothing; the library then treats its games as
//! uninstalled.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum StoreName {
    Steam,
    Epic,
    Gog,
    Xbox,
    Ea,
}

impl StoreName {
    /// The name in `settings.storesOff` and in the page.
    pub fn key(self) -> &'static str {
        match self {
            StoreName::Steam => "steam",
            StoreName::Epic => "epic",
            StoreName::Gog => "gog",
            StoreName::Xbox => "xbox",
            StoreName::Ea => "ea",
        }
    }
}

/// Where a library game came from and how it starts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub name: StoreName,
    /// the store's id for the game: Steam app id, Epic app name, GOG game id, Xbox package family name, EA offer id
    pub id: String,
    /// opened by its launcher (`steam://…`, `com.epicgames.launcher://…`, `shell:AppsFolder\…`); `None`: the exe starts it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// the exe's arguments, as one command-line string (GOG play tasks)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
}

/// One installed store game.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub store: Store,
    pub name: String,
    /// where its files are: the session tracks processes run from here
    pub folder: PathBuf,
    /// the exe, when the store names one
    pub exe: Option<PathBuf>,
    pub steam_app_id: Option<u64>,
}

/// Every installed game of the stores not in `off`, one per folder, A–Z by name.
pub fn installed(off: &[String]) -> Vec<Installed> {
    let on = |s: StoreName| !off.iter().any(|o| o.eq_ignore_ascii_case(s.key()));
    let mut all = Vec::new();
    if on(StoreName::Steam) {
        all.extend(steam_path().map(|p| steam(&p)).unwrap_or_default());
    }
    if on(StoreName::Epic) {
        if let Some(pd) = std::env::var_os("ProgramData") {
            all.extend(epic(&Path::new(&pd).join(r"Epic\EpicGamesLauncher\Data\Manifests")));
        }
    }
    if on(StoreName::Gog) || on(StoreName::Ea) {
        let entries = uninstall_entries();
        if on(StoreName::Gog) {
            all.extend(gog(&entries));
        }
        if on(StoreName::Ea) {
            all.extend(ea(&entries));
        }
    }
    if on(StoreName::Xbox) {
        all.extend(xbox(&packages()));
    }
    tidy(all)
}

/// One entry per folder (the first store wins), A–Z by name.
fn tidy(mut all: Vec<Installed>) -> Vec<Installed> {
    let mut seen = std::collections::HashSet::new();
    all.retain(|g| seen.insert(super::scan::key(&g.folder)));
    all.sort_by_key(|g| (g.name.to_lowercase(), super::scan::key(&g.folder)));
    all
}

// ---------- Steam ----------

/// Steam's tools and runtimes, which install like games.
const STEAM_TOOLS: [u64; 1] = [228980]; // Steamworks Common Redistributables
const STEAM_TOOL_PREFIX: [&str; 3] = ["Steamworks", "Proton", "Steam Linux Runtime"];
/// `StateFlags` bit: fully installed (not downloading, not uninstalled).
const STEAM_FULLY_INSTALLED: u64 = 4;

fn steam_path() -> Option<PathBuf> {
    reg_string(Hive::User, r"Software\Valve\Steam", "SteamPath")
        .or_else(|| reg_string(Hive::Machine, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"))
        .map(|p| PathBuf::from(p.replace('/', "\\")))
}

/// The installed games of the Steam client at `root`: its own library and those in `libraryfolders.vdf`.
pub fn steam(root: &Path) -> Vec<Installed> {
    let mut libraries = vec![root.to_path_buf()];
    if let Some(v) = read_text(&root.join(r"steamapps\libraryfolders.vdf")).map(|t| vdf(&t)) {
        if let Some(Vdf::Map(entries)) = v.get("libraryfolders") {
            for (_, e) in entries {
                // `"1" "D:\\SteamLibrary"` before 2021, `"1" { "path" "D:\\SteamLibrary" … }` since
                let path = match e {
                    Vdf::Str(s) => Some(s.as_str()),
                    Vdf::Map(_) => e.get("path").and_then(Vdf::as_str),
                };
                if let Some(p) = path {
                    let p = PathBuf::from(p.replace('/', "\\"));
                    if !libraries.iter().any(|l| super::scan::key(l) == super::scan::key(&p)) {
                        libraries.push(p);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for lib in libraries {
        let apps = lib.join("steamapps");
        let Ok(entries) = std::fs::read_dir(&apps) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            if let Some(g) = read_text(&e.path()).and_then(|t| steam_app(&t, &apps)) {
                out.push(g);
            }
        }
    }
    out
}

/// One `appmanifest_<appid>.acf`; `None` for a tool, a game still downloading, or a folder that isn't there.
fn steam_app(acf: &str, steamapps: &Path) -> Option<Installed> {
    let v = vdf(acf);
    let app = v.get("AppState")?;
    let id: u64 = app.get("appid")?.as_str()?.parse().ok()?;
    let name = app.get("name")?.as_str()?.trim().to_string();
    let flags: u64 = app.get("StateFlags").and_then(Vdf::as_str).and_then(|f| f.parse().ok()).unwrap_or(0);
    let dir = app.get("installdir")?.as_str()?;
    if STEAM_TOOLS.contains(&id) || STEAM_TOOL_PREFIX.iter().any(|p| name.starts_with(p)) || flags & STEAM_FULLY_INSTALLED == 0 {
        return None;
    }
    let folder = steamapps.join("common").join(dir);
    if name.is_empty() || dir.is_empty() || !folder.is_dir() {
        return None;
    }
    Some(Installed {
        store: Store { name: StoreName::Steam, id: id.to_string(), uri: Some(format!("steam://rungameid/{id}")), args: None },
        name,
        folder,
        exe: None,
        steam_app_id: Some(id),
    })
}

/// Valve's text KeyValues: `"key" "value"` and `"key" { … }`, `//` comments.
#[derive(Debug, PartialEq)]
enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    /// A key of a map, ignoring case (Steam isn't consistent: `AppState`, `appid`, `StateFlags`).
    fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }
}

enum Token {
    Text(String),
    Open,
    Close,
}

fn vdf(text: &str) -> Vdf {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(c) => s.push(c),
                            None => break,
                        },
                        c => s.push(c),
                    }
                }
                tokens.push(Token::Text(s));
            }
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Vdf::Map(vdf_map(&mut tokens.into_iter()))
}

/// Key–value pairs up to the closing brace (or the end).
fn vdf_map(it: &mut impl Iterator<Item = Token>) -> Vec<(String, Vdf)> {
    let mut m = Vec::new();
    while let Some(t) = it.next() {
        let key = match t {
            Token::Text(k) => k,
            Token::Close => break,
            Token::Open => continue,
        };
        match it.next() {
            Some(Token::Text(v)) => m.push((key, Vdf::Str(v))),
            Some(Token::Open) => m.push((key, Vdf::Map(vdf_map(it)))),
            _ => break,
        }
    }
    m
}

// ---------- Epic Games ----------

/// The games among the Epic launcher's manifests: not DLC, not engine plugins, not half installed.
pub fn epic(manifests: &Path) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(manifests) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .filter_map(|e| read_text(&e.path()))
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter_map(|m| epic_app(&m))
        .collect()
}

fn epic_app(m: &Value) -> Option<Installed> {
    let s = |k: &str| m[k].as_str().unwrap_or("").trim();
    let cats: Vec<&str> = m["AppCategories"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let dlc = cats.contains(&"addons") && !cats.contains(&"addons/launchable");
    let plugin = cats.iter().any(|c| c.starts_with("plugins"));
    let part = !s("MainGameAppName").is_empty() && s("MainGameAppName") != s("AppName");
    if dlc || plugin || part || m["bIsIncompleteInstall"].as_bool() == Some(true) {
        return None;
    }
    let (app, ns, item) = (s("AppName"), s("CatalogNamespace"), s("CatalogItemId"));
    let folder = PathBuf::from(s("InstallLocation").replace('/', "\\"));
    if app.is_empty() || ns.is_empty() || item.is_empty() || !folder.is_dir() {
        return None;
    }
    let name = Some(s("DisplayName")).filter(|n| !n.is_empty()).map(String::from).or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()))?;
    let exe = Some(s("LaunchExecutable")).filter(|e| !e.is_empty()).map(|e| folder.join(e.replace('/', "\\")));
    Some(Installed {
        store: Store {
            name: StoreName::Epic,
            id: app.to_string(),
            uri: Some(format!("com.epicgames.launcher://apps/{ns}%3A{item}%3A{app}?action=launch&silent=true")),
            args: None,
        },
        name,
        folder,
        exe,
        steam_app_id: None,
    })
}

// ---------- GOG ----------

/// An entry of Windows' installed programs (the Uninstall keys).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    /// the key's name: `1207658924_is1` for a GOG game, a GUID or a name for others
    pub key: String,
    pub name: String,
    pub publisher: String,
    pub folder: PathBuf,
}

/// The GOG games among the installed programs, from each one's `goggame-<id>.info`: its primary play task.
pub fn gog(programs: &[Program]) -> Vec<Installed> {
    programs
        .iter()
        .filter(|p| p.publisher == "GOG.com")
        .filter_map(|p| {
            let id = p.key.strip_suffix("_is1").filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))?;
            let info: Value = serde_json::from_str(&read_text(&p.folder.join(format!("goggame-{id}.info")))?).ok()?;
            gog_game(id, &info, p)
        })
        .collect()
}

fn gog_game(id: &str, info: &Value, p: &Program) -> Option<Installed> {
    let root = info["rootGameId"].as_str().unwrap_or("");
    if !root.is_empty() && root != id {
        return None; // DLC
    }
    let task = info["playTasks"].as_array()?.iter().find(|t| t["isPrimary"].as_bool() == Some(true) && t["type"] == "FileTask")?;
    let exe = p.folder.join(task["path"].as_str()?.replace('/', "\\"));
    let args = task["arguments"].as_str().map(str::trim).filter(|a| !a.is_empty()).map(String::from);
    let name = info["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&p.name).to_string();
    if name.is_empty() || !p.folder.is_dir() {
        return None;
    }
    Some(Installed {
        store: Store { name: StoreName::Gog, id: id.to_string(), uri: None, args },
        name,
        folder: p.folder.clone(),
        exe: Some(exe),
        steam_app_id: None,
    })
}

// ---------- EA app ----------

/// The EA games among the installed programs: those with `__Installer\installerdata.xml` in their folder.
pub fn ea(programs: &[Program]) -> Vec<Installed> {
    programs
        .iter()
        .filter_map(|p| {
            let xml = read_text(&p.folder.join(r"__Installer\installerdata.xml"))?;
            ea_game(&xml, p)
        })
        .collect()
}

fn ea_game(xml: &str, p: &Program) -> Option<Installed> {
    let offer = xml_text(xml, "contentID")?;
    let title = xml_sections(xml, "gameTitle")
        .into_iter()
        .find(|(open, _)| open.contains("en_US"))
        .or_else(|| xml_sections(xml, "gameTitle").into_iter().next())
        .map(|(_, t)| t);
    let name = Some(p.name.trim().to_string()).filter(|n| !n.is_empty()).or(title)?;
    // a launcher's path starts with the registry value it is relative to: `[HKEY_…\Install Dir]bin\game.exe`
    let exe = xml_sections(xml, "filePath")
        .into_iter()
        .map(|(_, path)| path.rsplit_once(']').map_or(path.clone(), |(_, rel)| rel.to_string()))
        .map(|rel| p.folder.join(rel.trim_start_matches(['\\', '/'])))
        .find(|e| e.is_file());
    if !p.folder.is_dir() {
        return None;
    }
    Some(Installed { store: Store { name: StoreName::Ea, id: offer, uri: None, args: None }, name, folder: p.folder.clone(), exe, steam_app_id: None })
}

// ---------- Xbox ----------

/// A package from HKCU's AppModel repository.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Package {
    /// `Name_Version_Arch_Resource_PublisherId`
    pub full_name: String,
    /// `DisplayName`, often an `@{…ms-resource…}` reference
    pub display_name: String,
    pub folder: PathBuf,
}

/// The packages that are GDK games: their folder has a `MicrosoftGame.config`.
pub fn xbox(packages: &[Package]) -> Vec<Installed> {
    packages
        .iter()
        .filter_map(|p| {
            let config = read_text(&p.folder.join("MicrosoftGame.config"))?;
            let manifest = read_text(&p.folder.join("AppxManifest.xml")).unwrap_or_default();
            xbox_game(p, &config, &manifest)
        })
        .collect()
}

fn xbox_game(p: &Package, config: &str, manifest: &str) -> Option<Installed> {
    let parts: Vec<&str> = p.full_name.split('_').collect();
    let (first, publisher) = (parts.first()?, parts.last()?);
    if parts.len() < 5 || first.is_empty() || publisher.is_empty() {
        return None;
    }
    let family = format!("{first}_{publisher}");
    let app = xml_attr(manifest, "Application", "Id").or_else(|| xml_attr(config, "Executable", "Id")).unwrap_or_else(|| "Game".into());
    let readable = |n: &String| !n.trim().is_empty() && !n.starts_with("ms-resource:") && !n.starts_with("@{");
    let name = xml_attr(config, "ShellVisuals", "DefaultDisplayName")
        .filter(readable)
        .or_else(|| Some(p.display_name.clone()).filter(readable))
        .or_else(|| xml_attr(config, "Identity", "Name"))
        .unwrap_or_else(|| first.to_string());
    let exe = xml_attr(config, "Executable", "Name").map(|e| p.folder.join(e.replace('/', "\\")));
    Some(Installed {
        store: Store { name: StoreName::Xbox, id: family.clone(), uri: Some(format!("shell:AppsFolder\\{family}!{app}")), args: None },
        name: name.trim().to_string(),
        folder: p.folder.clone(),
        exe,
        steam_app_id: None,
    })
}

// ---------- XML, just enough for these files ----------

/// The value of `attr` on the first `<tag …>`.
fn xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(i) = rest.find(&format!("<{tag}")) {
        rest = &rest[i + tag.len() + 1..];
        // `<Executable` must not match `<ExecutableList`
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            continue;
        }
        let open = &rest[..rest.find('>').unwrap_or(rest.len())];
        let mut a = open;
        while let Some(j) = a.find(attr) {
            let before = a[..j].chars().last();
            let after = a[j + attr.len()..].trim_start();
            if before.is_some_and(char::is_whitespace) && after.starts_with('=') {
                let v = after[1..].trim_start();
                let q = v.chars().next()?;
                if q == '"' || q == '\'' {
                    let end = v[1..].find(q)?;
                    return Some(xml_unescape(&v[1..1 + end]));
                }
            }
            a = &a[j + attr.len()..];
        }
        return None;
    }
    None
}

/// Every `<tag …>text</tag>`: (the opening tag's attributes, the text).
fn xml_sections(xml: &str, tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    let close = format!("</{tag}>");
    while let Some(i) = rest.find(&format!("<{tag}")) {
        rest = &rest[i + tag.len() + 1..];
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '>') {
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let attrs = rest[..gt].to_string();
        rest = &rest[gt + 1..];
        let Some(end) = rest.find(&close) else { break };
        out.push((attrs, xml_unescape(rest[..end].trim())));
        rest = &rest[end + close.len()..];
    }
    out
}

fn xml_text(xml: &str, tag: &str) -> Option<String> {
    xml_sections(xml, tag).into_iter().map(|(_, t)| t).find(|t| !t.is_empty())
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// A text file: UTF-8 (BOM or not) or UTF-16 with a BOM.
fn read_text(path: &Path) -> Option<String> {
    let b = std::fs::read(path).ok()?;
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b);
    Some(String::from_utf8_lossy(b).into_owned())
}

// ---------- the registry ----------

#[derive(Clone, Copy)]
enum Hive {
    User,
    Machine,
}

mod reg {
    use super::Hive;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };

    fn root(h: Hive) -> HKEY {
        match h {
            Hive::User => HKEY_CURRENT_USER,
            Hive::Machine => HKEY_LOCAL_MACHINE,
        }
    }

    /// A string value (REG_SZ or REG_EXPAND_SZ, expanded).
    pub fn string(h: Hive, path: &str, name: &str) -> Option<String> {
        let (path, name) = (HSTRING::from(path), HSTRING::from(name));
        let mut size = 0u32;
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        if unsafe { RegGetValueW(root(h), &path, &name, flags, None, None, Some(&mut size)) } != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        size = (buf.len() * 2) as u32;
        if unsafe { RegGetValueW(root(h), &path, &name, flags, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) } != ERROR_SUCCESS {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    /// The names of a key's subkeys.
    pub fn subkeys(h: Hive, path: &str) -> Vec<String> {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(root(h), &HSTRING::from(path), None, KEY_READ, &mut key) } != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            if unsafe { RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) } != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = unsafe { RegCloseKey(key) };
        out
    }
}

fn reg_string(h: Hive, path: &str, name: &str) -> Option<String> {
    reg::string(h, path, name).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Windows' installed programs with a folder, 64- and 32-bit, machine and user.
fn uninstall_entries() -> Vec<Program> {
    const KEYS: [(Hive, &str); 3] = [
        (Hive::Machine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::Machine, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::User, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];
    let mut out = Vec::new();
    for (hive, base) in KEYS {
        for key in reg::subkeys(hive, base) {
            let path = format!(r"{base}\{key}");
            let Some(folder) = reg_string(hive, &path, "InstallLocation") else { continue };
            out.push(Program {
                name: reg_string(hive, &path, "DisplayName").unwrap_or_default(),
                publisher: reg_string(hive, &path, "Publisher").unwrap_or_default(),
                folder: PathBuf::from(folder.trim_matches('"').trim_end_matches('\\')),
                key,
            });
        }
    }
    out
}

/// This user's packages, with their folders.
fn packages() -> Vec<Package> {
    const BASE: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    reg::subkeys(Hive::User, BASE)
        .into_iter()
        .filter_map(|full_name| {
            let path = format!(r"{BASE}\{full_name}");
            let folder = reg_string(Hive::User, &path, "PackageRootFolder")?;
            Some(Package { display_name: reg_string(Hive::User, &path, "DisplayName").unwrap_or_default(), folder: PathBuf::from(folder), full_name })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!("ch-stores-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn file(&self, rel: &str, text: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn acf(id: u64, name: &str, dir: &str, flags: u32) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"Universe\"\t\t\"1\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"installdir\"\t\t\"{dir}\"\n\t\"UserConfig\"\n\t{{\n\t\t\"language\"\t\t\"english\"\n\t}}\n}}\n"
        )
    }

    #[test]
    fn vdf_reads_nested_keys_escapes_and_comments() {
        let v = vdf("// made by Steam\n\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"228980\"\t\t\"0\"\n\t\t}\n\t}\n\t\"1\"\t\t\"D:\\\\Old \\\"Library\\\"\"\n\t\"2\" { \"label\" \"}\" }\n}\n");
        let lf = v.get("LibraryFolders").unwrap();
        assert_eq!(lf.get("0").unwrap().get("path").unwrap().as_str(), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(lf.get("0").unwrap().get("apps").unwrap().get("228980").unwrap().as_str(), Some("0"));
        assert_eq!(lf.get("1").unwrap().as_str(), Some(r#"D:\Old "Library""#));
        assert_eq!(lf.get("2").unwrap().get("label").unwrap().as_str(), Some("}"), "a quoted brace is text");
        assert_eq!(vdf(""), Vdf::Map(Vec::new()));
    }

    #[test]
    fn steam_finds_installed_games_in_every_library() {
        let t = Tmp::new("steam");
        let root = t.dir("Steam");
        let other = t.dir("SteamLibrary");
        t.file(
            "Steam/steamapps/libraryfolders.vdf",
            &format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"2\"\n\t{{\n\t\t\"path\"\t\t\"Z:\\\\Gone\"\n\t}}\n}}\n",
                root.display().to_string().replace('\\', "\\\\"),
                other.display().to_string().replace('\\', "\\\\")
            ),
        );
        t.file("Steam/steamapps/appmanifest_1145360.acf", &acf(1145360, "Hades", "Hades", 4));
        t.dir("Steam/steamapps/common/Hades");
        t.file("Steam/steamapps/appmanifest_228980.acf", &acf(228980, "Steamworks Common Redistributables", "Steamworks Shared", 4));
        t.dir("Steam/steamapps/common/Steamworks Shared");
        t.file("SteamLibrary/steamapps/appmanifest_504230.acf", &acf(504230, "Celeste", "Celeste", 4));
        t.dir("SteamLibrary/steamapps/common/Celeste");
        // downloading (StateFlags 1026: update required + downloading), and installed with no folder
        t.file("SteamLibrary/steamapps/appmanifest_1245620.acf", &acf(1245620, "ELDEN RING", "ELDEN RING", 1026));
        t.dir("SteamLibrary/steamapps/common/ELDEN RING");
        t.file("SteamLibrary/steamapps/appmanifest_367520.acf", &acf(367520, "Hollow Knight", "Hollow Knight", 4));
        let mut found = steam(&root);
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<_> = found.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Celeste", "Hades"]);
        assert_eq!(found[1].folder, root.join(r"steamapps\common\Hades"));
        assert_eq!(found[1].store.uri.as_deref(), Some("steam://rungameid/1145360"));
        assert_eq!((found[1].store.id.as_str(), found[1].steam_app_id), ("1145360", Some(1145360)));
        assert_eq!(found[0].folder, other.join(r"steamapps\common\Celeste"));
    }

    #[test]
    fn epic_skips_dlc_plugins_and_half_installs() {
        let t = Tmp::new("epic");
        let game = t.dir("Epic/Hades");
        let m = |name: &str, extra: Value| {
            let mut v = json!({
                "DisplayName": name, "InstallLocation": game.display().to_string(), "LaunchExecutable": "x64/Hades.exe",
                "AppName": "Min", "CatalogNamespace": "min", "CatalogItemId": "abc123", "AppCategories": ["public", "games", "applications"],
                "bIsIncompleteInstall": false, "MainGameAppName": "Min",
            });
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            v.to_string()
        };
        t.file("Manifests/A.item", &m("Hades", json!({})));
        t.file("Manifests/B.item", &m("Hades DLC", json!({"AppName": "Dlc", "AppCategories": ["addons"]})));
        t.file("Manifests/C.item", &m("Plugin", json!({"AppName": "P", "AppCategories": ["plugins", "plugins/engine"]})));
        t.file("Manifests/D.item", &m("Half", json!({"AppName": "H", "bIsIncompleteInstall": true})));
        t.file("Manifests/E.item", &m("Part", json!({"AppName": "Part", "MainGameAppName": "Min"})));
        t.file("Manifests/F.item", &m("Gone", json!({"AppName": "G", "InstallLocation": "Z:\\Gone"})));
        t.file("Manifests/G.item", "{ not json");
        t.file("Manifests/note.txt", "x");
        let found = epic(&t.0.join("Manifests"));
        assert_eq!(found.len(), 1, "{found:?}");
        let g = &found[0];
        assert_eq!((g.name.as_str(), g.store.id.as_str()), ("Hades", "Min"));
        assert_eq!(g.store.uri.as_deref(), Some("com.epicgames.launcher://apps/min%3Aabc123%3AMin?action=launch&silent=true"));
        assert_eq!(g.exe.as_deref(), Some(game.join(r"x64\Hades.exe").as_path()));
        assert!(epic(&t.0.join("NoSuchDir")).is_empty());
    }

    #[test]
    fn gog_uses_the_primary_play_task_and_skips_dlc() {
        let t = Tmp::new("gog");
        let folder = t.dir("GOG Games/Witcher 3");
        t.file(
            "GOG Games/Witcher 3/goggame-1207664663.info",
            &json!({"gameId": "1207664663", "rootGameId": "1207664663", "name": "The Witcher 3: Wild Hunt", "playTasks": [
                {"isPrimary": false, "type": "FileTask", "path": "bin/x64/config.exe"},
                {"isPrimary": true, "type": "FileTask", "path": "bin/x64/witcher3.exe", "arguments": " -skipintro "}
            ]})
            .to_string(),
        );
        let dlc = t.dir("GOG Games/Witcher 3 DLC");
        t.file("GOG Games/Witcher 3 DLC/goggame-1.info", &json!({"gameId": "1", "rootGameId": "1207664663", "playTasks": []}).to_string());
        let programs = [
            Program { key: "1207664663_is1".into(), name: "The Witcher 3".into(), publisher: "GOG.com".into(), folder: folder.clone() },
            Program { key: "1_is1".into(), name: "DLC".into(), publisher: "GOG.com".into(), folder: dlc },
            Program { key: "{GUID}".into(), name: "Other".into(), publisher: "GOG.com".into(), folder: folder.clone() },
            Program { key: "1207664663_is1".into(), name: "Not GOG".into(), publisher: "Someone".into(), folder: folder.clone() },
        ];
        let found = gog(&programs);
        assert_eq!(found.len(), 1, "{found:?}");
        let g = &found[0];
        assert_eq!((g.name.as_str(), g.store.id.as_str(), g.store.uri.as_deref()), ("The Witcher 3: Wild Hunt", "1207664663", None));
        assert_eq!(g.exe.as_deref(), Some(folder.join(r"bin\x64\witcher3.exe").as_path()));
        assert_eq!(g.store.args.as_deref(), Some("-skipintro"));
    }

    #[test]
    fn ea_reads_installerdata() {
        let t = Tmp::new("ea");
        let folder = t.dir("EA Games/Battlefield 2042");
        t.file("EA Games/Battlefield 2042/BF2042.exe", "x");
        t.file(
            "EA Games/Battlefield 2042/__Installer/installerdata.xml",
            r#"<?xml version="1.0" encoding="utf-8"?>
<DiPManifest version="4.0">
  <contentIDs><contentID>196216</contentID><contentID>Origin.OFR.50.0004323</contentID></contentIDs>
  <gameTitles><gameTitle locale="de_DE">Battlefield™ 2042 DE</gameTitle><gameTitle locale="en_US">Battlefield™ 2042</gameTitle></gameTitles>
  <runtime>
    <launcher uid="1"><filePath>[HKEY_LOCAL_MACHINE\SOFTWARE\EA Games\Battlefield 2042\Install Dir]BF2042_Trial.exe</filePath></launcher>
    <launcher uid="2"><filePath>[HKEY_LOCAL_MACHINE\SOFTWARE\EA Games\Battlefield 2042\Install Dir]BF2042.exe</filePath></launcher>
  </runtime>
</DiPManifest>"#,
        );
        let plain = t.dir("Other");
        let programs = [
            Program { key: "{A}".into(), name: String::new(), publisher: "Electronic Arts".into(), folder: folder.clone() },
            Program { key: "{B}".into(), name: "Other".into(), publisher: "Electronic Arts".into(), folder: plain },
        ];
        let found = ea(&programs);
        assert_eq!(found.len(), 1, "{found:?}");
        let g = &found[0];
        assert_eq!((g.name.as_str(), g.store.id.as_str(), g.store.uri.as_deref()), ("Battlefield™ 2042", "196216", None));
        assert_eq!(g.exe.as_deref(), Some(folder.join("BF2042.exe").as_path()), "the first launcher that is there");
    }

    #[test]
    fn xbox_takes_gdk_packages_and_opens_them_from_apps_folder() {
        let t = Tmp::new("xbox");
        let folder = t.dir("XboxGames/Hades/Content");
        t.file(
            "XboxGames/Hades/Content/MicrosoftGame.config",
            r#"<?xml version="1.0" encoding="utf-8"?>
<Game configVersion="1">
  <Identity Name="SupergiantGames.Hades" Publisher="CN=Supergiant" Version="1.0.0.0"/>
  <ExecutableList>
    <Executable Name="x64\Hades.exe" TargetDeviceFamily="PC" Id="Game"/>
  </ExecutableList>
  <ShellVisuals DefaultDisplayName="Hades &amp; Friends" PublisherDisplayName="Supergiant Games"/>
</Game>"#,
        );
        t.file("XboxGames/Hades/Content/AppxManifest.xml", r#"<Package><Applications><Application Id="HadesApp" Executable="x64\Hades.exe"/></Applications></Package>"#);
        let app = t.dir("WindowsApps/Calculator");
        let pkgs = [
            Package { full_name: "SupergiantGames.Hades_1.0.0.0_x64__abcd1234efgh5".into(), display_name: "@{x?ms-resource://y}".into(), folder: folder.clone() },
            Package { full_name: "Microsoft.WindowsCalculator_11.0_x64__8wekyb3d8bbwe".into(), display_name: "Calculator".into(), folder: app },
        ];
        let found = xbox(&pkgs);
        assert_eq!(found.len(), 1, "only the GDK game: {found:?}");
        let g = &found[0];
        assert_eq!((g.name.as_str(), g.store.id.as_str()), ("Hades & Friends", "SupergiantGames.Hades_abcd1234efgh5"));
        assert_eq!(g.store.uri.as_deref(), Some(r"shell:AppsFolder\SupergiantGames.Hades_abcd1234efgh5!HadesApp"));
        assert_eq!(g.exe.as_deref(), Some(folder.join(r"x64\Hades.exe").as_path()));
        // a resource name and no manifest: the package's display name, and the config's executable id
        let config = r#"<Game><Identity Name="Studio.Game"/><ExecutableList><Executable Name="g.exe" Id="Main"/></ExecutableList><ShellVisuals DefaultDisplayName="ms-resource:Name"/></Game>"#;
        let p = Package { full_name: "Studio.Game_2.0_x64__pub".into(), display_name: "Game Name".into(), folder: folder.clone() };
        let g = xbox_game(&p, config, "").unwrap();
        assert_eq!((g.name.as_str(), g.store.uri.as_deref()), ("Game Name", Some(r"shell:AppsFolder\Studio.Game_pub!Main")));
        assert!(xbox_game(&Package { full_name: "bad".into(), ..p }, config, "").is_none());
    }

    #[test]
    fn one_entry_per_folder_sorted_by_name() {
        let g = |store: StoreName, name: &str, folder: &str| Installed {
            store: Store { name: store, id: name.into(), uri: None, args: None },
            name: name.into(),
            folder: folder.into(),
            exe: None,
            steam_app_id: None,
        };
        let all = tidy(vec![g(StoreName::Steam, "b", r"C:\G\B"), g(StoreName::Epic, "a", r"C:\G\A"), g(StoreName::Gog, "b2", r"c:\g\b\")]);
        let names: Vec<_> = all.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    /// Reads only: what this PC has installed.
    #[test]
    fn reads_this_pc() {
        for g in installed(&[]) {
            println!("{:?} {} -> {} ({:?})", g.store.name, g.name, g.folder.display(), g.store.uri);
        }
    }
}
