//! AI card overlay: collapsed / chat / full, consent, streaming.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use dd_ai::{log_request, start_completion, AiSettings, Delta, Error as AiError, Request, Task};
use dd_vault_core::parse_note;
use ratatui::layout::Rect;

use crate::app::{App, ConfirmKind, Leader, Modal};
use crate::toasts::ToastLevel;

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

pub struct AiState {
    pub size: AiSize,
    pub draft: String,
    pub transcript: Vec<String>,
    pub stream: String,
    pub scroll: u16,
    pub rx: Option<Receiver<Result<Delta, AiError>>>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub settings: AiSettings,
    pub scripted: Option<Vec<String>>,
    pub pending: Option<PendingAi>,
    pub area: Rect,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            size: AiSize::Collapsed,
            draft: String::new(),
            transcript: Vec::new(),
            stream: String::new(),
            scroll: 0,
            rx: None,
            cancel: None,
            settings: AiSettings::default(),
            scripted: None,
            pending: None,
            area: Rect::default(),
        }
    }
}

impl AiState {
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
}

impl App {
    pub fn load_ai_settings(&mut self) {
        if let Some(paths) = &self.paths {
            self.ai.settings = AiSettings::load(&paths.ldnddev_dir().join("config.toml"));
        }
    }

    fn persist_ai_settings(&mut self) {
        let Some(paths) = &self.paths else {
            return;
        };
        if let Err(err) = self
            .ai
            .settings
            .save(&paths.ldnddev_dir().join("config.toml"))
        {
            self.push_toast(ToastLevel::Warning, err.to_string());
        }
    }

    pub fn cycle_ai_card(&mut self) {
        self.leader = Leader::None;
        self.ai.size = match self.ai.size {
            AiSize::Collapsed => AiSize::Chat,
            AiSize::Chat => AiSize::Full,
            AiSize::Full => AiSize::Collapsed,
        };
    }

    pub fn collapse_ai(&mut self) {
        self.ai.size = AiSize::Collapsed;
    }

    pub fn ai_enable(&mut self) {
        self.ai.settings.enable_current();
        self.persist_ai_settings();
        self.push_toast(
            ToastLevel::Success,
            format!("AI on ({})", self.ai.settings.provider),
        );
        if self.ai.size == AiSize::Collapsed {
            self.cycle_ai_card();
        }
    }

    pub fn ai_disable(&mut self) {
        self.cancel_ai_stream();
        self.ai.settings.disable();
        self.persist_ai_settings();
        self.push_toast(ToastLevel::Info, "AI off");
    }

    pub fn cancel_ai_stream(&mut self) {
        if let Some(c) = &self.ai.cancel {
            c.store(true, Ordering::Relaxed);
        }
        self.ai.rx = None;
        self.ai.cancel = None;
        if !self.ai.stream.is_empty() {
            self.ai
                .transcript
                .push(format!("(cancelled) {}", self.ai.stream));
            self.ai.stream.clear();
        }
    }

    pub fn request_ai(&mut self, prompt: String, task: Option<Task>) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() && task != Some(Task::Summarize) {
            self.push_toast(ToastLevel::Warning, "Empty AI prompt");
            return;
        }
        if !self.ai.settings.is_allowed() && self.ai.scripted.is_none() {
            self.push_toast(
                ToastLevel::Warning,
                "AI is off. :ai on to enable the current provider",
            );
            return;
        }
        if self.ai.size == AiSize::Collapsed {
            self.cycle_ai_card();
        }
        let task = task.unwrap_or_else(|| {
            if self.editor.selected_text().is_some() {
                Task::Rewrite
            } else if prompt.is_empty() {
                Task::Summarize
            } else {
                Task::Draft
            }
        });
        let selection = self.editor.selected_text();
        let replace = self.editor.visual_highlight();
        let note = self.editor.text();
        let rel = self
            .editor
            .rel
            .clone()
            .unwrap_or_else(|| "(no note)".into());
        let hops = self.hop_snippets();
        let mut summary = format!(
            "Provider: {} ({})\nModel: {}\nTask: {}\nNote: {} ({} chars)",
            self.ai.settings.provider,
            self.ai.settings.provider_kind().badge(),
            self.ai.settings.resolved_model(),
            task.label(),
            rel,
            note.chars().count()
        );
        if let Some(sel) = &selection {
            summary.push_str(&format!("\nSelection: {} chars", sel.chars().count()));
        }
        if !hops.is_empty() {
            summary.push_str(&format!(
                "\nLinked notes: {}",
                hops.iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !prompt.is_empty() {
            summary.push_str(&format!("\nPrompt: {prompt}"));
        }
        let user = build_user_message(task, &prompt, &rel, &note, selection.as_deref(), &hops);
        let pending = PendingAi {
            task,
            prompt: prompt.clone(),
            replace,
            request: Request {
                system: "You are a writing assistant inside a markdown vault. Reply with markdown only, no preamble or fences around the whole reply.".into(),
                user,
                model: self.ai.settings.resolved_model(),
            },
        };
        self.ai.pending = Some(pending);
        self.modal = Some(Modal::Confirm {
            kind: ConfirmKind::AiSend,
            message: format!("Send this to {}?\n\n{summary}", self.ai.settings.provider),
        });
    }

    fn hop_snippets(&self) -> Vec<(String, String)> {
        let Some(vault) = &self.vault else {
            return Vec::new();
        };
        let rel = self
            .editor
            .rel
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_default();
        let parsed = parse_note(&rel, &self.editor.text());
        let mut out = Vec::new();
        for link in parsed.links.iter().take(4) {
            let name = link.dst_raw.split('|').next().unwrap_or(&link.dst_raw);
            let name = name.split('#').next().unwrap_or(name).trim();
            if name.is_empty() {
                continue;
            }
            let Some(found) = self.tree.find_file(name) else {
                continue;
            };
            let Ok(body) = std::fs::read_to_string(vault.root.join(&found)) else {
                continue;
            };
            let snippet: String = body.chars().take(1200).collect();
            out.push((name.to_string(), snippet));
        }
        out
    }

    pub fn start_pending_ai(&mut self) {
        let Some(pending) = self.ai.pending.take() else {
            return;
        };
        self.ai
            .transcript
            .push(format!("> {} ({})", pending.prompt, pending.task.label()));
        self.ai.stream.clear();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = start_completion(
            &self.ai.settings,
            pending.request.clone(),
            self.ai.scripted.clone(),
            cancel.clone(),
        );
        self.ai.cancel = Some(cancel);
        self.ai.rx = Some(rx);
        self.ai.pending = Some(pending);
        if let Some(vault) = &self.vault {
            let task = self
                .ai
                .pending
                .as_ref()
                .map(|p| p.task.label())
                .unwrap_or("draft");
            let prompt = self
                .ai
                .pending
                .as_ref()
                .map(|p| p.prompt.as_str())
                .unwrap_or("");
            log_request(
                &vault.meta_dir,
                &self.ai.settings.provider,
                self.ai.settings.provider_kind().badge(),
                &self.ai.settings.resolved_model(),
                task,
                prompt,
            );
        }
    }

    pub fn poll_ai(&mut self) {
        let recv = self.ai.rx.as_ref().map(|r| r.try_recv());
        match recv {
            Some(Ok(Ok(Delta::Text(t)))) => {
                self.ai.stream.push_str(&t);
            }
            Some(Ok(Ok(Delta::Done))) => self.finish_ai_ok(),
            Some(Ok(Err(err))) => {
                self.ai.rx = None;
                self.ai.cancel = None;
                self.ai.pending = None;
                self.ai.stream.clear();
                self.push_toast(ToastLevel::Error, err.to_string());
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                if !self.ai.stream.is_empty() {
                    self.finish_ai_ok();
                } else {
                    self.ai.rx = None;
                    self.ai.cancel = None;
                    self.ai.pending = None;
                }
            }
            _ => {}
        }
    }

    fn finish_ai_ok(&mut self) {
        self.ai.rx = None;
        self.ai.cancel = None;
        let text = std::mem::take(&mut self.ai.stream);
        let pending = self.ai.pending.take();
        if !text.is_empty() {
            self.ai.transcript.push(text.clone());
            if let Some(p) = pending {
                match p.replace {
                    Some((a, b)) if p.task == Task::Rewrite => {
                        self.editor.replace_range(a, b, &text);
                    }
                    _ => {
                        if !text.starts_with('\n') && self.editor.rel.is_some() {
                            self.editor.insert_snippet(&format!("\n{text}"));
                        } else {
                            self.editor.insert_snippet(&text);
                        }
                    }
                }
            } else {
                self.editor.insert_snippet(&text);
            }
            self.push_toast(ToastLevel::Success, "AI inserted");
        }
    }

    pub fn handle_ai_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        if self.ai.rx.is_some() {
            if matches!(k.code, KeyCode::Esc) {
                self.cancel_ai_stream();
                self.push_toast(ToastLevel::Info, "AI cancelled");
            }
            return;
        }
        match k.code {
            KeyCode::Esc => self.collapse_ai(),
            KeyCode::Tab => self.cycle_ai_card(),
            KeyCode::Enter => {
                let draft = std::mem::take(&mut self.ai.draft);
                self.request_ai(draft, None);
            }
            KeyCode::Backspace => {
                self.ai.draft.pop();
            }
            KeyCode::Up => self.ai.scroll = self.ai.scroll.saturating_sub(1),
            KeyCode::Down => self.ai.scroll = self.ai.scroll.saturating_add(1),
            KeyCode::Char(c)
                if !k
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !c.is_control() =>
            {
                self.ai.draft.push(c);
            }
            _ => {}
        }
    }
}

fn build_user_message(
    task: Task,
    prompt: &str,
    rel: &str,
    note: &str,
    selection: Option<&str>,
    hops: &[(String, String)],
) -> String {
    let note_cap: String = note.chars().take(24_000).collect();
    let mut s = format!("# Task: {}\n", task.label());
    if !prompt.is_empty() {
        s.push_str("\n## Prompt\n");
        s.push_str(prompt);
        s.push('\n');
    }
    s.push_str(&format!("\n## Current note ({rel})\n{note_cap}\n"));
    if let Some(sel) = selection {
        s.push_str("\n## Selection\n");
        s.push_str(sel);
        s.push('\n');
    }
    if !hops.is_empty() {
        s.push_str("\n## Linked notes\n");
        for (name, body) in hops {
            s.push_str(&format!("### {name}\n{body}\n"));
        }
    }
    s
}
