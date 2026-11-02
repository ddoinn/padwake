//! A game's typical main-story length from HowLongToBeat, shown beside the time played in its journal.
//! HowLongToBeat has no public API. Its site asks `/api/search/site/init` for a token (tied to the caller's IP and
//! User-Agent), then POSTs the search with it in `x-auth-token`; a 403 means a stale token. Checked against the live
//! site on 2026-11-02 ("Hollow Knight": `comp_main` 97162 s). It changes every few months: when it breaks, lookups
//! fail (logged), the journal just doesn't show a length, and it is tried again at the next start or session.

use super::steam::norm;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SITE: &str = "https://howlongtobeat.com";
/// A browser's: the site refuses others, and the token only works with the User-Agent that asked for it.
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

/// The main-story length in seconds of the game best matching `name`; `Ok(None)` when nothing matches well enough.
pub fn main_story(name: &str) -> Result<Option<u64>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).user_agent(UA).build().into();
    let mut token = init(&agent)?;
    let mut tries = 0;
    let found = loop {
        match search(&agent, &token, name) {
            Err(ureq::Error::StatusCode(403)) if tries == 0 => {
                tries += 1;
                token = init(&agent)?;
            }
            other => break other.map_err(|e| format!("howlongtobeat search: {e}"))?,
        }
    };
    Ok(best(&found, name))
}

fn init(agent: &ureq::Agent) -> Result<String, String> {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let v: Value = agent
        .get(format!("{SITE}/api/search/site/init?t={t}"))
        .header("Referer", format!("{SITE}/"))
        .call()
        .and_then(|mut r| r.body_mut().read_json())
        .map_err(|e| format!("howlongtobeat init: {e}"))?;
    v["token"].as_str().map(str::to_string).ok_or_else(|| "howlongtobeat init: no token".into())
}

fn search(agent: &ureq::Agent, token: &str, name: &str) -> Result<Value, ureq::Error> {
    let terms: Vec<&str> = name.split_whitespace().collect();
    let body = json!({
        "searchType": "games", "searchTerms": terms, "searchPage": 1, "size": 20,
        "searchOptions": {
            "games": {"userId": 0, "platform": "", "sortCategory": "popular", "rangeCategory": "main",
                "rangeTime": {"min": 0, "max": 0}, "gameplay": {"perspective": "", "flow": "", "genre": ""},
                "year": "", "modifier": "hide_dlc"},
            "users": {"sortCategory": "postcount"}, "lists": {"sortCategory": "follows"},
            "filter": "", "sort": 0, "randomizer": 0
        },
        "useCache": true
    });
    agent
        .post(format!("{SITE}/api/search/site"))
        .header("Accept", "*/*")
        .header("Referer", format!("{SITE}/"))
        .header("Origin", SITE)
        .header("x-auth-token", token)
        .send_json(&body)?
        .body_mut()
        .read_json()
}

/// The result named exactly `name` (ignoring case and punctuation), else one whose name contains it or is contained in
/// it, most popular first; its `comp_main`, else `comp_plus`. Seconds.
fn best(found: &Value, name: &str) -> Option<u64> {
    let want = norm(name);
    if want.is_empty() {
        return None;
    }
    let games: Vec<&Value> = found["data"].as_array()?.iter().filter(|g| g["game_type"].as_str().unwrap_or("game") == "game").collect();
    let named = |g: &&Value| norm(g["game_name"].as_str().unwrap_or(""));
    let hit = games.iter().find(|g| named(g) == want).or_else(|| {
        games.iter().find(|g| {
            let n = named(g);
            !n.is_empty() && (n.contains(&want) || want.contains(&n))
        })
    })?;
    [&hit["comp_main"], &hit["comp_plus"]].into_iter().filter_map(Value::as_u64).find(|&s| s > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exact_name_wins_then_a_close_one() {
        let found = json!({"data": [
            {"game_name": "Hollow Knight: Silksong", "game_type": "game", "comp_main": 150000},
            {"game_name": "Hollow Knight", "game_type": "game", "comp_main": 97162, "comp_plus": 149869},
        ]});
        assert_eq!(best(&found, "Hollow Knight"), Some(97162));
        let only_long = json!({"data": [{"game_name": "Hades II", "game_type": "game", "comp_main": 0, "comp_plus": 90000}]});
        assert_eq!(best(&only_long, "HADES II"), Some(90000), "no main time: main + extras");
        let unrelated = json!({"data": [{"game_name": "Celeste", "game_type": "game", "comp_main": 30000}]});
        assert_eq!(best(&unrelated, "Hollow Knight"), None);
    }

    #[test]
    #[ignore = "network: asks howlongtobeat.com"]
    fn real_lookup() {
        let s = main_story("Hollow Knight").unwrap().unwrap();
        println!("Hollow Knight main story: {:.1} h", s as f64 / 3600.0);
        assert!(s > 20 * 3600 && s < 40 * 3600);
    }
}
