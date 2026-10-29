//! First-party artwork stays in the portable data directory, never in the repository.
use super::*;
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use image::{GenericImageView, ImageEncoder};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct CachedIcon {
    source_url: String,
    /// UTC seconds since the Unix epoch.
    fetched_at: u64,
    width: u32,
    height: u32,
    background_color: String,
}

pub(super) fn description(id: &str) -> &'static str {
    match id {
        "youtube" => "Watch videos from your favourite creators.",
        "netflix" => "Watch films, series and new favourites.",
        "spotify" => "Listen to music, podcasts and playlists.",
        "twitch" => "Watch live streams and join the conversation.",
        "plex" => "Watch films, live TV and your personal collection.",
        "crunchyroll" => "Watch anime and find your next favourite.",
        _ => "",
    }
}

// Known first-party sources also cover sites that block HTML discovery. Manifest URLs
// are resolved at refresh time, so their icon filenames can change independently.
fn sources(id: &str) -> (&'static str, &'static [&'static str]) {
    match id {
        "youtube" => ("https://www.youtube.com/manifest.webmanifest", &[
            "https://www.gstatic.com/youtube/img/web/maskable/logo_512x512.png"]),
        "spotify" => ("https://open.spotifycdn.com/cdn/generated/manifest-mobile-web-player.a1ddf51c.json", &[
            "https://www.scdn.co/i/_global/touch-icon-144.png"]),
        "netflix" => ("", &["https://assets.nflxext.com/en_us/layout/ecweb/netflix-app-icon_152.jpg"]),
        "twitch" => ("", &["https://static-cdn.jtvnw.net/ttv-static-metadata/twitch_logo3.jpg"]),
        "plex" => ("https://www.plex.tv/manifest.json", &[
            "https://www.plex.tv/icons/icon-512x512.png",
            "https://app.plex.tv/desktop/static/icon-ipad@2x.png"]),
        "crunchyroll" => ("", &["https://www.crunchyroll.com/build/assets/img/favicons/apple-touch-icon-180x180.png"]),
        _ => ("", &[]),
    }
}

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

static REQUESTS: OnceLock<mpsc::Sender<bool>> = OnceLock::new();
pub(super) fn start<R: Runtime>(app: AppHandle<R>, force: bool) {
    let sender = REQUESTS.get_or_init(|| {
        let (sender, requests) = mpsc::channel::<bool>();
        std::thread::spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(15)))
                .user_agent("Padwake/0.1").build().into();
            for force in &requests {
                let force = requests.try_iter().fold(force, |a,b| a || b);
                if let Err(e) = update(&app, &agent, force) { eprintln!("media icons: {e}"); }
            }
        });
        sender
    });
    if let Err(e) = sender.send(force) { eprintln!("media refresh: {e}"); }
}

fn bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    // Sessions include both games and Media. Only an already in-flight request can finish.
    while crate::games::is_running() { std::thread::sleep(Duration::from_secs(1)); }
    if !url.starts_with("https://") { return Err("icon URL must use HTTPS".into()); }
    agent.get(url).call().map_err(|e| format!("{url}: {e}"))?
        .body_mut().with_config().limit(8 << 20).read_to_vec().map_err(|e| e.to_string())
}

fn manifest_icons(value: &serde_json::Value, base: &str) -> Vec<String> {
    let mut icons = Vec::new();
    if let Some(list) = value["icons"].as_array() {
        for icon in list {
            let purpose = icon["purpose"].as_str().unwrap_or("any");
            if !purpose.split_whitespace().any(|p| p == "any" || p == "maskable") { continue; }
            let size = icon["sizes"].as_str().unwrap_or("").split_whitespace()
                .filter_map(|s| s.split_once('x')).filter_map(|(w,h)| Some(w.parse::<u32>().ok()?.min(h.parse().ok()?)))
                .max().unwrap_or(0);
            let Some(src) = icon["src"].as_str() else { continue };
            if size < 192 || !(icon["type"] == "image/png" || src.ends_with(".png")) { continue; }
            if let Ok(url) = tauri::Url::parse(base).and_then(|b| b.join(src)) {
                if url.scheme() == "https" { icons.push((size, url.to_string())); }
            }
        }
    }
    icons.sort_by(|a,b| b.0.cmp(&a.0));
    icons.into_iter().map(|(_,url)| url).collect()
}

fn update<R: Runtime>(app: &AppHandle<R>, agent: &ureq::Agent, force: bool) -> Result<(), String> {
    let dir = crate::data::dir();
    let mut entries = read_entries(dir)?;
    for index in 0..entries.len() {
        let entry = &mut entries[index];
        if !force && entry.icon.as_ref().is_some_and(|i| now().saturating_sub(i.fetched_at) < 7 * 86400) { continue; }
        let (manifest, fallback) = sources(&entry.id);
        let mut candidates = Vec::new();
        if !manifest.is_empty() {
            match bytes(agent, manifest).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string())) {
                Ok(value) => candidates = manifest_icons(&value, manifest),
                Err(e) => eprintln!("media {} manifest: {e}", entry.id),
            }
        }
        candidates.extend(fallback.iter().map(|s| s.to_string()));
        for url in candidates {
            match bytes(agent, &url).and_then(|b| cache(dir, entry, &url, &b)) {
                Ok(()) => {
                    save_entries(dir, &entries)?;
                    let _ = app.emit("media", &entries);
                    break;
                }
                Err(e) => eprintln!("media icon {url}: {e}"),
            }
        }
    }
    Ok(())
}

fn cache(dir: &Path, entry: &mut Entry, url: &str, bytes: &[u8]) -> Result<(), String> {
    let dir = std::path::absolute(dir).map_err(|e| e.to_string())?;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048); limits.max_image_height = Some(2048);
    let mut reader = reader; reader.limits(limits);
    let icon = reader.decode().map_err(|e| e.to_string())?;
    let (width, height) = icon.dimensions();
    if width.min(height) < 144 { return Err(format!("icon too small: {width}x{height}")); }
    // Median opaque edge pixel: robust to corner transparency and a logo touching an edge.
    let mut edge: [Vec<u8>; 3] = Default::default();
    for (x,y,p) in icon.to_rgba8().enumerate_pixels() {
        if (x == 0 || y == 0 || x == width-1 || y == height-1) && p[3] > 240 {
            for c in 0..3 { edge[c].push(p[c]); }
        }
    }
    let rgb: Vec<u8> = edge.iter_mut().map(|v| { v.sort_unstable(); v.get(v.len()/2).copied().unwrap_or(16) }).collect();
    let color = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    let mut tile = image::RgbaImage::from_pixel(512,512,image::Rgba([rgb[0],rgb[1],rgb[2],255]));
    let fitted = icon.resize(512,512,image::imageops::FilterType::Lanczos3).to_rgba8();
    image::imageops::overlay(&mut tile,&fitted,((512-fitted.width())/2).into(),((512-fitted.height())/2).into());
    let folder = dir.join("media").join(&entry.id);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&tile,512,512,image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    let path = folder.join("tile.png");
    crate::library::write_file(&path, &png)?;
    // A quiet, locally generated brand-colour background, without a second giant wordmark.
    let bg = folder.join("background.svg");
    crate::library::write_file(&bg, format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080"><defs><radialGradient id="g"><stop stop-color="{color}"/><stop offset="1" stop-color="#10151e"/></radialGradient></defs><path fill="url(#g)" d="M0 0h1920v1080H0z"/></svg>"##).as_bytes())?;
    entry.art = Art { tile: path.to_string_lossy().into_owned(), background: bg.to_string_lossy().into_owned() };
    entry.icon = Some(CachedIcon { source_url: url.into(), fetched_at: now(), width, height, background_color: color });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "live first-party downloads; writes artwork under PADWAKE_DATA"]
    fn download_all_service_icons() {
        let dir = crate::data::dir();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15))).build().into();
        let mut entries = defaults(dir).unwrap();
        for entry in &mut entries {
            let (manifest, fallback) = sources(&entry.id);
            let mut urls = if manifest.is_empty() { Vec::new() } else {
                bytes(&agent,manifest).ok().and_then(|b| serde_json::from_slice(&b).ok())
                    .map(|v| manifest_icons(&v,manifest)).unwrap_or_default()
            };
            urls.extend(fallback.iter().map(|s| s.to_string()));
            for url in urls {
                match bytes(&agent,&url).and_then(|b| cache(dir,entry,&url,&b)) {
                    Ok(()) => break,
                    Err(e) => eprintln!("{url}: {e}"),
                }
            }
            let info = entry.icon.as_ref().unwrap_or_else(|| panic!("{} has no icon",entry.id));
            println!("{} {}x{} {} bytes {}",entry.id,info.width,info.height,
                std::fs::metadata(&entry.art.tile).unwrap().len(),info.source_url);
        }
        save_entries(dir,&entries).unwrap();
    }
    #[test]
    fn manifest_prefers_largest_colour_icon_and_resolves_relative_urls() {
        let m = serde_json::json!({"icons":[
            {"src":"mono.png","sizes":"1024x1024","purpose":"monochrome"},
            {"src":"small.png","sizes":"96x96"},
            {"src":"any.png","sizes":"192x192"},
            {"src":"mask.png","sizes":"512x512","purpose":"maskable"}]});
        assert_eq!(manifest_icons(&m,"https://example.com/app/manifest.json"),[
            "https://example.com/app/mask.png","https://example.com/app/any.png"]);
    }
    #[test]
    fn cache_records_source_and_roundtrips_portable_paths() {
        let root = PathBuf::from(r"W:\tmp").join(format!("media-icon-test-{}", std::process::id()));
        let mut entries = defaults(&root).unwrap();
        let img = image::RgbaImage::from_pixel(192,192,image::Rgba([12,34,56,255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(&img,192,192,image::ExtendedColorType::Rgba8).unwrap();
        cache(&root,&mut entries[0],"https://example.com/icon.png",&png).unwrap();
        save_entries(&root,&entries).unwrap();
        let json = std::fs::read_to_string(root.join("media.json")).unwrap();
        assert!(!json.contains("W:"));
        let loaded = read_entries(&root).unwrap();
        assert!(Path::new(&loaded[0].art.tile).is_file());
        let info = loaded[0].icon.as_ref().unwrap();
        assert_eq!(info.background_color,"#0c2238");
        assert_eq!((info.width,info.height),(192,192));
        assert_eq!(info.source_url,"https://example.com/icon.png");
        assert!(cache(&root,&mut entries[0],"https://example.com/bad",b"not an image").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
