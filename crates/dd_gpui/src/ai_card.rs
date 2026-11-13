//! AI card state. The window paints it and applies inserts. No network in tests:
//! `start_completion`'s scripted chunks bypass the provider.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use dd_ai::{AiSettings, Delta, Error as AiError, Kind, Request, Task, start_completion};
use dd_vault_core::Paths;

const SYSTEM: &str = "You are a writing assistant inside a markdown vault. Reply with markdown only, no preamble or fences around the whole reply.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AiSize {
    #[default]
    Collapsed,
    Chat,
    Full,
}

#[derive(Clone, Debug)]
pub struct PendingAi {
    pub task: Task,
    pub prompt: String,
    pub replace: Option<(usize, usize)>,
    pub request: Request,
}

#[derive(Clone, Debug)]
pub struct NoteSnap {
    pub rel: String,
    pub text: String,
    pub selection: Option<String>,
    pub replace: Option<(usize, usize)>,
    pub hops: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiPrepare {
    Status(String),
    NeedKey(String),
    Confirm { message: String },
}

#[derive(Clone, Debug)]
pub struct AiLog {
    pub provider: String,
    pub badge: &'static str,
    pub model: String,
    pub task: &'static str,
    pub prompt: String,
}

#[derive(Debug)]
pub struct AiInsert {
    pub text: String,
    pub replace: Option<(usize, usize)>,
    pub rewrite: bool,
}

#[derive(Debug)]
pub enum AiPoll {
    Quiet,
    Changed,
    Insert(AiInsert),
    Failed(String),
}

#[derive(Debug)]
pub enum AiKey {
    Ignore,
    Collapse,
    Cycle,
    Send(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct ProviderOutcome {
    pub status: String,
    pub prompt_key: bool,
}

pub struct AiCard {
    pub size: AiSize,
    pub draft: String,
    pub draft_cursor: usize,
    pub transcript: Vec<String>,
    pub stream: String,
    pub scroll: usize,
    pub settings: AiSettings,
    pub pending: Option<PendingAi>,
    rx: Option<Receiver<Result<Delta, AiError>>>,
    cancel: Option<Arc<AtomicBool>>,
    scripted: Option<Vec<String>>,
}

impl Default for AiCard {
    fn default() -> Self {
        Self {
            size: AiSize::Collapsed,
            draft: String::new(),
            draft_cursor: 0,
            transcript: Vec::new(),
            stream: String::new(),
            scroll: 0,
            settings: AiSettings::default(),
            pending: None,
            rx: None,
            cancel: None,
            scripted: None,
        }
    }
}

impl AiCard {
    pub fn load(paths: &Paths) -> Self {
        let mut settings = AiSettings::load(&paths.ldnddev_dir().join("config.toml"));
        settings.keys_path = Some(paths.ai_keys_file());
        Self {
            settings,
            ..Self::default()
        }
    }

    pub fn badge(&self) -> &'static str {
        if !self.settings.enabled {
            "off"
        } else {
            self.settings.provider_kind().badge()
        }
    }

    pub fn title(&self) -> String {
        let extra = if self.rx.is_some() { " …" } else { "" };
        format!(
            " AI {} · {}{extra} ",
            self.badge(),
            self.settings.resolved_model()
        )
    }

    pub fn expanded(&self) -> bool {
        self.size != AiSize::Collapsed
    }

    pub fn streaming(&self) -> bool {
        self.rx.is_some()
    }

    pub fn cycle(&mut self) {
        self.size = match self.size {
            AiSize::Collapsed => AiSize::Chat,
            AiSize::Chat => AiSize::Full,
            AiSize::Full => AiSize::Collapsed,
        };
    }

    pub fn collapse(&mut self) {
        self.size = AiSize::Collapsed;
    }

    /// Test-only. The window never sets this, so a probe cannot hit the network by accident.
    #[cfg(test)]
    pub fn arm_scripted(&mut self, chunks: Vec<String>) {
        self.scripted = Some(chunks);
    }

    pub fn persist(&self, paths: &Paths) -> Result<(), String> {
        self.settings
            .save(&paths.ldnddev_dir().join("config.toml"))
            .map_err(|err| err.to_string())
    }

    pub fn enable(&mut self) -> Result<(), &'static str> {
        if self.settings.needs_key() && !self.settings.has_api_key() {
            return Err("need-key");
        }
        self.settings.enable_current();
        if self.size == AiSize::Collapsed {
            self.cycle();
        }
        Ok(())
    }

    pub fn disable(&mut self) {
        self.cancel_stream();
        self.settings.disable();
    }

    pub fn set_provider(&mut self, name: &str) -> Result<ProviderOutcome, String> {
        match self.settings.apply_provider(name) {
            Ok(info) => {
                if self.settings.enabled {
                    self.settings.enable_current();
                }
                if info.env_var.is_some() && !self.settings.has_api_key() {
                    return Ok(ProviderOutcome {
                        status: format!("{} selected — paste an API key", info.label),
                        prompt_key: true,
                    });
                }
                let extra = if info.kind == Kind::Local {
                    " (local)"
                } else {
                    ""
                };
                Ok(ProviderOutcome {
                    status: format!("AI provider: {}{extra}", info.id),
                    prompt_key: false,
                })
            }
            Err(err) => Err(format!("{err}. Try: {}", dd_ai::provider_ids_hint())),
        }
    }

    pub fn submit_key(&mut self, key: &str, then_enable: bool) -> Result<String, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Empty API key".into());
        }
        if !self.settings.needs_key() {
            return Ok(format!(
                "{} does not use an API key",
                self.settings.provider
            ));
        }
        self.settings
            .set_stored_key(key)
            .map_err(|err| err.to_string())?;
        if then_enable {
            self.settings.enable_current();
            if self.size == AiSize::Collapsed {
                self.cycle();
            }
            Ok(format!("AI on ({}) — key saved", self.settings.provider))
        } else {
            Ok(format!("{} key saved", self.settings.provider))
        }
    }

    pub fn submit_model(&mut self, model: &str) -> Result<String, String> {
        let model = model.trim();
        if model.is_empty() {
            return Err("Empty model id".into());
        }
        self.settings.model = model.to_string();
        Ok(format!("AI model: {model}"))
    }

    pub fn prepare(&mut self, prompt: String, task: Option<Task>, note: &NoteSnap) -> AiPrepare {
        self.pending = None;
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() && task != Some(Task::Summarize) {
            return AiPrepare::Status("Empty AI prompt".into());
        }
        if !self.settings.is_allowed() && self.scripted.is_none() {
            return AiPrepare::Status("AI is off. :ai on to enable the current provider".into());
        }
        if self.scripted.is_none() && self.settings.needs_key() && !self.settings.has_api_key() {
            return AiPrepare::NeedKey("No API key. Paste one, or set the provider env var".into());
        }
        if self.size == AiSize::Collapsed {
            self.cycle();
        }
        let task = task.unwrap_or_else(|| {
            if note.selection.is_some() {
                Task::Rewrite
            } else if prompt.is_empty() {
                Task::Summarize
            } else {
                Task::Draft
            }
        });
        let message = consent_message(&self.settings, task, &prompt, note);
        let user = build_user_message(
            task,
            &prompt,
            &note.rel,
            &note.text,
            note.selection.as_deref(),
            &note.hops,
        );
        self.pending = Some(PendingAi {
            task,
            prompt,
            replace: note.replace,
            request: Request {
                system: SYSTEM.into(),
                user,
                model: self.settings.resolved_model(),
            },
        });
        AiPrepare::Confirm { message }
    }

    /// Drop a consent that was not accepted, so a later confirm cannot send it.
    pub fn cancel_consent(&mut self) {
        self.pending = None;
    }

    pub fn start(&mut self) -> Option<AiLog> {
        let pending = self.pending.clone()?;
        self.transcript
            .push(format!("> {} ({})", pending.prompt, pending.task.label()));
        self.stream.clear();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = start_completion(
            &self.settings,
            pending.request.clone(),
            self.scripted.clone(),
            cancel.clone(),
        );
        self.cancel = Some(cancel);
        self.rx = Some(rx);
        Some(AiLog {
            provider: self.settings.provider.clone(),
            badge: self.settings.provider_kind().badge(),
            model: self.settings.resolved_model(),
            task: pending.task.label(),
            prompt: pending.prompt,
        })
    }

    pub fn poll(&mut self) -> AiPoll {
        let mut changed = false;
        loop {
            let recv = self.rx.as_ref().map(|rx| rx.try_recv());
            match recv {
                Some(Ok(Ok(Delta::Text(text)))) => {
                    self.stream.push_str(&text);
                    changed = true;
                }
                Some(Ok(Ok(Delta::Done))) => return self.finish_ok(),
                Some(Ok(Err(err))) => {
                    self.rx = None;
                    self.cancel = None;
                    self.pending = None;
                    self.stream.clear();
                    return AiPoll::Failed(err.to_string());
                }
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    if !self.stream.is_empty() {
                        return self.finish_ok();
                    }
                    self.rx = None;
                    self.cancel = None;
                    self.pending = None;
                    return AiPoll::Changed;
                }
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => break,
            }
        }
        if changed {
            AiPoll::Changed
        } else {
            AiPoll::Quiet
        }
    }

    pub fn cancel_stream(&mut self) -> bool {
        let live = self.rx.is_some();
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
        self.rx = None;
        self.cancel = None;
        if !self.stream.is_empty() {
            self.transcript.push(format!("(cancelled) {}", self.stream));
            self.stream.clear();
        }
        live
    }

    pub fn on_key(&mut self, key: &str, ch: Option<char>) -> AiKey {
        if self.rx.is_some() {
            if key == "escape" {
                self.cancel_stream();
                return AiKey::Cancelled;
            }
            return AiKey::Ignore;
        }
        match key {
            "escape" => {
                self.collapse();
                AiKey::Collapse
            }
            "tab" => {
                self.cycle();
                AiKey::Cycle
            }
            "enter" | "return" => {
                self.draft_cursor = 0;
                AiKey::Send(std::mem::take(&mut self.draft))
            }
            "backspace" => {
                dd_edit::line_backspace(&mut self.draft, &mut self.draft_cursor);
                AiKey::Ignore
            }
            "delete" | "del" => {
                dd_edit::line_delete(&mut self.draft, &mut self.draft_cursor);
                AiKey::Ignore
            }
            "left" => {
                dd_edit::line_left(&mut self.draft_cursor);
                AiKey::Ignore
            }
            "right" => {
                dd_edit::line_right(&self.draft, &mut self.draft_cursor);
                AiKey::Ignore
            }
            "home" => {
                dd_edit::line_home(&mut self.draft_cursor);
                AiKey::Ignore
            }
            "end" => {
                dd_edit::line_end(&self.draft, &mut self.draft_cursor);
                AiKey::Ignore
            }
            "up" => {
                self.scroll = self.scroll.saturating_sub(1);
                AiKey::Ignore
            }
            "down" => {
                self.scroll = self.scroll.saturating_add(1);
                AiKey::Ignore
            }
            _ => {
                if let Some(ch) = ch {
                    dd_edit::line_insert(&mut self.draft, &mut self.draft_cursor, ch);
                }
                AiKey::Ignore
            }
        }
    }

    pub fn body_lines(&self, limit: usize) -> Vec<String> {
        let mut lines = self.transcript.clone();
        if !self.stream.is_empty() {
            lines.push(self.stream.clone());
        }
        let start = self.scroll.min(lines.len().saturating_sub(limit.max(1)));
        lines.into_iter().skip(start).take(limit.max(1)).collect()
    }

    /// The draft line, hidden while a reply is streaming.
    pub fn draft_field(&self) -> Option<(String, usize)> {
        if self.expanded() && self.rx.is_none() {
            Some((self.draft.clone(), self.draft_cursor))
        } else {
            None
        }
    }

    fn finish_ok(&mut self) -> AiPoll {
        self.rx = None;
        self.cancel = None;
        let text = std::mem::take(&mut self.stream);
        let pending = self.pending.take();
        if text.is_empty() {
            return AiPoll::Changed;
        }
        self.transcript.push(text.clone());
        let (replace, rewrite) = match pending {
            Some(pending) => (pending.replace, pending.task == Task::Rewrite),
            None => (None, false),
        };
        AiPoll::Insert(AiInsert {
            text,
            replace,
            rewrite,
        })
    }
}

pub fn masked_key(draft: &str) -> String {
    "•".repeat(draft.chars().count())
}

fn consent_message(settings: &AiSettings, task: Task, prompt: &str, note: &NoteSnap) -> String {
    let mut summary = format!(
        "Provider: {} ({})\nModel: {}\nTask: {}\nNote: {} ({} chars)",
        settings.provider,
        settings.provider_kind().badge(),
        settings.resolved_model(),
        task.label(),
        note.rel,
        note.text.chars().count()
    );
    if let Some(selection) = &note.selection {
        summary.push_str(&format!("\nSelection: {} chars", selection.chars().count()));
    }
    if !note.hops.is_empty() {
        let names = note
            .hops
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        summary.push_str(&format!("\nLinked notes: {names}"));
    }
    if !prompt.is_empty() {
        summary.push_str(&format!("\nPrompt: {prompt}"));
    }
    format!("Send this to {}?\n\n{summary}", settings.provider)
}

pub fn build_user_message(
    task: Task,
    prompt: &str,
    rel: &str,
    note: &str,
    selection: Option<&str>,
    hops: &[(String, String)],
) -> String {
    let note_cap: String = note.chars().take(24_000).collect();
    let mut body = format!("# Task: {}\n", task.label());
    if !prompt.is_empty() {
        body.push_str("\n## Prompt\n");
        body.push_str(prompt);
        body.push('\n');
    }
    body.push_str(&format!("\n## Current note ({rel})\n{note_cap}\n"));
    if let Some(selection) = selection {
        body.push_str("\n## Selection\n");
        body.push_str(selection);
        body.push('\n');
    }
    if !hops.is_empty() {
        body.push_str("\n## Linked notes\n");
        for (name, hop) in hops {
            body.push_str(&format!("### {name}\n{hop}\n"));
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_ai::Kind;

    fn snap() -> NoteSnap {
        NoteSnap {
            rel: "notes/a.md".into(),
            text: "Hello vault".into(),
            selection: None,
            replace: None,
            hops: Vec::new(),
        }
    }

    #[test]
    fn draft_caret_inserts_in_the_middle() {
        let mut card = AiCard {
            size: AiSize::Chat,
            ..AiCard::default()
        };
        card.on_key("a", Some('a'));
        card.on_key("b", Some('b'));
        card.on_key("left", None);
        card.on_key("X", Some('X'));
        assert_eq!(card.draft, "aXb");
        assert_eq!(card.draft_cursor, 2);
        card.on_key("backspace", None);
        assert_eq!(card.draft, "ab");
        card.on_key("home", None);
        card.on_key("delete", None);
        assert_eq!((card.draft.as_str(), card.draft_cursor), ("b", 0));
    }

    #[test]
    fn off_refuses_before_consent() {
        let mut card = AiCard::default();
        match card.prepare("hello".into(), None, &snap()) {
            AiPrepare::Status(msg) => {
                assert!(msg.contains("AI is off"), "{msg}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(card.pending.is_none());
    }

    #[test]
    fn consent_lists_provider_and_cancel_drops_it() {
        let mut card = AiCard::default();
        card.arm_scripted(vec!["UNIQUEAIOUT".into()]);
        match card.prepare("rewrite this".into(), None, &snap()) {
            AiPrepare::Confirm { message } => {
                assert!(message.contains("Send this to spacexai?"), "{message}");
                assert!(message.contains("Task: draft"), "{message}");
                assert!(message.contains("notes/a.md"), "{message}");
                assert!(message.contains("Prompt: rewrite this"), "{message}");
            }
            other => panic!("expected consent, got {other:?}"),
        }
        assert!(card.pending.is_some());
        card.cancel_consent();
        assert!(card.start().is_none());
        assert!(!card.streaming());
    }

    #[test]
    fn scripted_completion_inserts_without_a_provider() {
        let mut card = AiCard::default();
        card.arm_scripted(vec!["UNIQUEAIOUT".into()]);
        assert!(matches!(
            card.prepare("go".into(), None, &snap()),
            AiPrepare::Confirm { .. }
        ));
        assert!(card.start().is_some());
        let mut inserted = None;
        for _ in 0..50 {
            match card.poll() {
                AiPoll::Insert(insert) => {
                    inserted = Some(insert);
                    break;
                }
                AiPoll::Failed(err) => panic!("{err}"),
                AiPoll::Quiet => std::thread::sleep(std::time::Duration::from_millis(10)),
                AiPoll::Changed => {}
            }
        }
        let insert = inserted.expect("scripted insert");
        assert_eq!(insert.text, "UNIQUEAIOUT");
        assert!(!insert.rewrite);
    }

    #[test]
    fn user_message_includes_the_note_and_hops() {
        let hops = vec![("beta".into(), "body".into())];
        let message = build_user_message(Task::Draft, "please", "a.md", "note", None, &hops);
        assert!(message.contains("# Task: draft"));
        assert!(message.contains("## Prompt\nplease"));
        assert!(message.contains("## Current note (a.md)\nnote"));
        assert!(message.contains("### beta\nbody"));
    }

    #[test]
    fn ollama_is_local_and_the_key_prompt_is_masked() {
        let mut card = AiCard::default();
        let outcome = card.set_provider("local").unwrap();
        assert_eq!(card.settings.provider, "ollama");
        assert_eq!(card.settings.provider_kind(), Kind::Local);
        assert!(!card.settings.needs_key());
        assert!(outcome.status.contains("ollama"));
        assert!(outcome.status.contains("local"));
        assert!(!outcome.prompt_key);
        assert_eq!(masked_key("super-secret"), "••••••••••••");
        assert!(!masked_key("super-secret").contains("secret"));
    }

    #[test]
    fn stored_key_is_mode_0600_and_not_echoed() {
        let dir = std::env::temp_dir().join(format!("dd-gui-ai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let keys = dir.join("ai.keys");
        let mut card = AiCard::default();
        card.set_provider("openrouter").unwrap();
        card.settings.keys_path = Some(keys.clone());
        let status = card.submit_key("test-key-value", false).unwrap();
        assert!(status.contains("key saved"), "{status}");
        assert!(!status.contains("test-key-value"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&keys).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "mode {mode:04o}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
