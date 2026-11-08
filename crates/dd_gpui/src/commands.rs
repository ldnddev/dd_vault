//! Space chords, the command palette, and name rules shared with the terminal app.
//!
//! An unknown palette id reports "Not in this window yet".

use std::path::PathBuf;

use dd_edit::Editor;
use dd_vault_core::{Jump, validate_entry_name};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Leader {
    #[default]
    None,
    Space,
    Vault,
    Files,
    Search,
    Tags,
    Daily,
    Ai,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    Picker,
    Files,
    Content,
    Tags,
    Daily,
    Palette,
    Preview,
    PreviewRead,
    Ai,
    Focus,
    Zen,
    Explore,
}

/// One step of a Space chord. Esc and any unknown key cancel.
pub fn leader_step(leader: Leader, key: &str, key_char: Option<&str>) -> (Leader, Option<Chord>) {
    if leader == Leader::None {
        return (Leader::None, None);
    }
    if key == "escape" {
        return (Leader::None, None);
    }
    let ch = chord_char(key, key_char);
    match leader {
        Leader::None => (Leader::None, None),
        Leader::Space => match ch {
            Some(' ') => (Leader::None, Some(Chord::Palette)),
            Some('v') => (Leader::Vault, None),
            Some('f') => (Leader::Files, None),
            Some('s') => (Leader::Search, None),
            Some('t') => (Leader::Tags, None),
            Some('n') => (Leader::Daily, None),
            Some('a') => (Leader::Ai, None),
            Some('p') => (Leader::None, Some(Chord::Preview)),
            Some('P') => (Leader::None, Some(Chord::PreviewRead)),
            Some('z') => (Leader::None, Some(Chord::Focus)),
            Some('h') => (Leader::None, Some(Chord::Zen)),
            Some('e') => (Leader::None, Some(Chord::Explore)),
            _ => (Leader::None, None),
        },
        Leader::Vault => match ch {
            Some('v') => (Leader::None, Some(Chord::Picker)),
            _ => (Leader::None, None),
        },
        Leader::Files => match ch {
            Some('f') => (Leader::None, Some(Chord::Files)),
            _ => (Leader::None, None),
        },
        Leader::Search => match ch {
            Some('g') => (Leader::None, Some(Chord::Content)),
            _ => (Leader::None, None),
        },
        Leader::Tags => match ch {
            Some('g') => (Leader::None, Some(Chord::Tags)),
            _ => (Leader::None, None),
        },
        Leader::Daily => match ch {
            Some('d') => (Leader::None, Some(Chord::Daily)),
            _ => (Leader::None, None),
        },
        Leader::Ai => match ch {
            Some('i') => (Leader::None, Some(Chord::Ai)),
            _ => (Leader::None, None),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static str,
}

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
        id: "jump-back",
        label: "Jump back",
        keys: "Ctrl+O",
    },
    PaletteItem {
        id: "jump-forward",
        label: "Jump forward",
        keys: "Ctrl+I",
    },
    PaletteItem {
        id: "follow",
        label: "Follow link under caret",
        keys: "gf  Enter",
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
        id: "ai-openrouter",
        label: "Use OpenRouter",
        keys: ":ai provider openrouter",
    },
    PaletteItem {
        id: "ai-ollama",
        label: "Use local Ollama",
        keys: ":ai provider ollama",
    },
    PaletteItem {
        id: "ai-spacexai",
        label: "Use SpaceXAI",
        keys: ":ai provider spacexai",
    },
    PaletteItem {
        id: "ai-key",
        label: "Set AI API key",
        keys: ":ai key",
    },
    PaletteItem {
        id: "ai-model",
        label: "Set AI model",
        keys: ":ai model",
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
        keys: ":git push  (commit if dirty, then push)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Save,
    Quit,
    Files,
    Content,
    Tags,
    Vault,
    Daily,
    JumpBack,
    JumpForward,
    Follow,
    New,
    Preview,
    Ai,
    AiOn,
    AiOff,
    AiOpenRouter,
    AiOllama,
    AiSpaceXai,
    AiKey,
    AiModel,
    GitStatus,
    GitCommit,
    GitPull,
    GitPush,
    Help,
    Theme,
    Focus,
    Zen,
    Explore,
    Later,
}

pub fn palette_action(id: &str) -> PaletteAction {
    match id {
        "save" => PaletteAction::Save,
        "quit" => PaletteAction::Quit,
        "files" => PaletteAction::Files,
        "content" => PaletteAction::Content,
        "tags" => PaletteAction::Tags,
        "vault" => PaletteAction::Vault,
        "daily" => PaletteAction::Daily,
        "jump-back" => PaletteAction::JumpBack,
        "jump-forward" => PaletteAction::JumpForward,
        "follow" => PaletteAction::Follow,
        "new" => PaletteAction::New,
        "preview" => PaletteAction::Preview,
        "ai" => PaletteAction::Ai,
        "ai-on" => PaletteAction::AiOn,
        "ai-off" => PaletteAction::AiOff,
        "ai-openrouter" => PaletteAction::AiOpenRouter,
        "ai-ollama" => PaletteAction::AiOllama,
        "ai-spacexai" => PaletteAction::AiSpaceXai,
        "ai-key" => PaletteAction::AiKey,
        "ai-model" => PaletteAction::AiModel,
        "git-status" => PaletteAction::GitStatus,
        "git-commit" => PaletteAction::GitCommit,
        "git-pull" => PaletteAction::GitPull,
        "git-push" => PaletteAction::GitPush,
        "help" => PaletteAction::Help,
        "theme" => PaletteAction::Theme,
        "focus" => PaletteAction::Focus,
        "zen" => PaletteAction::Zen,
        "explore" => PaletteAction::Explore,
        _ => PaletteAction::Later,
    }
}

/// Label prefix ranks above an id match, which ranks above a key-hint match.
pub fn filter_palette(query: &str) -> Vec<PaletteItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return PALETTE_ITEMS.to_vec();
    }
    let mut hits: Vec<(usize, PaletteItem)> = PALETTE_ITEMS
        .iter()
        .filter_map(|item| {
            let blob = format!("{} {} {}", item.label, item.keys, item.id).to_lowercase();
            if !blob.contains(&q) {
                return None;
            }
            let rank = if item.label.to_lowercase().starts_with(&q) {
                0
            } else if item.id.contains(&q) {
                1
            } else {
                2
            };
            Some((rank, *item))
        })
        .collect();
    hits.sort_by_key(|(rank, item)| (*rank, item.label));
    hits.into_iter().map(|(_, item)| item).collect()
}

/// Empty is an error. A trailing slash is a directory. A name with no dot becomes `.md`.
pub fn parse_new_name(raw: &str) -> Result<(String, bool), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Name is empty".into());
    }
    let is_dir = trimmed.ends_with('/');
    let name = trimmed.trim_end_matches('/').trim();
    validate_entry_name(name).map_err(|e| e.to_string())?;
    if is_dir {
        Ok((name.to_string(), true))
    } else if name.contains('.') {
        Ok((name.to_string(), false))
    } else {
        Ok((format!("{name}.md"), false))
    }
}

pub fn looks_like_image(name: &str) -> bool {
    dd_render::looks_like_image(name)
}

pub fn jump_to_heading(editor: &mut Editor, heading: &str) {
    let want = heading.trim();
    for (i, line) in editor.text().lines().enumerate() {
        let t = line.trim();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) {
            let text = t[hashes..].trim();
            if text.eq_ignore_ascii_case(want) {
                editor.go_line(i + 1);
                return;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpAction {
    Record,
    Back,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigateTo {
    pub rel: PathBuf,
    pub heading: Option<String>,
    pub cursor: Option<usize>,
    pub scroll: Option<usize>,
    pub scroll_off: Option<usize>,
    pub jump: JumpAction,
}

impl NavigateTo {
    pub fn file(rel: PathBuf) -> Self {
        Self {
            rel,
            heading: None,
            cursor: None,
            scroll: None,
            scroll_off: None,
            jump: JumpAction::Record,
        }
    }

    pub fn from_jump(loc: Jump, action: JumpAction) -> Self {
        Self {
            rel: loc.rel,
            heading: None,
            cursor: Some(loc.cursor),
            scroll: Some(loc.scroll),
            scroll_off: Some(loc.scroll_off),
            jump: action,
        }
    }
}

fn chord_char(key: &str, key_char: Option<&str>) -> Option<char> {
    if key == "space" {
        return Some(' ');
    }
    let text = key_char.filter(|text| !text.is_empty()).unwrap_or(key);
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_chords_match_the_terminal_leader() {
        assert_eq!(leader_step(Leader::Space, "f", Some("f")).0, Leader::Files);
        assert_eq!(
            leader_step(Leader::Files, "f", Some("f")),
            (Leader::None, Some(Chord::Files))
        );
        assert_eq!(
            leader_step(Leader::Space, "space", None),
            (Leader::None, Some(Chord::Palette))
        );
        assert_eq!(
            leader_step(Leader::Space, "p", Some("p")),
            (Leader::None, Some(Chord::Preview))
        );
        assert_eq!(
            leader_step(Leader::Space, "p", Some("P")),
            (Leader::None, Some(Chord::PreviewRead))
        );
        assert_eq!(
            leader_step(Leader::Space, "P", None),
            (Leader::None, Some(Chord::PreviewRead))
        );
        assert_eq!(
            leader_step(Leader::Space, "z", Some("z")).1,
            Some(Chord::Focus)
        );
        assert_eq!(
            leader_step(Leader::Space, "h", Some("h")).1,
            Some(Chord::Zen)
        );
        assert_eq!(
            leader_step(Leader::Space, "e", Some("e")).1,
            Some(Chord::Explore)
        );
        assert_eq!(leader_step(Leader::Space, "a", Some("a")).0, Leader::Ai);
        assert_eq!(
            leader_step(Leader::Ai, "i", Some("i")),
            (Leader::None, Some(Chord::Ai))
        );
        assert_eq!(
            leader_step(Leader::Ai, "x", Some("x")),
            (Leader::None, None)
        );
        assert_eq!(
            leader_step(Leader::Space, "escape", None),
            (Leader::None, None)
        );
        assert_eq!(
            leader_step(Leader::Vault, "v", Some("v")),
            (Leader::None, Some(Chord::Picker))
        );
        assert_eq!(
            leader_step(Leader::Search, "g", Some("g")),
            (Leader::None, Some(Chord::Content))
        );
        assert_eq!(
            leader_step(Leader::Tags, "g", Some("g")),
            (Leader::None, Some(Chord::Tags))
        );
        assert_eq!(
            leader_step(Leader::Daily, "d", Some("d")),
            (Leader::None, Some(Chord::Daily))
        );
        assert_eq!(
            leader_step(Leader::Space, "q", Some("q")),
            (Leader::None, None)
        );
    }

    #[test]
    fn palette_ranks_label_then_id() {
        let all = filter_palette("");
        assert!(all.iter().any(|item| item.id == "help"));
        assert!(all.iter().any(|item| item.id == "git-push"));
        let files = filter_palette("file");
        assert_eq!(files[0].id, "files");
        let by_id = filter_palette("ai-key");
        assert_eq!(by_id[0].id, "ai-key");
        assert!(filter_palette("zzzz-nope").is_empty());
        assert_eq!(palette_action("follow"), PaletteAction::Follow);
        assert_eq!(palette_action("zen"), PaletteAction::Zen);
        assert_eq!(palette_action("focus"), PaletteAction::Focus);
        assert_eq!(palette_action("explore"), PaletteAction::Explore);
        assert_eq!(palette_action("help"), PaletteAction::Help);
        assert_eq!(palette_action("theme"), PaletteAction::Theme);
        assert_eq!(palette_action("missing"), PaletteAction::Later);
        assert_eq!(palette_action("preview"), PaletteAction::Preview);
        assert_eq!(palette_action("ai"), PaletteAction::Ai);
        assert_eq!(palette_action("git-push"), PaletteAction::GitPush);
        assert_eq!(palette_action("git-status"), PaletteAction::GitStatus);
    }

    #[test]
    fn new_names_follow_the_tree_rules() {
        assert!(parse_new_name("  ").is_err());
        assert_eq!(parse_new_name("notes/").unwrap(), ("notes".into(), true));
        assert_eq!(parse_new_name("Hello").unwrap(), ("Hello.md".into(), false));
        assert_eq!(parse_new_name("a.md").unwrap(), ("a.md".into(), false));
        assert!(parse_new_name("..").is_err());
        assert!(parse_new_name("a/b").is_err());
    }

    #[test]
    fn image_suffixes_ignore_alias_and_heading() {
        assert!(looks_like_image("plot.png"));
        assert!(looks_like_image("plot.png|the plot"));
        assert!(looks_like_image("plot.JPEG#fig"));
        assert!(!looks_like_image("plot.md"));
        assert!(!looks_like_image("png"));
    }

    #[test]
    fn heading_jump_matches_ignoring_case() {
        let dir = std::env::temp_dir().join(format!("dd-gui-head-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("n.md");
        std::fs::write(&path, "# Intro\n\nbody\n\n## Notes\n").unwrap();
        let mut editor = Editor::open(path, "n.md".into()).unwrap();
        jump_to_heading(&mut editor, "notes");
        assert_eq!(editor.cursor_line_col().0, 4);
        jump_to_heading(&mut editor, "missing");
        assert_eq!(editor.cursor_line_col().0, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
