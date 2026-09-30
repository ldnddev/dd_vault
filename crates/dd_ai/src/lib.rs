//! Opt-in AI providers: SpaceXAI first, then OpenRouter, OpenAI-compatible HTTP, and local CLIs.

use std::collections::HashMap;
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
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
/// OpenRouter auto-router (https://openrouter.ai/docs).
pub const OPENROUTER_MODEL: &str = "openrouter/auto";
pub const OPENAI_BASE: &str = "https://api.openai.com/v1";
pub const OLLAMA_BASE: &str = "http://127.0.0.1:11434/v1";
pub const OLLAMA_MODEL: &str = "llama3.2";
const OPENROUTER_REFERER: &str = "https://github.com/ldnddev/dd_vault";
const OPENROUTER_TITLE: &str = "dd_vault";
pub const KEYS_FILENAME: &str = "ai.keys";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("AI is off (enable a provider with :ai on)")]
    Disabled,
    #[error("provider {0} is not on the allowlist")]
    NotAllowed(String),
    #[error("missing API key ({0}); paste one with :ai key")]
    MissingKey(&'static str),
    #[error("unknown AI provider: {0}")]
    UnknownProvider(String),
    #[error("AI keys file {0} must be mode 0600 (found {1:04o})")]
    KeysMode(String, u32),
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
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub env_var: Option<&'static str>,
    pub default_model: &'static str,
    pub default_base: Option<&'static str>,
    pub cli: bool,
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "spacexai",
        label: "SpaceXAI",
        kind: Kind::Network,
        env_var: Some("XAI_API_KEY"),
        default_model: SPACEXAI_MODEL,
        default_base: Some(SPACEXAI_BASE),
        cli: false,
    },
    ProviderInfo {
        id: "openrouter",
        label: "OpenRouter",
        kind: Kind::Network,
        env_var: Some("OPENROUTER_API_KEY"),
        default_model: OPENROUTER_MODEL,
        default_base: Some(OPENROUTER_BASE),
        cli: false,
    },
    ProviderInfo {
        id: "openai",
        label: "OpenAI",
        kind: Kind::Network,
        env_var: Some("OPENAI_API_KEY"),
        default_model: "gpt-4o-mini",
        default_base: Some(OPENAI_BASE),
        cli: false,
    },
    ProviderInfo {
        id: "ollama",
        label: "Ollama (local)",
        kind: Kind::Local,
        env_var: None,
        default_model: OLLAMA_MODEL,
        default_base: Some(OLLAMA_BASE),
        cli: false,
    },
    ProviderInfo {
        id: "grok-cli",
        label: "grok CLI",
        kind: Kind::Local,
        env_var: None,
        default_model: SPACEXAI_MODEL,
        default_base: None,
        cli: true,
    },
    ProviderInfo {
        id: "llm",
        label: "llm CLI",
        kind: Kind::Local,
        env_var: None,
        default_model: OLLAMA_MODEL,
        default_base: None,
        cli: true,
    },
];

pub fn provider_info(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Known ids, plus `local` → `ollama`.
pub fn parse_provider(name: &str) -> Result<&'static str, Error> {
    let n = name.trim().to_ascii_lowercase();
    let id = match n.as_str() {
        "local" => "ollama",
        other => other,
    };
    provider_info(id)
        .map(|p| p.id)
        .ok_or_else(|| Error::UnknownProvider(name.trim().to_string()))
}

pub fn provider_ids_hint() -> String {
    PROVIDERS
        .iter()
        .map(|p| p.id)
        .collect::<Vec<_>>()
        .join(", ")
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
    /// `~/.config/ldnddev/ai.keys` (mode 0600). Not written to config.toml.
    #[serde(skip)]
    pub keys_path: Option<PathBuf>,
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
            keys_path: None,
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
        let mut loaded: Self = v
            .get("ai")
            .cloned()
            .and_then(|a| a.try_into().ok())
            .unwrap_or_default();
        loaded.keys_path = config_toml.parent().map(|p| p.join(KEYS_FILENAME));
        loaded
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
        provider_info(&self.provider)
            .map(|p| p.kind)
            .unwrap_or(Kind::Network)
    }

    pub fn info(&self) -> Option<&'static ProviderInfo> {
        provider_info(&self.provider)
    }

    pub fn needs_key(&self) -> bool {
        self.info().and_then(|p| p.env_var).is_some()
    }

    pub fn env_var(&self) -> Option<&'static str> {
        self.info().and_then(|p| p.env_var)
    }

    pub fn is_cli(&self) -> bool {
        self.info().map(|p| p.cli).unwrap_or(false)
    }

    /// Env var first, then `ai.keys` (0600).
    pub fn has_api_key(&self) -> bool {
        if !self.needs_key() {
            return true;
        }
        if let Some(var) = self.env_var() {
            if std::env::var(var)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                return true;
            }
        }
        matches!(self.stored_key(), Ok(Some(k)) if !k.trim().is_empty())
    }

    pub fn stored_key(&self) -> Result<Option<String>, Error> {
        let Some(path) = &self.keys_path else {
            return Ok(None);
        };
        read_key(path, &self.provider)
    }

    pub fn set_stored_key(&self, key: &str) -> Result<(), Error> {
        let path = self
            .keys_path
            .as_ref()
            .ok_or_else(|| Error::Msg("no config dir to store an API key".into()))?;
        write_key(path, &self.provider, key)
    }

    pub fn apply_provider(&mut self, name: &str) -> Result<&'static ProviderInfo, Error> {
        let id = parse_provider(name)?;
        let info = provider_info(id).expect("parse_provider validates");
        self.provider = info.id.to_string();
        self.model = info.default_model.to_string();
        self.base_url.clear();
        Ok(info)
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
        if self.model.is_empty() {
            return self
                .info()
                .map(|p| p.default_model.to_string())
                .unwrap_or_else(default_model);
        }
        if self.provider == "ollama" && self.model.starts_with("grok-") {
            return OLLAMA_MODEL.into();
        }
        self.model.clone()
    }

    pub fn resolved_base(&self) -> String {
        if !self.base_url.is_empty() {
            return self.base_url.trim_end_matches('/').to_string();
        }
        self.info()
            .and_then(|p| p.default_base)
            .unwrap_or(SPACEXAI_BASE)
            .trim_end_matches('/')
            .to_string()
    }
}

fn keys_mode_ok(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(Error::KeysMode(path.display().to_string(), mode));
        }
    }
    Ok(())
}

fn read_key_file(path: &Path) -> Result<HashMap<String, String>, Error> {
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    keys_mode_ok(path)?;
    let text = fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, String> =
        toml::from_str(&text).map_err(|e| Error::Msg(e.to_string()))?;
    Ok(map)
}

fn read_key(path: &Path, provider: &str) -> Result<Option<String>, Error> {
    let map = read_key_file(path)?;
    Ok(map.get(provider).cloned().filter(|s| !s.trim().is_empty()))
}

fn write_key(path: &Path, provider: &str, key: &str) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut map = if path.is_file() {
        match keys_mode_ok(path) {
            Ok(()) => read_key_file(path)?,
            Err(Error::KeysMode(_, _)) => HashMap::new(),
            Err(e) => return Err(e),
        }
    } else {
        HashMap::new()
    };
    let key = key.trim();
    if key.is_empty() {
        map.remove(provider);
    } else {
        map.insert(provider.to_string(), key.to_string());
    }
    let body = toml::to_string_pretty(&map).map_err(|e| Error::Msg(e.to_string()))?;
    fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

pub fn resolve_api_key(settings: &AiSettings) -> Result<String, Error> {
    if !settings.needs_key() {
        return Ok(String::new());
    }
    let var = settings.env_var().unwrap_or("API_KEY");
    if let Ok(k) = std::env::var(var) {
        if !k.trim().is_empty() {
            return Ok(k);
        }
    }
    if let Some(k) = settings.stored_key()? {
        return Ok(k);
    }
    Err(Error::MissingKey(var))
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
        let result = if settings.is_cli() {
            run_cli(&settings, &req, &cancel, &tx)
        } else {
            run_http(&settings, &req, &cancel, &tx)
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
    let key = resolve_api_key(settings)?;
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
    let mut req_builder = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);
    if !key.trim().is_empty() {
        req_builder = req_builder.bearer_auth(key.trim());
    }
    if settings.provider == "openrouter" {
        req_builder = req_builder
            .header("HTTP-Referer", OPENROUTER_REFERER)
            .header("X-OpenRouter-Title", OPENROUTER_TITLE)
            .header("X-Title", OPENROUTER_TITLE);
    }
    let mut resp = req_builder.send().map_err(|e| {
        if settings.provider == "ollama" {
            Error::Msg(format!(
                "ollama is not reachable at {} (start `ollama serve`): {e}",
                settings.resolved_base()
            ))
        } else {
            Error::Msg(e.to_string())
        }
    })?;
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
        assert!(!text.contains("keys_path"), "{text}");
        let loaded = AiSettings::load(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.allow, vec!["spacexai"]);
        assert_eq!(
            loaded.keys_path.as_deref(),
            Some(dir.path().join(KEYS_FILENAME).as_path())
        );
    }

    #[test]
    fn openrouter_and_ollama_resolve() {
        let mut s = AiSettings::default();
        s.apply_provider("openrouter").unwrap();
        assert_eq!(s.provider, "openrouter");
        assert_eq!(s.resolved_model(), OPENROUTER_MODEL);
        assert_eq!(s.resolved_base(), OPENROUTER_BASE);
        assert_eq!(s.provider_kind(), Kind::Network);
        assert!(s.needs_key());
        s.apply_provider("local").unwrap();
        assert_eq!(s.provider, "ollama");
        assert_eq!(s.resolved_model(), OLLAMA_MODEL);
        assert_eq!(s.resolved_base(), OLLAMA_BASE);
        assert_eq!(s.provider_kind(), Kind::Local);
        assert!(!s.needs_key());
        assert!(s.has_api_key());
        assert!(parse_provider("nope").is_err());
    }

    #[test]
    fn stored_key_roundtrip_0600() {
        let dir = tempfile::tempdir().unwrap();
        let s = AiSettings {
            provider: "openrouter".into(),
            keys_path: Some(dir.path().join(KEYS_FILENAME)),
            ..Default::default()
        };
        assert!(s.stored_key().unwrap().is_none());
        s.set_stored_key("sk-or-v1-testsecret").unwrap();
        let path = s.keys_path.as_ref().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "ai.keys must be 0600");
        }
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("openrouter"));
        assert!(text.contains("sk-or-v1-testsecret"));
        assert_eq!(
            s.stored_key().unwrap().as_deref(),
            Some("sk-or-v1-testsecret")
        );
        assert!(s.has_api_key());
    }
}
