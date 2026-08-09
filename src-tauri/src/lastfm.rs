// Last.fm scrobbling. YT Music's own scrobbling is unreliable, so we watch playback
// and scrobble to Last.fm ourselves.
//
// Credentials (the user's own Last.fm API key/secret + the session key from authorizing)
// live in a JSON file in the app config dir - never in the repo. Setup happens through a
// tiny page served by the local control server (see lib.rs `/lastfm*` routes).
//
// All HTTP is blocking reqwest and runs either on the control-server thread or the worker
// thread below - never on the async runtime.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

use serde::{Deserialize, Serialize};

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
    pub api_secret: String,
    #[serde(default)]
    pub session_key: String,
    #[serde(default)]
    pub username: String,
}

impl Config {
    pub fn is_connected(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty() && !self.session_key.is_empty()
    }
}

fn config_file(dir: &PathBuf) -> PathBuf {
    dir.join("lastfm.json")
}

pub fn load(dir: &PathBuf) -> Config {
    std::fs::read_to_string(config_file(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(dir: &PathBuf, cfg: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(config_file(dir), serde_json::to_string_pretty(cfg).unwrap_or_default())
}

// api_sig = md5( sorted key+value pairs (minus format/callback) + secret )
fn sign(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut s = String::new();
    for (k, v) in params {
        if k == "format" || k == "callback" {
            continue;
        }
        s.push_str(k);
        s.push_str(v);
    }
    s.push_str(secret);
    format!("{:x}", md5::compute(s))
}

fn call(
    method: &str,
    mut params: BTreeMap<String, String>,
    secret: &str,
) -> Result<serde_json::Value, String> {
    params.insert("method".into(), method.into());
    let sig = sign(&params, secret);
    params.insert("api_sig".into(), sig);
    params.insert("format".into(), "json".into());
    let resp = reqwest::blocking::Client::new()
        .post(API_ROOT)
        .form(&params)
        .send()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error") {
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
        return Err(format!("last.fm error {err}: {msg}"));
    }
    Ok(v)
}

pub fn get_token(api_key: &str, secret: &str) -> Result<String, String> {
    let mut p = BTreeMap::new();
    p.insert("api_key".into(), api_key.to_string());
    let v = call("auth.getToken", p, secret)?;
    v.get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "no token in response".into())
}

pub fn authorize_url(api_key: &str, token: &str) -> String {
    format!("https://www.last.fm/api/auth/?api_key={api_key}&token={token}")
}

// Returns (session_key, username).
pub fn get_session(api_key: &str, secret: &str, token: &str) -> Result<(String, String), String> {
    let mut p = BTreeMap::new();
    p.insert("api_key".into(), api_key.to_string());
    p.insert("token".into(), token.to_string());
    let v = call("auth.getSession", p, secret)?;
    let sess = v.get("session").ok_or("no session in response")?;
    let key = sess
        .get("key")
        .and_then(|k| k.as_str())
        .ok_or("no session key")?
        .to_string();
    let name = sess
        .get("name")
        .and_then(|k| k.as_str())
        .unwrap_or("")
        .to_string();
    Ok((key, name))
}

fn update_now_playing(cfg: &Config, artist: &str, track: &str, album: &str) -> Result<(), String> {
    let mut p = BTreeMap::new();
    p.insert("api_key".into(), cfg.api_key.clone());
    p.insert("sk".into(), cfg.session_key.clone());
    p.insert("artist".into(), artist.to_string());
    p.insert("track".into(), track.to_string());
    if !album.is_empty() {
        p.insert("album".into(), album.to_string());
    }
    call("track.updateNowPlaying", p, &cfg.api_secret).map(|_| ())
}

fn scrobble(cfg: &Config, artist: &str, track: &str, album: &str, ts: u64) -> Result<(), String> {
    let mut p = BTreeMap::new();
    p.insert("api_key".into(), cfg.api_key.clone());
    p.insert("sk".into(), cfg.session_key.clone());
    p.insert("artist".into(), artist.to_string());
    p.insert("track".into(), track.to_string());
    p.insert("timestamp".into(), ts.to_string());
    if !album.is_empty() {
        p.insert("album".into(), album.to_string());
    }
    call("track.scrobble", p, &cfg.api_secret).map(|_| ())
}

pub enum Msg {
    NowPlaying { artist: String, track: String, album: String },
    Scrobble { artist: String, track: String, album: String, ts: u64 },
    Reload,
}

// A dedicated thread so blocking HTTP never touches the async runtime or the media
// listener. Reloads config on demand (after the user finishes setup).
pub fn spawn_worker(dir: PathBuf) -> Sender<Msg> {
    let (tx, rx) = channel::<Msg>();
    std::thread::spawn(move || {
        let mut cfg = load(&dir);
        for msg in rx {
            match msg {
                Msg::Reload => cfg = load(&dir),
                Msg::NowPlaying { artist, track, album } => {
                    if cfg.is_connected() {
                        let _ = update_now_playing(&cfg, &artist, &track, &album);
                    }
                }
                Msg::Scrobble { artist, track, album, ts } => {
                    if cfg.is_connected() {
                        let _ = scrobble(&cfg, &artist, &track, &album, ts);
                    }
                }
            }
        }
    });
    tx
}
