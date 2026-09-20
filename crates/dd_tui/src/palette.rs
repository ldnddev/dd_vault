//! Command palette (`<Space><Space>`).

use crate::app::{App, Leader, Modal, PaletteItem};
use crate::toasts::ToastLevel;
use dd_edit::GitOp;

pub const PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        id: "save",
        label: "Save note",
        keys: ":w  Ctrl+S",
    },
    PaletteItem {
        id: "quit",
        label: "Quit",
        keys: ":q  Ctrl+Q",
    },
    PaletteItem {
        id: "help",
        label: "Help",
        keys: "F1  :help",
    },
    PaletteItem {
        id: "theme",
        label: "Theme editor",
        keys: "F2",
    },
    PaletteItem {
        id: "preview",
        label: "Toggle preview",
        keys: "<Space>p",
    },
    PaletteItem {
        id: "files",
        label: "File finder",
        keys: "<Space>ff",
    },
    PaletteItem {
        id: "content",
        label: "Content search",
        keys: "<Space>sg",
    },
    PaletteItem {
        id: "tags",
        label: "Tag search",
        keys: "<Space>tg",
    },
    PaletteItem {
        id: "vault",
        label: "Vault picker",
        keys: "<Space>vv",
    },
    PaletteItem {
        id: "daily",
        label: "Open today's daily note",
        keys: ":daily  <Space>nd",
    },
    PaletteItem {
        id: "ai",
        label: "Toggle AI card",
        keys: "<Space>ai  :ai",
    },
    PaletteItem {
        id: "ai-on",
        label: "Enable AI provider",
        keys: ":ai on",
    },
    PaletteItem {
        id: "ai-off",
        label: "Disable AI",
        keys: ":ai off",
    },
    PaletteItem {
        id: "focus",
        label: "Focus mode (hide tree)",
        keys: "<Space>z",
    },
    PaletteItem {
        id: "zen",
        label: "Zen (editor only)",
        keys: "<Space>h",
    },
    PaletteItem {
        id: "explore",
        label: "Toggle tree",
        keys: "<Space>e",
    },
    PaletteItem {
        id: "new",
        label: "New file in tree",
        keys: "a",
    },
    PaletteItem {
        id: "git-status",
        label: "Git status",
        keys: ":git",
    },
    PaletteItem {
        id: "git-commit",
        label: "Git commit",
        keys: ":git commit",
    },
    PaletteItem {
        id: "git-pull",
        label: "Git pull",
        keys: ":git pull",
    },
    PaletteItem {
        id: "git-push",
        label: "Git push",
        keys: ":git push",
    },
];

pub fn filter_palette(query: &str) -> Vec<PaletteItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return PALETTE_ITEMS.to_vec();
    }
    let mut hits: Vec<(usize, PaletteItem)> = PALETTE_ITEMS
        .iter()
        .filter_map(|item| {
            let blob = format!("{} {} {}", item.label, item.keys, item.id).to_lowercase();
            if blob.contains(&q) {
                let rank = if item.label.to_lowercase().starts_with(&q) {
                    0
                } else if item.id.contains(&q) {
                    1
                } else {
                    2
                };
                Some((rank, *item))
            } else {
                None
            }
        })
        .collect();
    hits.sort_by_key(|(r, item)| (*r, item.label));
    hits.into_iter().map(|(_, i)| i).collect()
}

impl App {
    pub fn open_palette(&mut self) {
        self.leader = Leader::None;
        self.modal = Some(Modal::Palette {
            query: String::new(),
            selected: 0,
            items: PALETTE_ITEMS.to_vec(),
        });
    }

    pub fn refresh_palette(&mut self) {
        let Some(Modal::Palette { query, .. }) = &self.modal else {
            return;
        };
        let items = filter_palette(query);
        if let Some(Modal::Palette {
            selected,
            items: slot,
            ..
        }) = &mut self.modal
        {
            *slot = items;
            *selected = (*selected).min(slot.len().saturating_sub(1));
        }
    }

    pub fn activate_palette(&mut self) {
        let Some(Modal::Palette {
            selected, items, ..
        }) = &self.modal
        else {
            return;
        };
        let Some(item) = items.get(*selected).copied() else {
            self.modal = None;
            return;
        };
        self.modal = None;
        match item.id {
            "save" => self.save_note(),
            "quit" => self.request_quit(false),
            "help" => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            "theme" => {
                self.show_theme = true;
                self.theme_editor = Some(ldnddev_theme::ThemeEditor::new(
                    crate::theme::palette_from_theme(&self.theme),
                    crate::theme::extra_theme_fields(),
                ));
            }
            "preview" => self.toggle_preview(),
            "files" => self.open_finder(crate::app::FinderKind::Files),
            "content" => self.open_finder(crate::app::FinderKind::Content),
            "tags" => self.open_finder(crate::app::FinderKind::Tags),
            "vault" => self.open_picker(),
            "daily" => self.open_daily(),
            "ai" => self.cycle_ai_card(),
            "ai-on" => self.ai_enable(),
            "ai-off" => self.ai_disable(),
            "focus" => self.toggle_focus(),
            "zen" => self.toggle_zen(),
            "explore" => self.toggle_explore(),
            "new" => self.begin_new_entry(),
            "git-status" => self.handle_git(GitOp::Status),
            "git-commit" => self.handle_git(GitOp::Commit),
            "git-pull" => self.handle_git(GitOp::Pull),
            "git-push" => self.handle_git(GitOp::Push),
            other => self.push_toast(ToastLevel::Warning, format!("Unknown command: {other}")),
        }
    }

    pub fn open_daily(&mut self) {
        self.leader = Leader::None;
        let Some(vault) = self.vault.clone() else {
            self.push_toast(ToastLevel::Error, "No vault open");
            return;
        };
        let cfg = dd_vault_core::VaultConfig::load_for(&vault, self.paths.as_ref());
        let ymd = dd_vault_core::today_ymd();
        match dd_vault_core::ensure_daily(&vault.root, &cfg.daily_note_path, &ymd) {
            Ok(rel) => {
                self.reload_tree();
                self.refresh_git();
                self.open_file(rel);
            }
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }
}
