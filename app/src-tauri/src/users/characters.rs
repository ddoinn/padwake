//! Character avatars: popular characters (FEATURED, Berserk's among them) and a search, both from AniList's public
//! GraphQL API (free, no key; anime, manga, and the games they were made from). Nothing is bundled: a character's
//! picture is downloaded once, as its top square at 256 px, to `data\avatars\anilist-<id>.jpg`, shared by every user.
//! The page shows only those local files (its CSP allows no remote images). A user's avatar is then `char:<id>`.

use crate::library::art;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const API: &str = "https://graphql.anilist.co";
const SIZE: u32 = 256;
pub const SEARCH_MAX: usize = 24;

/// AniList character ids: popular game characters first, then Berserk.
pub const FEATURED: &[u64] = &[
    456,    // Cloud Strife
    758,    // Tifa Lockhart
    757,    // Aerith Gainsborough
    1816,   // Sephiroth
    132494, // 2B
    121590, // Joker (Ren Amamiya)
    2880,   // Dante
    12359,  // Vergil
    20842,  // Master Chief
    4201,   // Link
    3891,   // Pikachu
    2882,   // Sonic
    5165,   // Chun-Li
    284158, // David Martinez
    422,    // Guts
    424,    // Griffith
    423,    // Casca
    600,    // Zodd
    7881,   // The Skull Knight
    5060,   // Puck
];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: u64,
    pub name: String,
    /// the work it is best known from
    pub from: Option<String>,
    /// the local picture
    pub image: PathBuf,
}

/// The picture of character `id`, if it was downloaded.
pub fn picture(id: u64) -> PathBuf {
    let d = crate::data::dir().join("avatars");
    let _ = std::fs::create_dir_all(&d);
    let p = d.join(format!("anilist-{id}.jpg"));
    std::path::absolute(&p).unwrap_or(p)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("Padwake/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

const FIELDS: &str = "id name { full } image { large } media(perPage: 1, sort: POPULARITY_DESC) { nodes { title { english romaji } } }";

fn ask(agent: &ureq::Agent, query: &str, variables: Value) -> Result<Vec<Value>, String> {
    let mut r = agent
        .post(API)
        .send_json(json!({ "query": query, "variables": variables }))
        .map_err(|e| format!("AniList: {e}"))?;
    let v: Value = r.body_mut().read_json().map_err(|e| format!("AniList: {e}"))?;
    Ok(v["data"]["Page"]["characters"].as_array().cloned().unwrap_or_default())
}

/// What a character entry says: id, name, its work, and where its picture is.
fn parse(c: &Value) -> Option<(u64, String, Option<String>, String)> {
    let id = c["id"].as_u64()?;
    let name = c["name"]["full"].as_str()?.trim().to_string();
    let url = c["image"]["large"].as_str()?.to_string();
    let title = &c["media"]["nodes"][0]["title"];
    let from = title["english"].as_str().or_else(|| title["romaji"].as_str()).map(str::to_string);
    (!name.is_empty()).then_some((id, name, from, url))
}

/// AniList pictures only, so the page can't be made to fetch anything else.
fn allowed(url: &str) -> bool {
    url.strip_prefix("https://").and_then(|r| r.split('/').next()).is_some_and(|host| host == "anilist.co" || host.ends_with(".anilist.co"))
}

/// Downloads a picture as its top square at 256 px (a portrait's face is at its top). Kept: never fetched twice.
fn fetch(agent: &ureq::Agent, id: u64, url: &str) -> Result<PathBuf, String> {
    let path = picture(id);
    if path.is_file() {
        return Ok(path);
    }
    if !allowed(url) {
        return Err(format!("not an AniList picture: {url}"));
    }
    let mut r = agent.get(url).call().map_err(|e| format!("picture {id}: {e}"))?;
    let bytes = r.body_mut().with_config().limit(8 << 20).read_to_vec().map_err(|e| format!("picture {id}: {e}"))?;
    let img = art::square_top(art::decode(&bytes)?);
    let img = img.resize_to_fill(SIZE, SIZE, image::imageops::FilterType::CatmullRom);
    art::save_jpeg(&img, &path, 88)?;
    Ok(path)
}

/// The characters with their pictures, six downloads at a time; one whose picture fails is left out.
fn with_pictures(agent: &ureq::Agent, found: Vec<(u64, String, Option<String>, String)>) -> Vec<Character> {
    let mut out = Vec::new();
    for chunk in found.chunks(6) {
        let got: Vec<_> = std::thread::scope(|s| {
            let jobs: Vec<_> = chunk.iter().map(|(id, _, _, url)| s.spawn(move || fetch(agent, *id, url))).collect();
            jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| Err("picture thread".into()))).collect()
        });
        for ((id, name, from, _), image) in chunk.iter().zip(got) {
            match image {
                Ok(image) => out.push(Character { id: *id, name: name.clone(), from: from.clone(), image }),
                Err(e) => crate::system::log(&format!("avatars: {e}")),
            }
        }
    }
    out
}

/// The popular characters, in FEATURED's order. Asked once per run; a failure is asked again next time.
pub fn featured() -> Result<Vec<Character>, String> {
    static CACHE: OnceLock<Mutex<Option<Vec<Character>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some(c) = cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(c);
    }
    let agent = agent();
    let query = format!("query($ids: [Int]) {{ Page(perPage: 50) {{ characters(id_in: $ids) {{ {FIELDS} }} }} }}");
    let mut found: Vec<_> = ask(&agent, &query, json!({ "ids": FEATURED }))?.iter().filter_map(parse).collect();
    found.sort_by_key(|(id, ..)| FEATURED.iter().position(|f| f == id));
    let list = with_pictures(&agent, found);
    if list.is_empty() {
        return Err("AniList sent no pictures.".into());
    }
    *cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
    Ok(list)
}

/// Characters whose name matches, most loved first.
pub fn search(text: &str) -> Result<Vec<Character>, String> {
    let text = text.trim();
    if text.chars().count() < 2 {
        return Err("Type at least two letters.".into());
    }
    let text: String = text.chars().take(40).collect();
    let agent = agent();
    let query = format!(
        "query($q: String) {{ Page(perPage: {SEARCH_MAX}) {{ characters(search: $q, sort: [SEARCH_MATCH, FAVOURITES_DESC]) {{ {FIELDS} }} }} }}"
    );
    let found = ask(&agent, &query, json!({ "q": text }))?.iter().filter_map(parse).collect();
    Ok(with_pictures(&agent, found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_anilist_pictures_are_fetched() {
        assert!(allowed("https://s4.anilist.co/file/anilistcdn/character/large/b422-XTaiTuvRohsV.png"));
        assert!(!allowed("http://s4.anilist.co/x.png"), "https only");
        assert!(!allowed("https://anilist.co.evil.example/x.png"));
        assert!(!allowed("https://evil.example/anilist.co/x.png"));
    }

    #[test]
    fn a_character_entry_reads_its_name_work_and_picture() {
        let c = json!({"id": 422, "name": {"full": "Guts"}, "image": {"large": "https://s4.anilist.co/g.png"},
            "media": {"nodes": [{"title": {"english": null, "romaji": "Kenpuu Denki Berserk"}}]}});
        assert_eq!(parse(&c), Some((422, "Guts".into(), Some("Kenpuu Denki Berserk".into()), "https://s4.anilist.co/g.png".into())));
        assert_eq!(parse(&json!({"id": 1, "name": {"full": " "}, "image": {"large": "x"}})), None, "no name, no entry");
        assert_eq!(parse(&json!({"id": 1, "name": {"full": "A"}})), None, "no picture, no entry");
    }

    #[test]
    fn berserk_is_featured() {
        for guts_griffith_casca in [422, 424, 423] {
            assert!(FEATURED.contains(&guts_griffith_casca));
        }
        let mut ids = FEATURED.to_vec();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), FEATURED.len(), "no repeats");
    }

    #[test]
    #[ignore = "network: AniList"]
    fn featured_pictures_download() {
        let list = featured().unwrap();
        assert!(list.len() >= FEATURED.len() - 2, "{} of {}", list.len(), FEATURED.len());
        assert!(list.iter().any(|c| c.name == "Guts") && list.iter().all(|c| c.image.is_file()));
        let found = search("griffith").unwrap();
        assert_eq!(found.first().map(|c| c.id), Some(424));
    }
}
