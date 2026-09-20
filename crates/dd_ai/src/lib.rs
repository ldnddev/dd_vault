//! Opt-in AI providers: SpaceXAI first, then generic OpenAI-compatible and local CLIs.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const SPACEXAI_BASE: &str = "https://api.x.ai/v1";
/// Current SpaceXAI chat/code model from https://docs.x.ai/developers/models (2026-09).
pub const SPACEXAI_MODEL: &str = "grok-4.6";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("AI is off (enable a provider with :ai on)")]
    Disabled,
    #[error("provider {0} is not on the allowlist")]
    NotAllowed(String),
    #[error("missing API key ({0})")]
    MissingKey(&'static str),
    #[error("AI request cancelled")]
    Cancelled,
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Network,
    Local,
}

impl Kind {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Network => "net",
            Self::Local => "local",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Draft,
    Rewrite,
    Summarize,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Rewrite => "rewrite",
            Self::Summarize => "summarize",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub system: String,
    pub user: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delta {
    Text(String),
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub allow: Vec<String>,
}

fn default_provider() -> String {
    "spacexai".into()
}
fn default_model() -> String {
    SPACEXAI_MODEL.into()
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_provider(),
            model: default_model(),
            base_url: String::new(),
            allow: Vec::new(),
        }
    }
}

impl AiSettings {
    pub fn load(config_toml: &Path) -> Self {
        let Ok(text) = fs::read_to_string(config_toml) else {
            return Self::default();
        };
        let Ok(v) = text.parse::<toml::Value>() else {
            return Self::default();
        };
        v.get("ai")
            .cloned()
            .and_then(|a| a.try_into().ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_toml: &Path) -> Result<(), Error> {
        if let Some(dir) = config_toml.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut root: toml::Value = fs::read_to_string(config_toml)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(toml::Value::Table(toml::map::Map::new()));
        let table = root
            .as_table_mut()
            .ok_or_else(|| Error::Msg("config.toml is not a table".into()))?;
        let ai = toml::Value::try_from(self).map_err(|e| Error::Msg(e.to_string()))?;
        table.insert("ai".into(), ai);
        let body = toml::to_string_pretty(&root).map_err(|e| Error::Msg(e.to_string()))?;
        fs::write(config_toml, body)?;
        Ok(())
    }

    pub fn provider_kind(&self) -> Kind {
        match self.provider.as_str() {
            "ollama" | "grok-cli" | "llm" => Kind::Local,
            _ => Kind::Network,
        }
    }

    pub fn provider_id(&self) -> &str {
        self.provider.as_str()
    }

    pub fn is_allowed(&self) -> bool {
        self.enabled && self.allow.iter().any(|a| a == &self.provider)
    }

    pub fn enable_current(&mut self) {
        self.enabled = true;
        if !self.allow.iter().any(|a| a == &self.provider) {
            self.allow.push(self.provider.clone());
        }
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn resolved_model(&self) -> String {
        if self.provider == "ollama" && self.model.starts_with("grok-") {
            return "llama3.2".into();
        }
        if self.model.is_empty() {
            return default_model();
        }
        self.model.clone()
    }

    pub fn resolved_base(&self) -> String {
        if !self.base_url.is_empty() {
            return self.base_url.trim_end_matches('/').to_string();
        }
        match self.provider.as_str() {
            "openai" => "https://api.openai.com/v1".into(),
            _ => SPACEXAI_BASE.to_string(),
        }
    }
}

pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for needle in ["ghp_", "github_pat_", "AKIA", "sk-", "xai-", "Bearer "] {
        while let Some(i) = out.find(needle) {
            let rest = &out[i + needle.len()..];
            let n = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            let end = i + needle.len() + n;
            out.replace_range(i..end, "[redacted]");
        }
    }
    out
}

pub fn append_log(path: &Path, rec: &serde_json::Value) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let line = serde_json::to_string(rec).unwrap_or_else(|_| "{}".into());
    let mut f = match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let _ = writeln!(f, "{line}");
}

pub fn parse_sse_line(line: &str) -> Option<Result<Delta, Error>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return None;
    }
    let data = line.strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return Some(Ok(Delta::Done));
    }
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => return Some(Err(Error::Msg(e.to_string()))),
    };
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("API error");
        return Some(Err(Error::Msg(msg.into())));
    }
    let text = v
        .pointer("/choices/0/delta/content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())?;
    Some(Ok(Delta::Text(text.to_string())))
}

/// Start a completion on a background thread. `scripted` bypasses the network (tests).
pub fn start_completion(
    settings: &AiSettings,
    req: Request,
    scripted: Option<Vec<String>>,
    cancel: Arc<AtomicBool>,
) -> Receiver<Result<Delta, Error>> {
    let (tx, rx) = mpsc::channel();
    let settings = settings.clone();
    std::thread::spawn(move || {
        let send = |d| {
            let _ = tx.send(d);
        };
        if let Some(chunks) = scripted {
            for c in chunks {
                if cancel.load(Ordering::Relaxed) {
                    send(Err(Error::Cancelled));
                    return;
                }
                send(Ok(Delta::Text(c)));
            }
            send(Ok(Delta::Done));
            return;
        }
        if !settings.is_allowed() {
            send(Err(Error::Disabled));
            return;
        }
        let result = match settings.provider.as_str() {
            "ollama" | "grok-cli" | "llm" => run_cli(&settings, &req, &cancel, &tx),
            _ => run_http(&settings, &req, &cancel, &tx),
        };
        match result {
            Ok(()) => send(Ok(Delta::Done)),
            Err(Error::Cancelled) => send(Err(Error::Cancelled)),
            Err(e) => send(Err(e)),
        }
    });
    rx
}

fn run_http(
    settings: &AiSettings,
    req: &Request,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Result<Delta, Error>>,
) -> Result<(), Error> {
    let key_var = if settings.provider == "openai" {
        "OPENAI_API_KEY"
    } else {
        "XAI_API_KEY"
    };
    let key = std::env::var(key_var).map_err(|_| Error::MissingKey(key_var))?;
    if key.trim().is_empty() {
        return Err(Error::MissingKey(key_var));
    }
    let url = format!("{}/chat/completions", settings.resolved_base());
    let body = serde_json::json!({
        "model": req.model,
        "stream": true,
        "messages": [
            {"role": "system", "content": req.system},
            {"role": "user", "content": req.user},
        ],
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| Error::Msg(e.to_string()))?;
    let mut resp = client
        .post(&url)
        .bearer_auth(key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| Error::Msg(e.to_string()))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let t = resp.text().unwrap_or_default();
        return Err(Error::Msg(format!("HTTP {status}: {t}")));
    }
    let mut buf = String::new();
    let mut bytes = [0u8; 2048];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = resp.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        buf.push_str(&String::from_utf8_lossy(&bytes[..n]));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            if let Some(ev) = parse_sse_line(&line) {
                match ev {
                    Ok(Delta::Done) => return Ok(()),
                    Ok(Delta::Text(t)) => {
                        let _ = tx.send(Ok(Delta::Text(t)));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(())
}

fn run_cli(
    settings: &AiSettings,
    req: &Request,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Result<Delta, Error>>,
) -> Result<(), Error> {
    let model = req.model.clone();
    let mut cmd = match settings.provider.as_str() {
        "ollama" => {
            let mut c = Command::new("ollama");
            c.args(["run", &model]);
            c
        }
        "grok-cli" => Command::new("grok"),
        "llm" => {
            let mut c = Command::new("llm");
            c.args(["-m", &model]);
            c
        }
        other => return Err(Error::Msg(format!("unknown CLI provider: {other}"))),
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Msg(format!("{} not found on PATH", settings.provider))
        } else {
            Error::Msg(e.to_string())
        }
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let payload = format!("{}\n\n{}", req.system, req.user);
        let _ = stdin.write_all(payload.as_bytes());
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Msg("no stdout".into()))?;
    let mut buf = [0u8; 512];
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            return Err(Error::Cancelled);
        }
        match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                if !chunk.is_empty() {
                    let _ = tx.send(Ok(Delta::Text(chunk)));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(Error::Msg(format!(
            "{} exited {}",
            settings.provider, status
        )));
    }
    Ok(())
}

pub fn unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn log_path(meta_dir: &Path) -> PathBuf {
    meta_dir.join("logs").join("ai.jsonl")
}

pub fn log_request(
    meta_dir: &Path,
    provider: &str,
    kind: &str,
    model: &str,
    task: &str,
    prompt: &str,
) {
    let rec = serde_json::json!({
        "ts": unix_ts(),
        "provider": provider,
        "kind": kind,
        "model": model,
        "task": task,
        "prompt": redact(prompt),
    });
    append_log(&log_path(meta_dir), &rec);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_strips_tokens() {
        let s = redact("key ghp_ABCDEF123 and Bearer tok-xyz");
        assert!(!s.contains("ABCDEF"), "{s}");
        assert!(s.contains("[redacted]"), "{s}");
    }

    #[test]
    fn sse_parses_delta_and_done() {
        let d = parse_sse_line(r#"data: {"choices":[{"delta":{"content":"Hi"}}]}"#)
            .unwrap()
            .unwrap();
        assert_eq!(d, Delta::Text("Hi".into()));
        assert_eq!(
            parse_sse_line("data: [DONE]").unwrap().unwrap(),
            Delta::Done
        );
        assert!(parse_sse_line(": keep-alive").is_none());
    }

    #[test]
    fn settings_off_by_default() {
        let s = AiSettings::default();
        assert!(!s.enabled);
        assert!(!s.is_allowed());
        assert_eq!(s.provider, "spacexai");
        assert_eq!(s.resolved_model(), SPACEXAI_MODEL);
        assert_eq!(s.provider_kind(), Kind::Network);
    }

    #[test]
    fn enable_allowlists() {
        let mut s = AiSettings::default();
        s.enable_current();
        assert!(s.is_allowed());
        assert_eq!(s.allow, vec!["spacexai"]);
    }

    #[test]
    fn scripted_stream_emits_chunks() {
        let s = AiSettings::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = start_completion(
            &s,
            Request {
                system: String::new(),
                user: "x".into(),
                model: "m".into(),
            },
            Some(vec!["A".into(), "B".into()]),
            cancel,
        );
        let mut got = Vec::new();
        for _ in 0..8 {
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(Ok(Delta::Text(t))) => got.push(t),
                Ok(Ok(Delta::Done)) => break,
                Ok(Err(e)) => panic!("{e}"),
                Err(_) => break,
            }
        }
        assert_eq!(got, vec!["A", "B"]);
    }

    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "secret_patterns = [\"X\"]\n").unwrap();
        let mut s = AiSettings::default();
        s.enable_current();
        s.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("secret_patterns"), "{text}");
        let loaded = AiSettings::load(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.allow, vec!["spacexai"]);
    }
}
