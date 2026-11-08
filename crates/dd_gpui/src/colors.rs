//! Theme tokens from `dd_vault_theme.yml`, packed for GPUI.

use std::path::Path;

use ldnddev_theme::{
    OmarchyPaths, Palette, ParseMode, Rgb, ThemeSource, apply_omarchy, default_config_home,
    global_theme_path, load_from_file, load_from_str, local_theme_path,
};

pub const THEME_FILENAME: &str = "dd_vault_theme.yml";

const BUILTIN_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../dd_vault_theme.yml"
));

#[derive(Clone, Debug)]
pub struct GuiTheme {
    pub base_background: u32,
    pub body_background: u32,
    pub modal_background: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_disabled: u32,
    pub text_inverse: u32,
    pub text_labels: u32,
    pub text_active_focus: u32,
    pub modal_text: u32,
    /// Modal titles. The shell paints the discard prompt with this token.
    pub modal_header: u32,
    pub selected_background: u32,
    pub border_default: u32,
    pub border_active: u32,
    pub cursor: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    /// Callout and diagram color. Token `info`, else a blue.
    pub info: u32,
    pub folders: u32,
    pub files: u32,
    pub links: u32,
    pub quote: String,
}

pub fn pack_rgb(color: Rgb) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

#[derive(Clone, Debug)]
pub struct ThemeLoad {
    pub theme: GuiTheme,
    pub palette: Palette,
    pub warning: Option<String>,
}

pub fn load_theme() -> ThemeLoad {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    load_theme_from(
        &cwd,
        default_config_home().as_deref(),
        &OmarchyPaths::system(),
    )
}

pub fn load_theme_from(
    project_root: &Path,
    config_home: Option<&Path>,
    omarchy: &OmarchyPaths,
) -> ThemeLoad {
    let mut warning = None;
    let local = local_theme_path(project_root, THEME_FILENAME);
    if local.exists() {
        match load_from_file(&local, ParseMode::Strict, ThemeSource::Local) {
            Ok(palette) => return finish(palette, omarchy, warning),
            Err(err) => {
                warning = Some(format!(
                    "theme '{}' failed ({err}); falling back",
                    local.display()
                ));
            }
        }
    }
    if let Some(home) = config_home {
        let global = global_theme_path(home, THEME_FILENAME);
        if global.exists() {
            match load_from_file(&global, ParseMode::Strict, ThemeSource::Global) {
                Ok(palette) => return finish(palette, omarchy, warning),
                Err(err) => {
                    warning = Some(format!(
                        "theme '{}' failed ({err}); using built-in defaults",
                        global.display()
                    ));
                }
            }
        }
    }
    let palette = load_from_str(BUILTIN_YAML, ParseMode::Lenient, ThemeSource::Default)
        .unwrap_or_else(|_| Palette::builtin());
    ThemeLoad {
        theme: from_palette(palette.clone()),
        palette,
        warning,
    }
}

fn finish(palette: Palette, omarchy: &OmarchyPaths, warning: Option<String>) -> ThemeLoad {
    match apply_omarchy(&palette, omarchy) {
        Ok(resolved) => ThemeLoad {
            theme: from_palette(resolved.clone()),
            palette: resolved,
            warning,
        },
        Err(err) => {
            let palette = Palette::builtin();
            ThemeLoad {
                theme: from_palette(palette.clone()),
                palette,
                warning: Some(format!(
                    "omarchy theme failed ({err}); using built-in defaults"
                )),
            }
        }
    }
}

pub fn from_palette(palette: Palette) -> GuiTheme {
    let base_background = token(&palette, "base_background", 0x0F_11_14);
    let text_primary = token(&palette, "text_primary", 0xF5_F6_F7);
    let border_active = token(&palette, "border_active", 0x64_B4_F5);
    GuiTheme {
        base_background,
        body_background: token(&palette, "body_background", 0x2A_2D_31),
        modal_background: token(&palette, "modal_background", 0x1C_1E_21),
        text_primary,
        text_secondary: token(&palette, "text_secondary", 0x9E_A3_AA),
        text_disabled: token(&palette, "text_disabled", 0xA0_A4_A8),
        text_inverse: token(&palette, "text_inverse", 0xF9_FA_FB),
        text_labels: token(&palette, "text_labels", 0xFF_AF_46),
        text_active_focus: token(&palette, "text_active_focus", border_active),
        modal_text: token(&palette, "modal_text", text_primary),
        modal_header: token(&palette, "modal_header", border_active),
        selected_background: token(&palette, "selected_background", base_background),
        border_default: token(&palette, "border_default", text_primary),
        border_active,
        cursor: token(&palette, "cursor", border_active),
        success: token(&palette, "success", 0x82_E0_AA),
        warning: token(&palette, "warning", 0xF5_C4_69),
        error: token(&palette, "error", 0xE5_73_73),
        info: token(&palette, "info", 0x5D_AD_E2),
        folders: token(&palette, "folders", border_active),
        files: token(&palette, "files", 0xFF_AF_46),
        links: token(&palette, "links", 0xFF_A0_87),
        quote: palette
            .header_quotes
            .first()
            .cloned()
            .unwrap_or_else(|| "dd_vault".to_string()),
    }
}

fn token(palette: &Palette, key: &str, fallback: u32) -> u32 {
    palette.get(key).map(pack_rgb).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_colors_match_the_canonical_dark_tokens() {
        let palette = Palette::builtin();
        let background = palette.get("base_background").expect("background");
        let foreground = palette.get("text_primary").expect("foreground");
        assert_eq!(pack_rgb(background), 0x0F_11_14);
        assert_eq!(pack_rgb(foreground), 0xF5_F6_F7);
    }

    #[test]
    fn from_palette_packs_those_tokens() {
        let theme = from_palette(Palette::builtin());
        assert_eq!(theme.base_background, 0x0F_11_14);
        assert_eq!(theme.text_primary, 0xF5_F6_F7);
    }
}
