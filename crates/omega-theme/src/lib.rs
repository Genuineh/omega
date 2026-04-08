use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ratatui::style::Color;
use ratatui::widgets::BorderType;
use serde::Deserialize;

pub const DEFAULT_THEME_PATH: &str = ".omega/theme.toml";

const DEFAULT_THEME_TOML: &str = r##"# Default omega-tui theme overrides
theme = "dark"

[input]
border_type = "rounded"
text_fg = "#569cd6"
normal_border_fg = "#a3bbd6"
insert_border_fg = "#4ec9b0"

[context_bar]
label_fg = "#747e8c"
hint_fg = "#acb3bd"

[status_bar]
label_fg = "#747e8c"
divider_fg = "#626b78"
idle_fg = "#7bc78f"
running_fg = "#ffc468"

[surfaces]
text_fg = "#d4d4d4"
focus_border_fg = "#4ec9b0"
border_dim_fg = "#303030"
panel_bg = "#11161d"
sidebar_bg = "#0d1117"
sidebar_rail_bg = "#131922"
section_bg = "#171e28"
title_fg = "#eef2f6"

[messages]
tool_fg = "#dcdcaa"
"##;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmegaTheme {
    pub surfaces: SurfaceTheme,
    pub input: InputTheme,
    pub context_bar: ContextBarTheme,
    pub status_bar: StatusBarTheme,
    pub messages: MessageTheme,
    pub overlay: OverlayTheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTheme {
    pub panel_border_type: BorderType,
    pub text_fg: Color,
    pub muted_text_fg: Color,
    pub border_dim_fg: Color,
    pub focus_border_fg: Color,
    pub panel_bg: Color,
    pub sidebar_bg: Color,
    pub sidebar_rail_bg: Color,
    pub section_bg: Color,
    pub title_fg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputTheme {
    pub border_type: BorderType,
    pub bg: Color,
    pub text_fg: Color,
    pub placeholder_fg: Color,
    pub normal_border_fg: Color,
    pub insert_border_fg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBarTheme {
    pub bg: Color,
    pub label_fg: Color,
    pub hint_fg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarTheme {
    pub bg: Color,
    pub label_fg: Color,
    pub divider_fg: Color,
    pub normal_mode_fg: Color,
    pub insert_mode_fg: Color,
    pub idle_fg: Color,
    pub running_fg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTheme {
    pub user_fg: Color,
    pub agent_fg: Color,
    pub tool_fg: Color,
    pub error_fg: Color,
    pub separator_fg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayTheme {
    pub border_type: BorderType,
    pub bg: Color,
    pub mask_bg: Color,
    pub button_fg: Color,
    pub selected_button_fg: Color,
    pub selected_button_bg: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPalette {
    pub panel_border_type: BorderType,
    pub input_border_type: BorderType,
    pub overlay_border_type: BorderType,
    pub text: Color,
    pub border_dim: Color,
    pub focus_border: Color,
    pub panel_bg: Color,
    pub sidebar_bg: Color,
    pub sidebar_rail_bg: Color,
    pub section_bg: Color,
    pub title_fg: Color,
    pub input_bg: Color,
    pub input_text: Color,
    pub input_placeholder: Color,
    pub context_bar_bg: Color,
    pub context_label: Color,
    pub context_hint: Color,
    pub status_bar_bg: Color,
    pub status_label: Color,
    pub bar_divider: Color,
    pub mode_normal_fg: Color,
    pub mode_insert_fg: Color,
    pub status_idle_fg: Color,
    pub status_running_fg: Color,
    pub user_message: Color,
    pub agent_message: Color,
    pub tool_message: Color,
    pub error_message: Color,
    pub separator_message: Color,
    pub overlay_bg: Color,
    pub overlay_mask_bg: Color,
    pub overlay_button_fg: Color,
    pub overlay_button_selected_fg: Color,
    pub overlay_button_selected_bg: Color,
    // Markdown rendering (15B-40)
    pub heading_1_fg: Color,
    pub heading_2_fg: Color,
    pub heading_3_fg: Color,
    pub inline_code_fg: Color,
    pub inline_code_bg: Color,
    pub hr_fg: Color,
    // Code block (15B-41)
    pub code_block_bg: Color,
    pub code_lang_fg: Color,
    pub code_border_fg: Color,
    // Message badges (15B-42)
    pub user_badge_fg: Color,
    pub assistant_badge_fg: Color,
    pub warning_badge_fg: Color,
    pub error_badge_fg: Color,
    // Final answer (15B-43)
    pub final_answer_accent_fg: Color,
    pub final_answer_border_fg: Color,
    // Thinking (15B-45)
    pub thinking_summary_fg: Color,
    pub thinking_body_fg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedTheme {
    pub theme: OmegaTheme,
    pub source: ThemeSource,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}

impl ThemeSource {
    pub fn source_label(&self) -> String {
        match self {
            Self::BuiltinDefault => "builtin".to_string(),
            Self::File(path) | Self::FileWithFallback(path) => path.display().to_string(),
        }
    }
}

impl LoadedTheme {
    pub fn source_label(&self) -> String {
        self.source.source_label()
    }
}

impl OmegaTheme {
    pub fn dark() -> Self {
        Self {
            surfaces: SurfaceTheme {
                panel_border_type: BorderType::Rounded,
                text_fg: Color::Rgb(219, 229, 238),
                muted_text_fg: Color::Rgb(151, 163, 179),
                border_dim_fg: Color::Rgb(44, 56, 70),
                focus_border_fg: Color::Rgb(113, 210, 194),
                panel_bg: Color::Rgb(17, 22, 29),
                sidebar_bg: Color::Rgb(13, 17, 23),
                sidebar_rail_bg: Color::Rgb(19, 25, 34),
                section_bg: Color::Rgb(23, 30, 40),
                title_fg: Color::Rgb(238, 242, 246),
            },
            input: InputTheme {
                border_type: BorderType::Rounded,
                bg: Color::Rgb(12, 17, 23),
                text_fg: Color::Rgb(138, 180, 248),
                placeholder_fg: Color::Rgb(151, 163, 179),
                normal_border_fg: Color::Rgb(86, 102, 120),
                insert_border_fg: Color::Rgb(113, 210, 194),
                cursor_fg: Color::Reset,
                cursor_bg: Color::Rgb(138, 180, 248),
            },
            context_bar: ContextBarTheme {
                bg: Color::Rgb(12, 17, 23),
                label_fg: Color::Rgb(110, 124, 143),
                hint_fg: Color::Rgb(182, 190, 199),
            },
            status_bar: StatusBarTheme {
                bg: Color::Rgb(12, 17, 23),
                label_fg: Color::Rgb(110, 124, 143),
                divider_fg: Color::Rgb(70, 84, 101),
                normal_mode_fg: Color::Rgb(138, 180, 248),
                insert_mode_fg: Color::Rgb(113, 210, 194),
                idle_fg: Color::Rgb(128, 201, 149),
                running_fg: Color::Rgb(243, 196, 110),
            },
            messages: MessageTheme {
                user_fg: Color::Rgb(128, 201, 149),
                agent_fg: Color::Rgb(219, 229, 238),
                tool_fg: Color::Rgb(229, 215, 141),
                error_fg: Color::Rgb(255, 113, 113),
                separator_fg: Color::Rgb(44, 56, 70),
            },
            overlay: OverlayTheme {
                border_type: BorderType::Rounded,
                bg: Color::Rgb(23, 30, 40),
                mask_bg: Color::Rgb(6, 8, 12),
                button_fg: Color::Rgb(219, 229, 238),
                selected_button_fg: Color::Rgb(12, 17, 23),
                selected_button_bg: Color::Rgb(113, 210, 194),
            },
        }
    }

    pub fn default_theme_toml() -> &'static str {
        DEFAULT_THEME_TOML
    }

    pub fn render_palette(&self) -> RenderPalette {
        RenderPalette {
            panel_border_type: self.surfaces.panel_border_type,
            input_border_type: self.input.border_type,
            overlay_border_type: self.overlay.border_type,
            text: self.surfaces.text_fg,
            border_dim: self.surfaces.border_dim_fg,
            focus_border: self.surfaces.focus_border_fg,
            panel_bg: self.surfaces.panel_bg,
            sidebar_bg: self.surfaces.sidebar_bg,
            sidebar_rail_bg: self.surfaces.sidebar_rail_bg,
            section_bg: self.surfaces.section_bg,
            title_fg: self.surfaces.title_fg,
            input_bg: self.input.bg,
            input_text: self.input.text_fg,
            input_placeholder: self.input.placeholder_fg,
            context_bar_bg: self.context_bar.bg,
            context_label: self.context_bar.label_fg,
            context_hint: self.context_bar.hint_fg,
            status_bar_bg: self.status_bar.bg,
            status_label: self.status_bar.label_fg,
            bar_divider: self.status_bar.divider_fg,
            mode_normal_fg: self.status_bar.normal_mode_fg,
            mode_insert_fg: self.status_bar.insert_mode_fg,
            status_idle_fg: self.status_bar.idle_fg,
            status_running_fg: self.status_bar.running_fg,
            user_message: self.messages.user_fg,
            agent_message: self.messages.agent_fg,
            tool_message: self.messages.tool_fg,
            error_message: self.messages.error_fg,
            separator_message: self.messages.separator_fg,
            overlay_bg: self.overlay.bg,
            overlay_mask_bg: self.overlay.mask_bg,
            overlay_button_fg: self.overlay.button_fg,
            overlay_button_selected_fg: self.overlay.selected_button_fg,
            overlay_button_selected_bg: self.overlay.selected_button_bg,
            // Markdown rendering
            heading_1_fg: Color::Rgb(86, 156, 214),   // bright blue
            heading_2_fg: Color::Rgb(78, 201, 176),   // teal
            heading_3_fg: Color::Rgb(172, 179, 189),  // muted
            inline_code_fg: Color::Rgb(206, 145, 120),// orange-ish
            inline_code_bg: Color::Rgb(40, 40, 48),   // subtle dark
            hr_fg: Color::Rgb(98, 107, 120),           // dim
            // Code block
            code_block_bg: Color::Rgb(30, 30, 46),    // dark surface
            code_lang_fg: Color::Rgb(116, 126, 140),  // muted label
            code_border_fg: Color::Rgb(68, 71, 90),   // dim border
            // Message badges
            user_badge_fg: Color::Rgb(80, 250, 123),   // green
            assistant_badge_fg: Color::Rgb(139, 148, 158),// muted
            warning_badge_fg: Color::Rgb(255, 196, 104), // amber
            error_badge_fg: Color::Rgb(255, 85, 85),     // red
            // Final answer
            final_answer_accent_fg: Color::Rgb(80, 250, 123), // bright green
            final_answer_border_fg: Color::Rgb(68, 71, 90),   // dim
            // Thinking
            thinking_summary_fg: Color::Rgb(98, 114, 164),    // muted blue
            thinking_body_fg: Color::Rgb(68, 71, 90),         // very dim
        }
    }

    pub fn load(root: &Path) -> LoadedTheme {
        let path = root.join(DEFAULT_THEME_PATH);
        if !path.exists() {
            match Self::write_default_file(&path) {
                Ok(()) => {
                    return match Self::load_from_file(&path) {
                        Ok(theme) => LoadedTheme {
                            theme,
                            source: ThemeSource::File(path),
                            warnings: Vec::new(),
                        },
                        Err(error) => LoadedTheme {
                            theme: Self::dark(),
                            source: ThemeSource::FileWithFallback(path.clone()),
                            warnings: vec![format!(
                                "Default theme file at {} was created but failed to load: {error}. Falling back to built-in defaults.",
                                path.display()
                            )],
                        },
                    };
                }
                Err(error) => {
                    return LoadedTheme {
                        theme: Self::dark(),
                        source: ThemeSource::BuiltinDefault,
                        warnings: vec![format!(
                            "Failed to create default theme file at {}: {error}. Falling back to built-in defaults.",
                            path.display()
                        )],
                    };
                }
            }
        }

        match Self::load_from_file(&path) {
            Ok(theme) => LoadedTheme {
                theme,
                source: ThemeSource::File(path),
                warnings: Vec::new(),
            },
            Err(error) => LoadedTheme {
                theme: Self::dark(),
                source: ThemeSource::FileWithFallback(path.clone()),
                warnings: vec![format!(
                    "Theme config at {} is invalid: {error}. Falling back to built-in defaults.",
                    path.display()
                )],
            },
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme file {}", path.display()))?;
        let config = toml::from_str::<ThemeConfig>(&raw)
            .with_context(|| format!("failed to parse theme file {}", path.display()))?;

        let mut theme = Self::dark();
        theme
            .apply_config(config)
            .with_context(|| format!("failed to apply theme file {}", path.display()))?;
        Ok(theme)
    }

    fn write_default_file(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create theme config dir {}", parent.display())
            })?;
        }
        fs::write(path, Self::default_theme_toml())
            .with_context(|| format!("failed to write theme file {}", path.display()))
    }

    fn apply_config(&mut self, config: ThemeConfig) -> Result<()> {
        if let Some(theme_name) = config.theme.as_deref() {
            if !theme_name.eq_ignore_ascii_case("dark") {
                bail!("unsupported theme '{theme_name}', only 'dark' is currently available");
            }
        }

        if let Some(surfaces) = config.surfaces {
            apply_border_type(surfaces.border_type, &mut self.surfaces.panel_border_type);
            apply_color(
                "surfaces.text_fg",
                surfaces.text_fg,
                &mut self.surfaces.text_fg,
            )?;
            apply_color(
                "surfaces.muted_text_fg",
                surfaces.muted_text_fg,
                &mut self.surfaces.muted_text_fg,
            )?;
            apply_color(
                "surfaces.border_dim_fg",
                surfaces.border_dim_fg,
                &mut self.surfaces.border_dim_fg,
            )?;
            apply_color(
                "surfaces.focus_border_fg",
                surfaces.focus_border_fg,
                &mut self.surfaces.focus_border_fg,
            )?;
            apply_color("surfaces.panel_bg", surfaces.panel_bg, &mut self.surfaces.panel_bg)?;
            apply_color(
                "surfaces.sidebar_bg",
                surfaces.sidebar_bg,
                &mut self.surfaces.sidebar_bg,
            )?;
            apply_color(
                "surfaces.sidebar_rail_bg",
                surfaces.sidebar_rail_bg,
                &mut self.surfaces.sidebar_rail_bg,
            )?;
            apply_color(
                "surfaces.section_bg",
                surfaces.section_bg,
                &mut self.surfaces.section_bg,
            )?;
            apply_color("surfaces.title_fg", surfaces.title_fg, &mut self.surfaces.title_fg)?;
        }

        if let Some(input) = config.input {
            apply_border_type(input.border_type, &mut self.input.border_type);
            apply_color("input.bg", input.bg, &mut self.input.bg)?;
            apply_color("input.text_fg", input.text_fg, &mut self.input.text_fg)?;
            apply_color(
                "input.placeholder_fg",
                input.placeholder_fg,
                &mut self.input.placeholder_fg,
            )?;
            apply_color(
                "input.normal_border_fg",
                input.normal_border_fg,
                &mut self.input.normal_border_fg,
            )?;
            apply_color(
                "input.insert_border_fg",
                input.insert_border_fg,
                &mut self.input.insert_border_fg,
            )?;
            apply_color(
                "input.cursor_fg",
                input.cursor_fg,
                &mut self.input.cursor_fg,
            )?;
            apply_color(
                "input.cursor_bg",
                input.cursor_bg,
                &mut self.input.cursor_bg,
            )?;
        }

        if let Some(context_bar) = config.context_bar {
            apply_color("context_bar.bg", context_bar.bg, &mut self.context_bar.bg)?;
            apply_color(
                "context_bar.label_fg",
                context_bar.label_fg,
                &mut self.context_bar.label_fg,
            )?;
            apply_color(
                "context_bar.hint_fg",
                context_bar.hint_fg,
                &mut self.context_bar.hint_fg,
            )?;
        }

        if let Some(status_bar) = config.status_bar {
            apply_color("status_bar.bg", status_bar.bg, &mut self.status_bar.bg)?;
            apply_color(
                "status_bar.label_fg",
                status_bar.label_fg,
                &mut self.status_bar.label_fg,
            )?;
            apply_color(
                "status_bar.divider_fg",
                status_bar.divider_fg,
                &mut self.status_bar.divider_fg,
            )?;
            apply_color(
                "status_bar.normal_mode_fg",
                status_bar.normal_mode_fg,
                &mut self.status_bar.normal_mode_fg,
            )?;
            apply_color(
                "status_bar.insert_mode_fg",
                status_bar.insert_mode_fg,
                &mut self.status_bar.insert_mode_fg,
            )?;
            apply_color(
                "status_bar.idle_fg",
                status_bar.idle_fg,
                &mut self.status_bar.idle_fg,
            )?;
            apply_color(
                "status_bar.running_fg",
                status_bar.running_fg,
                &mut self.status_bar.running_fg,
            )?;
        }

        if let Some(messages) = config.messages {
            apply_color(
                "messages.user_fg",
                messages.user_fg,
                &mut self.messages.user_fg,
            )?;
            apply_color(
                "messages.agent_fg",
                messages.agent_fg,
                &mut self.messages.agent_fg,
            )?;
            apply_color(
                "messages.tool_fg",
                messages.tool_fg,
                &mut self.messages.tool_fg,
            )?;
            apply_color(
                "messages.error_fg",
                messages.error_fg,
                &mut self.messages.error_fg,
            )?;
            apply_color(
                "messages.separator_fg",
                messages.separator_fg,
                &mut self.messages.separator_fg,
            )?;
        }

        if let Some(overlay) = config.overlay {
            apply_border_type(overlay.border_type, &mut self.overlay.border_type);
            apply_color("overlay.bg", overlay.bg, &mut self.overlay.bg)?;
            apply_color(
                "overlay.mask_bg",
                overlay.mask_bg,
                &mut self.overlay.mask_bg,
            )?;
            apply_color(
                "overlay.button_fg",
                overlay.button_fg,
                &mut self.overlay.button_fg,
            )?;
            apply_color(
                "overlay.selected_button_fg",
                overlay.selected_button_fg,
                &mut self.overlay.selected_button_fg,
            )?;
            apply_color(
                "overlay.selected_button_bg",
                overlay.selected_button_bg,
                &mut self.overlay.selected_button_bg,
            )?;
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ThemeConfig {
    theme: Option<String>,
    surfaces: Option<SurfaceOverrides>,
    input: Option<InputOverrides>,
    context_bar: Option<ContextBarOverrides>,
    status_bar: Option<StatusBarOverrides>,
    messages: Option<MessageOverrides>,
    overlay: Option<OverlayOverrides>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SurfaceOverrides {
    border_type: Option<BorderTypeValue>,
    text_fg: Option<String>,
    muted_text_fg: Option<String>,
    border_dim_fg: Option<String>,
    focus_border_fg: Option<String>,
    panel_bg: Option<String>,
    sidebar_bg: Option<String>,
    sidebar_rail_bg: Option<String>,
    section_bg: Option<String>,
    title_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InputOverrides {
    border_type: Option<BorderTypeValue>,
    bg: Option<String>,
    text_fg: Option<String>,
    placeholder_fg: Option<String>,
    normal_border_fg: Option<String>,
    insert_border_fg: Option<String>,
    cursor_fg: Option<String>,
    cursor_bg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ContextBarOverrides {
    bg: Option<String>,
    label_fg: Option<String>,
    hint_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StatusBarOverrides {
    bg: Option<String>,
    label_fg: Option<String>,
    divider_fg: Option<String>,
    normal_mode_fg: Option<String>,
    insert_mode_fg: Option<String>,
    idle_fg: Option<String>,
    running_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MessageOverrides {
    user_fg: Option<String>,
    agent_fg: Option<String>,
    tool_fg: Option<String>,
    error_fg: Option<String>,
    separator_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct OverlayOverrides {
    border_type: Option<BorderTypeValue>,
    bg: Option<String>,
    mask_bg: Option<String>,
    button_fg: Option<String>,
    selected_button_fg: Option<String>,
    selected_button_bg: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BorderTypeValue {
    Plain,
    Rounded,
}

impl From<BorderTypeValue> for BorderType {
    fn from(value: BorderTypeValue) -> Self {
        match value {
            BorderTypeValue::Plain => BorderType::Plain,
            BorderTypeValue::Rounded => BorderType::Rounded,
        }
    }
}

fn apply_border_type(value: Option<BorderTypeValue>, target: &mut BorderType) {
    if let Some(value) = value {
        *target = value.into();
    }
}

fn apply_color(field: &str, value: Option<String>, target: &mut Color) -> Result<()> {
    if let Some(value) = value {
        *target = parse_color(field, &value)?;
    }
    Ok(())
}

fn parse_color(field: &str, value: &str) -> Result<Color> {
    if value.eq_ignore_ascii_case("reset") {
        return Ok(Color::Reset);
    }

    let Some(hex) = value.strip_prefix('#') else {
        bail!("{field} must use #RRGGBB or 'reset', got '{value}'");
    };
    if hex.len() != 6 {
        bail!("{field} must use #RRGGBB or 'reset', got '{value}'");
    }

    let red = u8::from_str_radix(&hex[0..2], 16)
        .with_context(|| format!("{field} has invalid red channel in '{value}'"))?;
    let green = u8::from_str_radix(&hex[2..4], 16)
        .with_context(|| format!("{field} has invalid green channel in '{value}'"))?;
    let blue = u8::from_str_radix(&hex[4..6], 16)
        .with_context(|| format!("{field} has invalid blue channel in '{value}'"))?;

    Ok(Color::Rgb(red, green, blue))
}

#[cfg(test)]
mod tests {
    use super::{OmegaTheme, ThemeSource, DEFAULT_THEME_PATH};
    use ratatui::style::Color;
    use ratatui::widgets::BorderType;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("omega-theme-{label}-{unique}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn load_creates_default_theme_file_when_missing() {
        let root = temp_root("create");
        let loaded = OmegaTheme::load(&root);
        let path = root.join(DEFAULT_THEME_PATH);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            OmegaTheme::default_theme_toml()
        );
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.source, ThemeSource::File(path));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_from_file_overrides_theme_tokens() {
        let root = temp_root("override");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        let path = omega_dir.join("theme.toml");
        fs::write(
            &path,
            "theme = \"dark\"\n\n[input]\ninsert_border_fg = \"#102030\"\n\n[surfaces]\nborder_type = \"plain\"\n",
        )
        .unwrap();

        let theme = OmegaTheme::load_from_file(&path).unwrap();

        assert_eq!(theme.input.insert_border_fg, Color::Rgb(16, 32, 48));
        assert_eq!(theme.surfaces.panel_border_type, BorderType::Plain);
        assert_eq!(
            theme.status_bar.running_fg,
            OmegaTheme::dark().status_bar.running_fg
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_theme_file_falls_back_to_builtin() {
        let root = temp_root("invalid");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        let path = omega_dir.join("theme.toml");
        fs::write(&path, "[input]\ntext_fg = \"blue\"\n").unwrap();

        let loaded = OmegaTheme::load(&root);

        assert!(matches!(loaded.source, ThemeSource::FileWithFallback(_)));
        assert_eq!(loaded.theme.input.text_fg, OmegaTheme::dark().input.text_fg);
        assert_eq!(loaded.warnings.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overlay_load_keeps_unspecified_defaults() {
        let root = temp_root("merge");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        let path = omega_dir.join("theme.toml");
        fs::write(
            &path,
            "theme = \"dark\"\n\n[status_bar]\nlabel_fg = \"#111111\"\n",
        )
        .unwrap();

        let theme = OmegaTheme::load_from_file(&path).unwrap();

        assert_eq!(theme.status_bar.label_fg, Color::Rgb(17, 17, 17));
        assert_eq!(
            theme.input.border_type,
            OmegaTheme::dark().input.border_type
        );
        assert_eq!(
            theme.overlay.selected_button_bg,
            OmegaTheme::dark().overlay.selected_button_bg
        );

        let _ = fs::remove_dir_all(root);
    }
}
