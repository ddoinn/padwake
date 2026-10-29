//! Store info and art for one game, ported from `Enrich` and `HubMedia` in `extras/antcloud/sync.ps1`: the Steam store
//! match, description, tagline, screenshots, trailer and music, SteamGridDB art (key in `sgdb.key` in the data dir),
//! and the latest Steam news. Everything lands in the game's folder in the data dir.

use super::{art, trailer, Art, Game, News};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const STORE: &str = "https://store.steampowered.com/api";
const CDN: &str = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps";
const SGDB: &str = "https://www.steamgriddb.com/api/v2";

pub struct Http {
    agent: ureq::Agent,
    sgdb_key: Option<String>,
}

impl Http {
    pub fn new() -> Http {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent("Padwake/0.1")
            .build()
            .into();
        let sgdb_key = std::fs::read_to_string(crate::data::dir().join("sgdb.key"))
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        Http { agent, sgdb_key }
    }

    pub fn has_sgdb_key(&self) -> bool {
        self.sgdb_key.is_some()
    }

    fn call(&self, url: &str, auth: Option<&str>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let mut req = self.agent.get(url);
        if let Some(key) = auth {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        req.call().map_err(|e| format!("{}: {e}", short(url)))
    }

    pub fn json(&self, url: &str) -> Result<Value, String> {
        self.call(url, None)?.body_mut().read_json().map_err(|e| format!("{}: {e}", short(url)))
    }

    pub fn text(&self, url: &str) -> Result<String, String> {
        self.call(url, None)?.body_mut().read_to_string().map_err(|e| format!("{}: {e}", short(url)))
    }

    pub fn bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut r = self.call(url, None)?;
        r.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| format!("{}: {e}", short(url)))
    }

    /// A SteamGridDB list (`data`), or nothing without a key or on failure.
    fn sgdb(&self, path: &str, problems: &mut Vec<String>) -> Vec<Value> {
        let Some(key) = &self.sgdb_key else { return Vec::new() };
        let url = format!("{SGDB}/{path}");
        let got = self.call(&url, Some(key)).and_then(|mut r| r.body_mut().read_json::<Value>().map_err(|e| e.to_string()));
        match got {
            Ok(v) => v["data"].as_array().cloned().unwrap_or_default(),
            Err(e) => {
                problems.push(format!("SteamGridDB {e}"));
                Vec::new()
            }
        }
    }
}

/// A URL without its query, for log lines.
fn short(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------- the Steam match ----------

/// Store search terms for a folder: the whole name, then with trailing words dropped down to two ("Ghost of Tsushima
/// DC" finds "Ghost of Tsushima"), then the game's name. A lone dash isn't a word: Steam finds nothing for
/// "Sekiro - Shadows Die Twice" but does for "Sekiro Shadows Die Twice".
pub fn queries(folder_name: &str, name: &str) -> Vec<String> {
    let words: Vec<&str> = folder_name.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).collect();
    let mut q: Vec<String> = (words.len().min(2)..=words.len()).rev().filter(|&n| n > 0).map(|n| words[..n].join(" ")).collect();
    q.push(name.to_string());
    let mut seen = std::collections::HashSet::new();
    q.retain(|s| !s.trim().is_empty() && seen.insert(s.clone()));
    q
}

/// For comparing names: no ™ or ®, lower case, punctuation runs as one space.
pub fn norm(s: &str) -> String {
    let s: String = s.chars().filter(|c| *c != '™' && *c != '®').flat_map(char::to_lowercase).collect();
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The search hit for a query: the exact name (Steam lists "Hades II" before "Hades"), else the first hit whose name
/// has every word of the query ("Ghost of Tsushima" finds "Ghost of Tsushima DIRECTOR'S CUT"). sync.ps1 took any first
/// hit, but Steam answers "Stand-in" with "Blasterbug: Last Bug Standing!", so a hit must at least name the game.
pub fn pick<'a>(items: &'a [Value], query: &str) -> Option<&'a Value> {
    let q = norm(query);
    let name = |i: &Value| norm(i["name"].as_str().unwrap_or(""));
    items.iter().find(|i| name(i) == q).or_else(|| {
        items.iter().find(|i| {
            let n = name(i);
            let words: Vec<&str> = n.split(' ').collect();
            q.split(' ').all(|w| words.contains(&w))
        })
    })
}

/// The Steam app for a folder; `Ok(None)` when no search term finds one.
fn find_app(http: &Http, tried: &[String], stop: &dyn Fn() -> bool) -> Result<Option<u64>, String> {
    for q in tried {
        pause_check(stop)?;
        let found = http.json(&format!("{STORE}/storesearch/?term={}&l=english&cc=US", encode(q)))?;
        let items = found["items"].as_array().cloned().unwrap_or_default();
        if let Some(id) = pick(&items, q).and_then(|i| i["id"].as_u64()) {
            return Ok(Some(id));
        }
    }
    // a game that started during the last search wins over the miss, so the folder is retried later
    pause_check(stop)?;
    Ok(None)
}

// ---------- text ----------

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "trade" => '™',
        "reg" => '®',
        "copy" => '©',
        _ => return None,
    })
}

/// Plain text from store HTML: tags dropped, entities decoded, spaces collapsed. With `lines`, `<br>` and the ends of
/// paragraphs, headings and list items become line breaks (at most one blank line); without, everything is one line.
pub fn html_text(html: &str, lines: bool) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find(['<', '&']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if rest.starts_with('<') {
            let Some(end) = rest.find('>') else { break };
            let tag = rest[1..end].trim_start_matches('/').to_ascii_lowercase();
            let tag = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            let closing = rest[1..].starts_with('/');
            let heading = matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
            let block = heading || matches!(tag, "p" | "div" | "ul" | "ol");
            if tag == "br" || block || (tag == "li" && !closing) {
                out.push('\n');
            }
            if heading && !closing {
                out.push('\n'); // a blank line before a heading
            }
            rest = &rest[end + 1..];
        } else {
            let decoded = rest[1..].find(';').filter(|&n| n <= 10).and_then(|n| Some((entity(&rest[1..1 + n])?, n)));
            match decoded {
                Some((c, n)) => {
                    out.push(c);
                    rest = &rest[n + 2..];
                }
                None => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
    }
    out.push_str(rest);
    if !lines {
        return out.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let mut text = String::new();
    let mut blank = false;
    for line in out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")) {
        if line.is_empty() {
            blank = !text.is_empty();
            continue;
        }
        if !text.is_empty() {
            text.push_str(if blank { "\n\n" } else { "\n" });
        }
        text.push_str(&line);
        blank = false;
    }
    text
}

/// The hero's tagline: the first sentence of the store's short description, kept only if it's short (≤ 80).
pub fn tagline(short_description: &str) -> Option<String> {
    let s = html_text(short_description, false);
    let chars: Vec<char> = s.chars().collect();
    let end = (0..chars.len().saturating_sub(1))
        .find(|&i| matches!(chars[i], '.' | '!' | '?') && chars[i + 1].is_whitespace())
        .map_or(chars.len(), |i| i + 1);
    let first: String = chars[..end].iter().collect::<String>().trim().to_string();
    (!first.is_empty() && first.chars().count() <= 80).then_some(first)
}

/// A news item's text: no HTML, BBCode (`[p]`, `[url=…]`), clan image tokens or links; about `max` characters.
pub fn news_text(contents: &str, max: usize) -> String {
    let text = html_text(contents, false);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(']').filter(|&e| e <= 300);
        let is_tag = end.is_some_and(|e| {
            let inner = rest[1..e].trim_start_matches('/');
            let name = inner.split('=').next().unwrap_or("");
            name == "*" || (!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
        });
        match (is_tag, end) {
            (true, Some(e)) => {
                out.push(' ');
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('[');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    let words: Vec<&str> =
        out.split_whitespace().filter(|w| !w.contains("{STEAM_CLAN_IMAGE}") && !w.starts_with("http")).collect();
    cap(&words.join(" "), max)
}

fn cap(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ']))
}

// ---------- the pipeline ----------

/// What enrichment found for one game. `problems` are the assets that didn't land, for the log.
#[derive(Default)]
pub struct Found {
    pub steam_app_id: u64,
    pub name: String,
    pub tagline: Option<String>,
    pub description: Option<String>,
    pub art: Art,
    pub screenshots: Vec<PathBuf>,
    pub trailer: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub news: Vec<News>,
    pub problems: Vec<String>,
}

/// What enrichment fails with when a game starts: the game stays not done, so the next pass tries it again from scratch.
pub const PAUSED: &str = "paused: a game is running";

/// `Err(PAUSED)` once `stop` is true (a game started): nothing of ours keeps running beside a game.
pub fn pause_check(stop: &dyn Fn() -> bool) -> Result<(), String> {
    if stop() {
        Err(PAUSED.into())
    } else {
        Ok(())
    }
}

/// Fetches a game's store info, art and media into `dir`. Fails without a Steam match, or with `PAUSED` as soon as
/// `stop` turns true (a game started): it is checked before every download and every image or ffmpeg step.
pub fn enrich(http: &Http, game: &Game, dir: &Path, ffmpeg: Option<&Path>, stop: &dyn Fn() -> bool) -> Result<Found, String> {
    pause_check(stop)?;
    let folder_name = game.folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tried = queries(&folder_name, &game.name);
    let id = find_app(http, &tried, stop)?.ok_or_else(|| format!("no Steam match (tried: {})", tried.join(" | ")))?;
    pause_check(stop)?;
    let details = http.json(&format!("{STORE}/appdetails?appids={id}&l=english&cc=US"))?;
    let d = &details[id.to_string()]["data"];
    if !d.is_object() {
        return Err(format!("Steam details failed for {id}"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut f = Found { steam_app_id: id, ..Default::default() };
    f.name = d["name"].as_str().unwrap_or(&game.name).replace(['™', '®'], "").trim().to_string();
    // some About sections are only pictures (Cyberpunk 2077): then the short description
    f.description = [&d["about_the_game"], &d["short_description"]]
        .into_iter()
        .filter_map(|v| v.as_str())
        .map(|h| html_text(h, true))
        .find(|s| !s.is_empty());
    f.tagline = d["short_description"].as_str().and_then(tagline);
    let cdn = format!("{CDN}/{id}");
    let shots: Vec<String> =
        d["screenshots"].as_array().into_iter().flatten().filter_map(|s| s["path_full"].as_str()).map(|u| short(u).to_string()).collect();
    let problems = &mut f.problems;

    // cover: Steam's portrait library art, else a SteamGridDB 600x900 grid
    pause_check(stop)?;
    let cover = first_image(http, stop, [format!("{cdn}/library_600x900_2x.jpg"), format!("{cdn}/library_600x900.jpg")])
        .or_else(|| sgdb_image(http, stop, &format!("grids/steam/{id}?dimensions=600x900&types=static&nsfw=false&humor=false"), problems));
    pause_check(stop)?;
    if let Some(img) = &cover {
        f.art.cover = save(art::save_jpeg(&art::fit(img.clone(), 600), &dir.join("cover.jpg"), 88), dir.join("cover.jpg"), problems);
    }
    // square tile: SteamGridDB's top-voted 1:1 grid with a logo, else the top square of the cover
    pause_check(stop)?;
    let grid = sgdb_image(
        http,
        stop,
        &format!("grids/steam/{id}?dimensions=1024x1024,512x512&types=static&styles=alternate,white_logo,material&nsfw=false&humor=false"),
        problems,
    );
    pause_check(stop)?;
    if let Some(tile) = grid.or_else(|| cover.map(art::square_top)) {
        f.art.tile = save(art::save_jpeg(&art::fit(tile, 512), &dir.join("tile.jpg"), 90), dir.join("tile.jpg"), problems);
    } else {
        problems.push("tile: none".into());
    }
    // background: logo-free key art that isn't too bright (white text goes on it): Steam's hero, then SteamGridDB
    // heroes, then screenshots; else the darkest of them
    pause_check(stop)?;
    let mut candidates = vec![format!("{cdn}/library_hero_2x.jpg"), format!("{cdn}/library_hero.jpg")];
    let heroes = http.sgdb(&format!("heroes/steam/{id}?types=static&nsfw=false&humor=false"), problems);
    candidates.extend(heroes.iter().take(5).filter_map(|h| h["url"].as_str()).map(String::from));
    candidates.extend(shots.iter().take(4).cloned());
    let mut darkest: Option<(f64, image::DynamicImage)> = None;
    for url in &candidates {
        pause_check(stop)?;
        let Some(img) = http.bytes(url).ok().and_then(|b| art::decode(&b).ok()) else { continue };
        let l = art::luma(&img);
        let done = l < 130.0;
        if done || darkest.as_ref().is_none_or(|(dl, _)| l < *dl) {
            darkest = Some((l, img));
        }
        if done {
            break;
        }
    }
    pause_check(stop)?;
    match darkest {
        Some((_, img)) => {
            let p = dir.join("background.jpg");
            f.art.background = save(art::save_jpeg(&art::fit(art::crop_16_9(img), 1920), &p, 90), p, problems);
        }
        None => problems.push("background: none".into()),
    }
    // logo, dark ones made white for the dark UI
    pause_check(stop)?;
    let logo = first_image(http, stop, [format!("{cdn}/logo_2x.png"), format!("{cdn}/logo.png")])
        .or_else(|| sgdb_image(http, stop, &format!("logos/steam/{id}?types=static&nsfw=false&humor=false"), problems));
    pause_check(stop)?;
    match logo {
        Some(img) => {
            let mut rgba = img.to_rgba8();
            art::whiten_dark_glyph(&mut rgba);
            let p = dir.join("logo.png");
            f.art.logo = save(art::save_png(&rgba, &p), p, problems);
        }
        None => problems.push("logo: none".into()),
    }
    // the hub's 5 screenshots
    for url in shots.iter().take(5) {
        pause_check(stop)?;
        let p = dir.join(format!("shot{}.jpg", f.screenshots.len() + 1));
        match http.bytes(url).and_then(|b| art::decode(&b)).and_then(|img| art::save_jpeg(&art::fit(img, 1280), &p, 88)) {
            Ok(()) => f.screenshots.push(p),
            Err(e) => f.problems.push(format!("screenshot: {e}")),
        }
    }
    for n in f.screenshots.len() + 1..=5 {
        let _ = std::fs::remove_file(dir.join(format!("shot{n}.jpg")));
    }
    // official news
    let news_url = format!(
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={id}&count=5&maxlength=400&feeds=steam_community_announcements&format=json"
    );
    pause_check(stop)?;
    match http.json(&news_url) {
        Ok(n) => {
            f.news = n["appnews"]["newsitems"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| News {
                    title: html_text(i["title"].as_str().unwrap_or(""), false),
                    url: i["url"].as_str().unwrap_or("").to_string(),
                    date: i["date"].as_u64().unwrap_or(0) * 1000,
                    contents: news_text(i["contents"].as_str().unwrap_or(""), 300),
                })
                .collect()
        }
        Err(e) => f.problems.push(format!("news: {e}")),
    }
    // trailer and music: a highlighted movie first
    pause_check(stop)?;
    let movies: Vec<&Value> = d["movies"].as_array().map(|m| m.iter().collect()).unwrap_or_default();
    let movie = movies.iter().filter(|m| m["highlight"].as_bool() == Some(true)).chain(movies.iter()).find_map(|m| m["hls_h264"].as_str());
    match movie {
        Some(master) => {
            let (t, m, errors) = trailer::fetch(http, master, dir, ffmpeg, stop)?;
            f.trailer = t;
            f.music = m;
            f.problems.extend(errors);
        }
        None => f.problems.push("trailer: the store has no movie".into()),
    }
    Ok(f)
}

/// The first of `urls` that downloads and decodes. Tries no more once `stop` is true (the caller then checks it).
fn first_image(http: &Http, stop: &dyn Fn() -> bool, urls: impl IntoIterator<Item = String>) -> Option<image::DynamicImage> {
    urls.into_iter().take_while(|_| !stop()).find_map(|u| http.bytes(&u).ok().and_then(|b| art::decode(&b).ok()))
}

fn sgdb_image(http: &Http, stop: &dyn Fn() -> bool, path: &str, problems: &mut Vec<String>) -> Option<image::DynamicImage> {
    if stop() {
        return None;
    }
    let list = http.sgdb(path, problems);
    first_image(http, stop, list.iter().filter_map(|g| g["url"].as_str()).take(3).map(String::from))
}

fn save(result: Result<(), String>, path: PathBuf, problems: &mut Vec<String>) -> Option<PathBuf> {
    match result {
        Ok(()) => Some(path),
        Err(e) => {
            problems.push(e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_terms_drop_trailing_words_and_lone_dashes() {
        assert_eq!(
            queries("Sekiro - Shadows Die Twice", "Sekiro - Shadows Die Twice"),
            ["Sekiro Shadows Die Twice", "Sekiro Shadows Die", "Sekiro Shadows", "Sekiro - Shadows Die Twice"]
        );
        assert_eq!(queries("Hades", "Hades"), ["Hades"]);
        assert_eq!(queries("!!!", "!!!"), ["!!!"]);
    }

    #[test]
    fn an_exact_name_beats_the_first_hit() {
        let items = [json!({"id": 1145350, "name": "Hades II"}), json!({"id": 1145360, "name": "Hades"})];
        assert_eq!(pick(&items, "Hades").unwrap()["id"], 1145360);
        let items = [json!({"id": 1, "name": "DARK SOULS™: REMASTERED"}), json!({"id": 2, "name": "Dark Souls III"})];
        assert_eq!(pick(&items, "Dark Souls Remastered").unwrap()["id"], 1);
        // no exact name: the first hit that has every word
        let items = [json!({"id": 9, "name": "Ghost Recon"}), json!({"id": 2215430, "name": "Ghost of Tsushima DIRECTOR'S CUT"})];
        assert_eq!(pick(&items, "Ghost of Tsushima").unwrap()["id"], 2215430);
        assert!(pick(&items, "Ghost of Tsushima DC").is_none(), "\"dc\" isn't in the name: the shorter term finds it");
        let items = [json!({"id": 1817070, "name": "Marvel’s Spider-Man Remastered"})];
        assert_eq!(pick(&items, "Spider-man Remastered").unwrap()["id"], 1817070);
        // a hit that doesn't name the game is no match
        let items = [json!({"id": 3069340, "name": "Blasterbug: Last Bug Standing! "}), json!({"id": 1, "name": "ANIME STANDING"})];
        assert!(pick(&items, "Stand-in").is_none());
        assert!(pick(&[], "Hades").is_none());
    }

    #[test]
    fn tagline_is_the_first_short_sentence() {
        assert_eq!(
            tagline("Explore a vast world. Fight &amp; die. Repeat.").as_deref(),
            Some("Explore a vast world.")
        );
        assert_eq!(tagline("<b>Help Madeline survive her inner demons!</b> On her journey up Celeste Mountain.").as_deref(), Some("Help Madeline survive her inner demons!"));
        assert_eq!(tagline("Version 1.5 is out now"), Some("Version 1.5 is out now".into()));
        // one long sentence: no tagline
        assert_eq!(tagline("Defy the god of the dead as you hack and slash out of the Underworld in this rogue-like dungeon crawler from the creators of Bastion, Transistor, and Pyre."), None);
        assert_eq!(tagline(""), None);
    }

    #[test]
    fn html_is_stripped() {
        let about = "<h2 class=\"bb_tag\">About</h2><p>Defy the god &amp; the <i>dead</i>.<br>Second&nbsp;line</p>\
            <ul><li>One</li><li>Two &#8211; &#x2014; &hellip;</li></ul><img src=\"x.gif\" /> Tom &amp Jerry";
        assert_eq!(html_text(about, true), "About\n\nDefy the god & the dead.\nSecond line\n\nOne\nTwo – — …\nTom &amp Jerry");
        assert_eq!(html_text("a <b>b</b>\n\n c", false), "a b c");
        // a heading straight after text (Hades) starts a new paragraph
        assert_eq!(html_text("of <b>Pyre</b>.<h2 class=\"bb_tag\" >BATTLE</h2>As the", true), "of Pyre.\n\nBATTLE\nAs the");
        let news = "[p]We're thrilled[/p] {STEAM_CLAN_IMAGE}/356/1e6a.png [url=https://x.y]Shop[/url] [UPDATE] ok\
            <br>https://store.steampowered.com/app/1";
        assert_eq!(news_text(news, 300), "We're thrilled Shop [UPDATE] ok");
        let long = "word ".repeat(100);
        let capped = news_text(&long, 300);
        assert!(capped.chars().count() <= 301 && capped.ends_with("word…"), "{capped}");
    }
}
