//! Preference pages on the settings sheet.
//!
//! Themes and Help are drawn by the sheet itself. These pages match the desktop
//! sheet, minus fonts: Editor, Window, AI, and Keymap.

use crate::app::{App, FinderKind};

pub const NAV: [&str; 6] = ["Themes", "Editor", "Window", "AI", "Keymap", "Help"];
pub const LAST: u8 = 5;
pub const HELP: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefId {
    Wrap,
    Preview,
    PreviewRead,
    Focus,
    Zen,
    Explore,
    AiOn,
    Provider(&'static str),
    Model,
    Key,
    Files,
    Content,
    Tags,
    Picker,
    Daily,
    Palette,
    AiCard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pref {
    pub title: &'static str,
    pub keys: &'static str,
    pub id: PrefId,
}

pub enum Slot {
    Head(&'static str),
    Row(usize),
}

pub fn items(nav: u8) -> Vec<Pref> {
    match nav {
        1 => vec![Pref {
            title: "Wrap lines",
            keys: "",
            id: PrefId::Wrap,
        }],
        2 => vec![
            Pref {
                title: "Show preview",
                keys: "<Space>p",
                id: PrefId::Preview,
            },
            Pref {
                title: "Focus",
                keys: "<Space>z",
                id: PrefId::Focus,
            },
            Pref {
                title: "Zen",
                keys: "<Space>h",
                id: PrefId::Zen,
            },
            Pref {
                title: "Show tree",
                keys: "<Space>e",
                id: PrefId::Explore,
            },
        ],
        3 => ai_items(),
        4 => keymap_items(),
        _ => Vec::new(),
    }
}

pub fn slots(nav: u8) -> Vec<Slot> {
    let rows = items(nav);
    if nav != 3 {
        return (0..rows.len()).map(Slot::Row).collect();
    }
    let mut out = Vec::new();
    let mut headed = false;
    for (index, row) in rows.iter().enumerate() {
        if matches!(row.id, PrefId::Provider(_)) && !headed {
            out.push(Slot::Head("Provider"));
            headed = true;
        }
        out.push(Slot::Row(index));
    }
    out
}

pub fn value(app: &App, id: PrefId) -> String {
    let on_off = |on: bool| {
        if on {
            "on".to_string()
        } else {
            "off".to_string()
        }
    };
    match id {
        PrefId::Wrap => on_off(app.wrap_notes),
        PrefId::Preview => on_off(app.preview_visible),
        PrefId::Focus => on_off(app.focus),
        PrefId::Zen => on_off(app.zen),
        PrefId::Explore => on_off(app.explore),
        PrefId::AiOn => on_off(app.ai.settings.enabled),
        PrefId::Provider(name) => {
            if app.ai.settings.provider == name {
                "on".to_string()
            } else {
                String::new()
            }
        }
        PrefId::Model => app.ai.settings.resolved_model(),
        PrefId::Key => {
            if app.ai.settings.has_api_key() {
                "saved".to_string()
            } else {
                "not set".to_string()
            }
        }
        PrefId::PreviewRead
        | PrefId::Files
        | PrefId::Content
        | PrefId::Tags
        | PrefId::Picker
        | PrefId::Daily
        | PrefId::Palette
        | PrefId::AiCard => String::new(),
    }
}

pub fn move_row(app: &mut App, delta: isize) {
    let count = items(app.settings_nav).len();
    if count == 0 {
        return;
    }
    let last = count - 1;
    let next = match delta {
        isize::MAX => last,
        isize::MIN => 0,
        step => {
            let cur = app.settings_row.min(last) as isize;
            (cur + step).clamp(0, last as isize) as usize
        }
    };
    app.settings_row = next;
    app.settings_follow = true;
}

/// Run the highlighted preference. A prompt opened here takes the next keys.
pub fn activate(app: &mut App) {
    let rows = items(app.settings_nav);
    let Some(row) = rows.get(app.settings_row) else {
        return;
    };
    let id = row.id;
    let had_modal = app.modal.is_some();
    run(app, id);
    if !had_modal && app.modal.is_some() {
        app.settings_modal = true;
    }
}

fn run(app: &mut App, id: PrefId) {
    match id {
        PrefId::Wrap => {
            app.wrap_notes = !app.wrap_notes;
            app.save_session();
        }
        PrefId::Preview => app.toggle_preview(),
        PrefId::PreviewRead => app.toggle_preview_read(),
        PrefId::Focus => app.toggle_focus(),
        PrefId::Zen => app.toggle_zen(),
        PrefId::Explore => app.toggle_explore(),
        PrefId::AiOn => {
            if app.ai.settings.enabled {
                app.ai_disable();
            } else {
                app.ai_enable();
            }
        }
        PrefId::Provider(name) => app.set_ai_provider(name),
        PrefId::Model => app.prompt_ai_model(),
        PrefId::Key => app.prompt_ai_key(false),
        PrefId::Files => app.open_finder(FinderKind::Files),
        PrefId::Content => app.open_finder(FinderKind::Content),
        PrefId::Tags => app.open_finder(FinderKind::Tags),
        PrefId::Picker => app.open_picker(),
        PrefId::Daily => app.open_daily(),
        PrefId::Palette => app.open_palette(),
        PrefId::AiCard => app.cycle_ai_card(),
    }
}

fn ai_items() -> Vec<Pref> {
    let mut rows = vec![Pref {
        title: "Enabled",
        keys: ":ai on",
        id: PrefId::AiOn,
    }];
    for info in dd_ai::PROVIDERS {
        rows.push(Pref {
            title: info.label,
            keys: "",
            id: PrefId::Provider(info.id),
        });
    }
    rows.push(Pref {
        title: "Model",
        keys: ":ai model",
        id: PrefId::Model,
    });
    rows.push(Pref {
        title: "API key",
        keys: ":ai key",
        id: PrefId::Key,
    });
    rows
}

fn keymap_items() -> Vec<Pref> {
    vec![
        Pref {
            title: "Toggle preview",
            keys: "<Space>p",
            id: PrefId::Preview,
        },
        Pref {
            title: "Read preview",
            keys: "<Space>P",
            id: PrefId::PreviewRead,
        },
        Pref {
            title: "File finder",
            keys: "<Space>ff",
            id: PrefId::Files,
        },
        Pref {
            title: "Content search",
            keys: "<Space>sg",
            id: PrefId::Content,
        },
        Pref {
            title: "Tag search",
            keys: "<Space>tg",
            id: PrefId::Tags,
        },
        Pref {
            title: "Vault picker",
            keys: "<Space>vv",
            id: PrefId::Picker,
        },
        Pref {
            title: "Today's daily note",
            keys: "<Space>nd",
            id: PrefId::Daily,
        },
        Pref {
            title: "Command palette",
            keys: "<Space><Space>",
            id: PrefId::Palette,
        },
        Pref {
            title: "AI card",
            keys: "<Space>ai",
            id: PrefId::AiCard,
        },
        Pref {
            title: "Focus mode",
            keys: "<Space>z",
            id: PrefId::Focus,
        },
        Pref {
            title: "Zen",
            keys: "<Space>h",
            id: PrefId::Zen,
        },
        Pref {
            title: "Show tree",
            keys: "<Space>e",
            id: PrefId::Explore,
        },
    ]
}
