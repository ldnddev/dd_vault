//! Settings rows for the desktop sheet.
//!
//! Preferences write the files the commands already use. The keymap lists the
//! Space chords. Save, git, and the other palette commands stay in the palette.
//! F1 opens the sheet. The left list picks one section. Help is that list's last page.

use crate::commands::Chord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Editor,
    Window,
    Ai,
    Keymap,
}

impl SettingsPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Themes",
            Self::Editor => "Editor",
            Self::Window => "Window",
            Self::Ai => "AI",
            Self::Keymap => "Keymap",
        }
    }
}

/// Which face a Themes font row opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSlot {
    Editor,
    Preview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingId {
    Themes,
    Fonts(FontSlot),
    Colors,
    Wrap,
    Preview,
    FocusMode,
    Zen,
    Explore,
    AiEnabled,
    AiProvider(&'static str),
    AiModel,
    AiKey,
    Chord(Chord),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingRow {
    pub id: SettingId,
    pub page: SettingsPage,
    pub title: &'static str,
    pub detail: &'static str,
    pub keys: &'static str,
}

const FIND: &[SettingRow] = &[
    SettingRow {
        id: SettingId::Themes,
        page: SettingsPage::Appearance,
        title: "Theme",
        detail: "Highlighting paints the window. y saves the theme file. Esc reverts.",
        keys: "F1",
    },
    SettingRow {
        id: SettingId::Fonts(FontSlot::Editor),
        page: SettingsPage::Appearance,
        title: "Editor font",
        detail: "Monospace face for notes, the tree, and chrome.",
        keys: "",
    },
    SettingRow {
        id: SettingId::Fonts(FontSlot::Preview),
        page: SettingsPage::Appearance,
        title: "Preview font",
        detail: "Proportional face for preview prose.",
        keys: "",
    },
    SettingRow {
        id: SettingId::Colors,
        page: SettingsPage::Appearance,
        title: "Edit colors",
        detail: "Hex overrides on the applied theme. R resets to the built-in palette.",
        keys: "e",
    },
];

const EDITOR: &[SettingRow] = &[SettingRow {
    id: SettingId::Wrap,
    page: SettingsPage::Editor,
    title: "Wrap lines",
    detail: "Soft-wrap notes on word boundaries. Saved with this vault.",
    keys: "",
}];

const WINDOW: &[SettingRow] = &[
    SettingRow {
        id: SettingId::Preview,
        page: SettingsPage::Window,
        title: "Show preview",
        detail: "Rendered note under the editor. Saved with this vault.",
        keys: "<Space>p",
    },
    SettingRow {
        id: SettingId::FocusMode,
        page: SettingsPage::Window,
        title: "Focus",
        detail: "Hide the tree and keep the preview. Saved with this vault.",
        keys: "<Space>z",
    },
    SettingRow {
        id: SettingId::Zen,
        page: SettingsPage::Window,
        title: "Zen",
        detail: "Editor only. Header and footer stay. Saved with this vault.",
        keys: "<Space>h",
    },
    SettingRow {
        id: SettingId::Explore,
        page: SettingsPage::Window,
        title: "Show tree",
        detail: "Forces the folder list on, including in focus and zen.",
        keys: "<Space>e",
    },
];

const AI_TAIL: &[SettingRow] = &[
    SettingRow {
        id: SettingId::AiEnabled,
        page: SettingsPage::Ai,
        title: "Enabled",
        detail: "Each request still asks before it is sent. Saved in ~/.config/ldnddev/config.toml.",
        keys: ":ai on",
    },
    SettingRow {
        id: SettingId::AiModel,
        page: SettingsPage::Ai,
        title: "Model",
        detail: "Chat model id for the current provider.",
        keys: ":ai model",
    },
    SettingRow {
        id: SettingId::AiKey,
        page: SettingsPage::Ai,
        title: "API key",
        detail: "Stored in ~/.config/ldnddev/ai.keys (mode 0600).",
        keys: ":ai key",
    },
];

const KEYMAP: &[SettingRow] = &[
    SettingRow {
        id: SettingId::Chord(Chord::Preview),
        page: SettingsPage::Keymap,
        title: "Toggle preview",
        detail: "",
        keys: "<Space>p",
    },
    SettingRow {
        id: SettingId::Chord(Chord::PreviewRead),
        page: SettingsPage::Keymap,
        title: "Read preview",
        detail: "",
        keys: "<Space>P",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Files),
        page: SettingsPage::Keymap,
        title: "File finder",
        detail: "",
        keys: "<Space>ff",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Content),
        page: SettingsPage::Keymap,
        title: "Content search",
        detail: "",
        keys: "<Space>sg",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Tags),
        page: SettingsPage::Keymap,
        title: "Tag search",
        detail: "",
        keys: "<Space>tg",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Picker),
        page: SettingsPage::Keymap,
        title: "Vault picker",
        detail: "",
        keys: "<Space>vv",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Daily),
        page: SettingsPage::Keymap,
        title: "Today's daily note",
        detail: "",
        keys: "<Space>nd",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Palette),
        page: SettingsPage::Keymap,
        title: "Command palette",
        detail: "",
        keys: "<Space><Space>",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Ai),
        page: SettingsPage::Keymap,
        title: "AI card",
        detail: "",
        keys: "<Space>ai",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Focus),
        page: SettingsPage::Keymap,
        title: "Focus mode",
        detail: "",
        keys: "<Space>z",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Zen),
        page: SettingsPage::Keymap,
        title: "Zen",
        detail: "",
        keys: "<Space>h",
    },
    SettingRow {
        id: SettingId::Chord(Chord::Explore),
        page: SettingsPage::Keymap,
        title: "Show tree",
        detail: "",
        keys: "<Space>e",
    },
];

fn provider_rows() -> Vec<SettingRow> {
    dd_ai::PROVIDERS
        .iter()
        .map(|info| SettingRow {
            id: SettingId::AiProvider(info.id),
            page: SettingsPage::Ai,
            title: info.label,
            detail: "",
            keys: "",
        })
        .collect()
}

pub fn all_rows() -> Vec<SettingRow> {
    let mut rows = Vec::new();
    rows.extend_from_slice(FIND);
    rows.extend_from_slice(EDITOR);
    rows.extend_from_slice(WINDOW);
    rows.push(AI_TAIL[0]);
    rows.extend(provider_rows());
    rows.extend_from_slice(&AI_TAIL[1..]);
    rows.extend_from_slice(KEYMAP);
    rows
}

pub fn matching_rows(query: &str) -> Vec<SettingRow> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(usize, SettingRow)> = all_rows()
        .into_iter()
        .filter_map(|row| {
            let blob = format!(
                "{} {} {} {}",
                row.title,
                row.detail,
                row.keys,
                row.page.label()
            )
            .to_lowercase();
            if !blob.contains(&q) {
                return None;
            }
            let rank = if row.title.to_lowercase().starts_with(&q) {
                0
            } else if row.keys.to_lowercase().contains(&q) {
                1
            } else {
                2
            };
            Some((rank, row))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.title.cmp(b.1.title)));
    hits.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_themes_editor_window_ai_and_keymap() {
        assert_eq!(
            [
                SettingsPage::Appearance.label(),
                SettingsPage::Editor.label(),
                SettingsPage::Window.label(),
                SettingsPage::Ai.label(),
                SettingsPage::Keymap.label(),
            ],
            ["Themes", "Editor", "Window", "AI", "Keymap"]
        );
    }

    #[test]
    fn keymap_lists_every_space_chord() {
        let rows: Vec<_> = all_rows()
            .into_iter()
            .filter(|row| row.page == SettingsPage::Keymap)
            .collect();
        let keys: Vec<&str> = rows.iter().map(|row| row.keys).collect();
        assert!(!keys.contains(&"F2"));
        assert!(rows.iter().all(|row| row.id != SettingId::Themes));
        for label in [
            "<Space>p",
            "<Space>P",
            "<Space>ff",
            "<Space>sg",
            "<Space>tg",
            "<Space>vv",
            "<Space>nd",
            "<Space><Space>",
            "<Space>ai",
            "<Space>z",
            "<Space>h",
            "<Space>e",
        ] {
            assert!(keys.contains(&label), "{label}");
        }
        let chords: Vec<Chord> = rows
            .iter()
            .filter_map(|row| match row.id {
                SettingId::Chord(chord) => Some(chord),
                _ => None,
            })
            .collect();
        for chord in [
            Chord::Picker,
            Chord::Files,
            Chord::Content,
            Chord::Tags,
            Chord::Daily,
            Chord::Palette,
            Chord::Preview,
            Chord::PreviewRead,
            Chord::Ai,
            Chord::Focus,
            Chord::Zen,
            Chord::Explore,
        ] {
            assert!(chords.contains(&chord));
        }
    }

    #[test]
    fn preferences_leave_save_and_git_in_the_palette() {
        let blob = all_rows()
            .iter()
            .map(|row| format!("{} {} {}", row.title, row.detail, row.keys))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        assert!(!blob.contains("git"));
        assert!(!blob.contains(":w"));
        assert!(!blob.contains("ctrl+s"));
        let appearance: Vec<_> = all_rows()
            .into_iter()
            .filter(|row| row.page == SettingsPage::Appearance)
            .collect();
        assert!(appearance.iter().all(|row| {
            matches!(
                row.id,
                SettingId::Themes | SettingId::Fonts(_) | SettingId::Colors
            )
        }));
    }

    #[test]
    fn search_finds_wrap_and_f1_and_skips_an_empty_query() {
        assert!(matching_rows("").is_empty());
        assert!(matching_rows("   ").is_empty());
        assert!(matching_rows("zzzz-nope").is_empty());
        let wrap = matching_rows("wrap");
        assert_eq!(wrap.len(), 1);
        assert_eq!(wrap[0].id, SettingId::Wrap);
        let f1 = matching_rows("f1");
        assert!(f1.iter().any(|row| row.id == SettingId::Themes));
        assert!(matching_rows("f2").is_empty());
        assert!(
            matching_rows("font")
                .iter()
                .any(|row| matches!(row.id, SettingId::Fonts(FontSlot::Editor)))
        );
        assert!(
            matching_rows("ollama")
                .iter()
                .any(|row| matches!(row.id, SettingId::AiProvider("ollama")))
        );
    }
}
