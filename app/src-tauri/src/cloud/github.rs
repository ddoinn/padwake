//! GitHub over HTTPS: the latest release (updates, no sign-in), the device flow (sign-in on a TV: a code to type on a
//! phone), and one private gist (the backup). `PADWAKE_GITHUB_API` and `PADWAKE_GITHUB_WEB` point it at a
//! fake server for tests.

use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

pub const GIST_DESCRIPTION: &str = "Padwake backup";
pub const GIST_FILE: &str = "padwake-backup.json";
pub const SIGNED_OUT: &str = "signed out";

fn api() -> String {
    std::env::var("PADWAKE_GITHUB_API").unwrap_or_else(|_| "https://api.github.com".into())
}

fn web() -> String {
    std::env::var("PADWAKE_GITHUB_WEB").unwrap_or_else(|_| "https://github.com".into())
}

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Http {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(concat!("Padwake/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .build()
            .into();
        Http { agent }
    }

    /// A JSON request; returns the status and the body (Null if it isn't JSON).
    fn json(&self, method: &str, url: &str, token: Option<&str>, body: Option<Value>) -> Result<(u16, Value), String> {
        let short = url.split('?').next().unwrap_or(url);
        let auth = token.map(|t| format!("Bearer {t}"));
        macro_rules! headers {
            ($r:expr) => {{
                let r = $r.header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28");
                match &auth {
                    Some(a) => r.header("Authorization", a),
                    None => r,
                }
            }};
        }
        let sent = match (method, body) {
            ("GET", _) => headers!(self.agent.get(url)).call(),
            ("POST", Some(b)) => headers!(self.agent.post(url)).send_json(b),
            ("PATCH", Some(b)) => headers!(self.agent.patch(url)).send_json(b),
            _ => return Err(format!("{method} {short}: no body")),
        };
        let mut r = sent.map_err(|e| format!("{short}: {e}"))?;
        let status = r.status().as_u16();
        let v = r.body_mut().with_config().limit(16 << 20).read_json::<Value>().unwrap_or(Value::Null);
        Ok((status, v))
    }

    // ---------- releases ----------

    /// The newest release of `repo` ("owner/name"). Anonymous: the repository must be public.
    pub fn latest_release(&self, repo: &str) -> Result<Release, String> {
        let (status, v) = self.json("GET", &format!("{}/repos/{repo}/releases/latest", api()), None, None)?;
        match status {
            200 => serde_json::from_value(v).map_err(|e| format!("release: {e}")),
            404 => Err(format!("{repo} has no public release")),
            403 | 429 => Err("GitHub is busy; it will be checked again later".into()),
            s => Err(format!("GitHub answered {s}")),
        }
    }

    /// Downloads a file of at most `limit` bytes, calling `progress(done, total)` as it goes.
    pub fn download(&self, url: &str, limit: u64, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
        let mut r = self
            .agent
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|e| format!("download: {e}"))?;
        if r.status() != 200 {
            return Err(format!("download: GitHub answered {}", r.status()));
        }
        let total = r.body().content_length().unwrap_or(0);
        if total > limit {
            return Err(format!("download: {total} bytes is too big"));
        }
        let mut reader = r.body_mut().with_config().limit(limit).reader();
        let mut out = Vec::with_capacity(total as usize);
        let mut buf = vec![0u8; 256 << 10];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            progress(out.len() as u64, total);
        }
        Ok(out)
    }

    // ---------- sign-in: the device flow ----------

    pub fn device_code(&self, client_id: &str) -> Result<DeviceCode, String> {
        let url = format!("{}/login/device/code", web());
        let (status, v) = self.json("POST", &url, None, Some(json!({ "client_id": client_id, "scope": "gist" })))?;
        if status != 200 || v.get("device_code").is_none() {
            return Err(error_text(&v).unwrap_or_else(|| format!("GitHub answered {status}")));
        }
        serde_json::from_value(v).map_err(|e| format!("device code: {e}"))
    }

    pub fn poll_token(&self, client_id: &str, device_code: &str) -> Poll {
        let url = format!("{}/login/oauth/access_token", web());
        let body = json!({
            "client_id": client_id,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        });
        let v = match self.json("POST", &url, None, Some(body)) {
            Ok((_, v)) => v,
            Err(e) => return Poll::Failed(e),
        };
        if let Some(t) = v["access_token"].as_str() {
            return Poll::Token(t.to_string());
        }
        match v["error"].as_str() {
            Some("authorization_pending") => Poll::Pending,
            Some("slow_down") => Poll::SlowDown,
            Some("expired_token") => Poll::Failed("The code expired. Sign in again for a new one.".into()),
            Some("access_denied") => Poll::Failed("Sign-in was cancelled on GitHub.".into()),
            _ => Poll::Failed(error_text(&v).unwrap_or_else(|| "GitHub didn't sign you in".into())),
        }
    }

    /// The signed-in account's login. `Err(SIGNED_OUT)` when the token no longer works.
    pub fn login(&self, token: &str) -> Result<String, String> {
        let (status, v) = self.json("GET", &format!("{}/user", api()), Some(token), None)?;
        match status {
            200 => v["login"].as_str().map(str::to_string).ok_or_else(|| "no login".into()),
            401 => Err(SIGNED_OUT.into()),
            s => Err(format!("GitHub answered {s}")),
        }
    }

    // ---------- the backup gist ----------

    /// The backup gist's id, found by its description and file among the account's gists.
    pub fn find_gist(&self, token: &str) -> Result<Option<String>, String> {
        for page in 1..=10 {
            let url = format!("{}/gists?per_page=100&page={page}", api());
            let (status, v) = self.json("GET", &url, Some(token), None)?;
            if status == 401 {
                return Err(SIGNED_OUT.into());
            }
            if status != 200 {
                return Err(format!("gists: GitHub answered {status}"));
            }
            let list = v.as_array().cloned().unwrap_or_default();
            let hit = list.iter().find(|g| g["description"] == GIST_DESCRIPTION && g["files"].get(GIST_FILE).is_some());
            if let Some(g) = hit {
                return Ok(g["id"].as_str().map(str::to_string));
            }
            if list.len() < 100 {
                break;
            }
        }
        Ok(None)
    }

    /// The backup file's text, or `None` if the gist is gone.
    pub fn read_gist(&self, token: &str, id: &str) -> Result<Option<String>, String> {
        let (status, v) = self.json("GET", &format!("{}/gists/{id}", api()), Some(token), None)?;
        match status {
            200 => {}
            404 => return Ok(None),
            401 => return Err(SIGNED_OUT.into()),
            s => return Err(format!("gist: GitHub answered {s}")),
        }
        let file = &v["files"][GIST_FILE];
        if file["truncated"] == true {
            // GitHub sends at most 1 MB inline; the rest comes from the raw URL
            let raw = file["raw_url"].as_str().ok_or("gist: no raw_url")?;
            let mut r = self.agent.get(raw).call().map_err(|e| format!("gist: {e}"))?;
            let text = r.body_mut().with_config().limit(16 << 20).read_to_string();
            return text.map(Some).map_err(|e| format!("gist: {e}"));
        }
        Ok(file["content"].as_str().map(str::to_string))
    }

    /// Creates the private backup gist; returns its id.
    pub fn create_gist(&self, token: &str, content: &str) -> Result<String, String> {
        let body = json!({ "description": GIST_DESCRIPTION, "public": false, "files": { GIST_FILE: { "content": content } } });
        let (status, v) = self.json("POST", &format!("{}/gists", api()), Some(token), Some(body))?;
        match status {
            201 => v["id"].as_str().map(str::to_string).ok_or_else(|| "gist: no id".into()),
            401 => Err(SIGNED_OUT.into()),
            s => Err(error_text(&v).unwrap_or_else(|| format!("gist: GitHub answered {s}"))),
        }
    }

    /// Replaces the backup file. `Ok(false)` if the gist is gone.
    pub fn update_gist(&self, token: &str, id: &str, content: &str) -> Result<bool, String> {
        let body = json!({ "files": { GIST_FILE: { "content": content } } });
        let (status, v) = self.json("PATCH", &format!("{}/gists/{id}", api()), Some(token), Some(body))?;
        match status {
            200 => Ok(true),
            404 => Ok(false),
            401 => Err(SIGNED_OUT.into()),
            s => Err(error_text(&v).unwrap_or_else(|| format!("gist: GitHub answered {s}"))),
        }
    }
}

fn error_text(v: &Value) -> Option<String> {
    v["error_description"].as_str().or_else(|| v["message"].as_str()).map(str::to_string)
}

#[derive(Deserialize, Debug, Clone)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(default = "five")]
    pub interval: u64,
}

fn five() -> u64 {
    5
}

pub enum Poll {
    Token(String),
    Pending,
    SlowDown,
    Failed(String),
}

/// "v1.2.3" or "1.2.3" → [1, 2, 3]; anything after a '-' or '+' is ignored.
pub fn version(tag: &str) -> Option<Vec<u64>> {
    let core = tag.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// True if `tag` is a newer version than `current`.
pub fn newer(tag: &str, current: &str) -> bool {
    match (version(tag), version(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.1.0"));
        assert!(!newer("v0.0.9", "0.1.0"));
        assert!(!newer("nightly", "0.1.0"));
        assert_eq!(version("v1.2.3-beta.1"), Some(vec![1, 2, 3]));
    }
}
