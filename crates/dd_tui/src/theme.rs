use std::path::{Path, PathBuf};

use ldnddev_theme::{
    load_from_file, load_from_str, ColorField, Palette, ParseMode, ThemeSource, EXTRA_MODAL_HEADER,
    EXTRA_TEXT_DISABLED, EXTRA_TEXT_INVERSE,
};
use ratatui::style::{Color, Style};

pub const THEME_FILENAME: &str = "dd_vault_theme.yml";
const BUILTIN_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../dd_vault_theme.yml"
));

#[derive(Clone, Debug)]
pub struct AppTheme {
    pub base_background: Color,
    pub body_background: Color,
    pub modal_background: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub text_inverse: Color,
    pub text_labels: Color,
    pub text_active_focus: Color,
    pub modal_labels: Color,
    pub modal_text: Color,
    pub modal_header: Color,
    pub selected_background: Color,
    pub border_default: Color,
    pub border_active: Color,
    pub scrollbar: Color,
    pub scrollbar_hover: Color,
    pub input_border_default: Color,
    pub input_border_focus: Color,
    pub input_text_default: Color,
    pub input_text_focus: Color,
    pub cursor: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub folders: Color,
    pub files: Color,
    pub links: Color,
    pub app_shell: Style,
    pub active_border: Style,
    pub header_quotes: Vec<String>,
    pub source: ThemeSource,
    pub version: u64,
}

#[derive(Clone, Debug)]
pub struct ThemeLoad {
    pub theme: AppTheme,
    pub warning: Option<String>,
}

pub fn extra_theme_fields() -> &'static [ColorField] {
    &[EXTRA_MODAL_HEADER, EXTRA_TEXT_DISABLED, EXTRA_TEXT_INVERSE]
}

impl AppTheme {
    pub fn load() -> ThemeLoad {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::load_from(&cwd, ldnddev_theme::default_config_home().as_deref())
    }

    pub fn load_from(project_root: &Path, config_home: Option<&Path>) -> ThemeLoad {
        let mut warning = None;

        let local = ldnddev_theme::local_theme_path(project_root, THEME_FILENAME);
        if local.exists() {
            match load_from_file(&local, ParseMode::Strict, ThemeSource::Local) {
                Ok(palette) => {
                    return ThemeLoad {
                        theme: Self::from_palette(palette, ThemeSource::Local),
                        warning,
                    };
                }
                Err(err) => {
                    warning = Some(format!(
                        "theme '{}' failed ({err}); falling back",
                        local.display()
                    ));
                }
            }
        }

        if let Some(home) = config_home {
            let global = ldnddev_theme::global_theme_path(home, THEME_FILENAME);
            if global.exists() {
                match load_from_file(&global, ParseMode::Strict, ThemeSource::Global) {
                    Ok(palette) => {
                        return ThemeLoad {
                            theme: Self::from_palette(palette, ThemeSource::Global),
                            warning,
                        };
                    }
                    Err(err) => {
                        warning = Some(format!(
                            "theme '{}' failed ({err}); using built-in defaults",
                            global.display()
                        ));
                    }
                }
            }
        }

        ThemeLoad {
            theme: Self::builtin(),
            warning,
        }
    }

    pub fn builtin() -> Self {
        let palette = load_from_str(BUILTIN_YAML, ParseMode::Lenient, ThemeSource::Default)
            .unwrap_or_else(|_| Palette::builtin());
        Self::from_palette(palette, ThemeSource::Default)
    }

    pub fn from_palette(mut palette: Palette, source: ThemeSource) -> Self {
        palette.ensure_extras(extra_theme_fields());
        if palette.header_quotes.is_empty() {
            palette.header_quotes = default_header_quotes();
        }
        palette.source = source;
        let get =
            |key: &str, fallback: Color| palette.get(key).map(rgb_to_color).unwrap_or(fallback);
        let base_background = get("base_background", Color::Rgb(15, 17, 20));
        let text_primary = get("text_primary", Color::Rgb(245, 246, 247));
        let border_active = get("border_active", Color::Rgb(100, 180, 245));
        Self {
            base_background,
            body_background: get("body_background", Color::Rgb(42, 45, 49)),
            modal_background: get("modal_background", Color::Rgb(28, 30, 33)),
            text_primary,
            text_secondary: get("text_secondary", Color::Rgb(158, 163, 170)),
            text_disabled: get("text_disabled", Color::Rgb(160, 164, 168)),
            text_inverse: get("text_inverse", Color::Rgb(249, 250, 251)),
            text_labels: get("text_labels", Color::Rgb(255, 175, 70)),
            text_active_focus: get("text_active_focus", border_active),
            modal_labels: get("modal_labels", border_active),
            modal_text: get("modal_text", text_primary),
            modal_header: get("modal_header", border_active),
            selected_background: get("selected_background", base_background),
            border_default: get("border_default", text_primary),
            border_active,
            scrollbar: get("scrollbar", Color::Rgb(255, 160, 135)),
            scrollbar_hover: get("scrollbar_hover", border_active),
            input_border_default: get("input_border_default", text_primary),
            input_border_focus: get("input_border_focus", border_active),
            input_text_default: get("input_text_default", text_primary),
            input_text_focus: get("input_text_focus", border_active),
            cursor: get("cursor", border_active),
            success: get("success", Color::Rgb(130, 224, 170)),
            warning: get("warning", Color::Rgb(245, 196, 105)),
            error: get("error", Color::Rgb(229, 115, 115)),
            info: get("info", Color::Rgb(93, 173, 226)),
            folders: get("folders", border_active),
            files: get("files", Color::Rgb(255, 175, 70)),
            links: get("links", Color::Rgb(255, 160, 135)),
            app_shell: Style::default().bg(base_background).fg(text_primary),
            active_border: Style::default().fg(border_active),
            header_quotes: palette.header_quotes,
            source,
            version: palette.version,
        }
    }
}

impl Default for AppTheme {
    fn default() -> Self {
        Self::builtin()
    }
}

pub fn rgb_to_color(rgb: ldnddev_theme::Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

fn color_to_rgb(color: Color) -> ldnddev_theme::Rgb {
    match color {
        Color::Rgb(r, g, b) => ldnddev_theme::Rgb { r, g, b },
        _ => ldnddev_theme::Rgb { r: 0, g: 0, b: 0 },
    }
}

pub fn color_to_hex(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
        _ => "?".to_string(),
    }
}

pub fn palette_from_theme(theme: &AppTheme) -> Palette {
    let mut palette = Palette::builtin();
    palette.header_quotes = theme.header_quotes.clone();
    palette.source = theme.source;
    palette.version = theme.version;
    for (key, color) in [
        ("base_background", theme.base_background),
        ("body_background", theme.body_background),
        ("modal_background", theme.modal_background),
        ("text_primary", theme.text_primary),
        ("text_secondary", theme.text_secondary),
        ("text_disabled", theme.text_disabled),
        ("text_inverse", theme.text_inverse),
        ("text_labels", theme.text_labels),
        ("text_active_focus", theme.text_active_focus),
        ("modal_labels", theme.modal_labels),
        ("modal_text", theme.modal_text),
        ("modal_header", theme.modal_header),
        ("selected_background", theme.selected_background),
        ("border_default", theme.border_default),
        ("border_active", theme.border_active),
        ("scrollbar", theme.scrollbar),
        ("scrollbar_hover", theme.scrollbar_hover),
        ("input_border_default", theme.input_border_default),
        ("input_border_focus", theme.input_border_focus),
        ("input_text_default", theme.input_text_default),
        ("input_text_focus", theme.input_text_focus),
        ("cursor", theme.cursor),
        ("success", theme.success),
        ("warning", theme.warning),
        ("error", theme.error),
        ("info", theme.info),
        ("folders", theme.folders),
        ("files", theme.files),
        ("links", theme.links),
    ] {
        palette.set(key, color_to_rgb(color));
    }
    palette
}

pub fn apply_palette(theme: &mut AppTheme, palette: &Palette) {
    *theme = AppTheme::from_palette(palette.clone(), palette.source);
}

pub fn default_header_quotes() -> Vec<String> {
    [
        "Files on disk. Opinions in git.",
        "Wikilinks, not vendor lock-in.",
        "The vault is the folder. The rest is a cache.",
        "hjkl through your second brain.",
        "Preview is a courtesy. Markdown is the truth.",
        "AI stays in the corner until you ask.",
        "Offline first. Sync is a git push.",
        "One note, one file, no surprises.",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub fn choose_header_copy(quotes: &[String]) -> String {
    let fallback = default_header_quotes();
    let list = if quotes.is_empty() { &fallback } else { quotes };
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        ^ u64::from(std::process::id());
    list[(seed as usize) % list.len()].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn builtin_loads_embedded_yaml() {
        let theme = AppTheme::builtin();
        assert_eq!(theme.source, ThemeSource::Default);
        assert_eq!(theme.version, 1);
        assert_eq!(color_to_hex(theme.base_background), "#0F1114");
        assert_eq!(color_to_hex(theme.modal_header), "#64B4F5");
        assert!(!theme.header_quotes.is_empty());
    }

    #[test]
    fn local_theme_wins() {
        let dir = tempfile::tempdir().expect("tmp");
        let yaml = BUILTIN_YAML.replace("#0F1114", "#112233");
        fs::write(dir.path().join(THEME_FILENAME), yaml).expect("write");
        let loaded = AppTheme::load_from(dir.path(), None);
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.theme.source, ThemeSource::Local);
        assert_eq!(color_to_hex(loaded.theme.base_background), "#112233");
    }

    #[test]
    fn bad_version_falls_back_with_warning() {
        let dir = tempfile::tempdir().expect("tmp");
        fs::write(dir.path().join(THEME_FILENAME), "version: 99\ncolors: {}\n").expect("write");
        let loaded = AppTheme::load_from(dir.path(), None);
        assert!(loaded.warning.as_deref().unwrap_or("").contains("failed"));
        assert_eq!(loaded.theme.source, ThemeSource::Default);
    }

    #[test]
    fn global_used_when_local_missing() {
        let project = tempfile::tempdir().expect("project");
        let xdg = tempfile::tempdir().expect("xdg");
        let yaml = BUILTIN_YAML.replace("#0F1114", "#ABCDEF");
        let global_dir = xdg.path().join("ldnddev");
        fs::create_dir_all(&global_dir).expect("mkdir");
        fs::write(global_dir.join(THEME_FILENAME), yaml).expect("write");
        let loaded = AppTheme::load_from(project.path(), Some(xdg.path()));
        assert_eq!(loaded.theme.source, ThemeSource::Global);
        assert_eq!(color_to_hex(loaded.theme.base_background), "#ABCDEF");
    }

    #[test]
    fn missing_local_and_global_is_builtin() {
        let project = tempfile::tempdir().expect("project");
        let xdg = tempfile::tempdir().expect("xdg");
        let loaded = AppTheme::load_from(project.path(), Some(xdg.path()));
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.theme.source, ThemeSource::Default);
        assert_eq!(color_to_hex(loaded.theme.base_background), "#0F1114");
    }
}
