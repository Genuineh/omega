use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use omega_hpc_paths::{OmegaProjectLayout, THEME_CONFIG_PATH};
use ratatui::style::Color;
use ratatui::widgets::BorderType;
use serde::Deserialize;

pub const DEFAULT_THEME_PATH: &str = THEME_CONFIG_PATH;

const DEFAULT_THEME_TOML: &str = r##"# Default omega-tui theme overrides
#
# Every color below carries a `# role: <semantic role>` comment so
# designers can grep for the role they want to retune without having to
# read the Rust side. Roles map 1:1 to the field names in
# `crates/omega-theme/src/lib.rs`; if a field is renamed, update the
# comment to match.
theme = "dark"

[input]
border_type = "rounded"
# role: input field background (entire input box, not just the shell)
bg = "#0b1016"
# role: input field text color when typing
text_fg = "#e2e8f0"
# role: input field placeholder text ("Press Space jk ...")
placeholder_fg = "#697383"
# role: input shell border in Normal mode (read-only)
normal_border_fg = "#5b6470"
# role: input shell border in Insert mode (editable)
insert_border_fg = "#71d2c2"
# role: cursor block background (cursor "inverts" against text)
cursor_bg = "#f8fafc"

[context_bar]
# role: thin bar between response panel and input shell
bg = "#0c1117"
# role: context bar leading label text (e.g. "scene: research")
label_fg = "#d7dde5"
# role: context bar hint text (e.g. workflow / step labels)
hint_fg = "#9eb6ff"

[status_bar]
# role: bottom status bar background
bg = "#0c1117"
# role: status bar leading label text
label_fg = "#d7dde5"
# role: status bar divider strokes between segments
divider_fg = "#d7dde5"
# role: status bar mode indicator in Normal mode
normal_mode_fg = "#cbd5df"
# role: status bar mode indicator in Insert mode
insert_mode_fg = "#71d2c2"
# role: status bar idle-state spinner / dot
idle_fg = "#7ddc8b"
# role: status bar running-state spinner / dot
running_fg = "#f2d089"

[surfaces]
# role: default body text in panels
text_fg = "#d7dde5"
# role: secondary body text (subordinate labels, breadcrumbs)
muted_text_fg = "#6b7280"
# role: focused panel border + chrome accent
focus_border_fg = "#71d2c2"
# role: unfocused panel border + section outline (faint)
border_dim_fg = "#1f2937"
# role: structural dividers between sub-sections inside a panel
section_outline_fg = "#243247"
# role: default panel background (response, side panels)
panel_bg = "#0c1117"
# role: sidebar panel background
sidebar_bg = "#101722"
# role: sidebar rail (one-row tab strip) background
sidebar_rail_bg = "#141d2a"
# role: nested section card background
section_bg = "#182231"
# role: focused panel title text
title_fg = "#f8fafc"

[report]
# role: report-style section header text
section_header_fg = "#cfe0ff"
# role: emphasised metric / KPI value text
metric_emphasis_fg = "#f2d089"
# role: inline `code` text
code_fg = "#d7dde5"
# role: dimmed meta line (sub-status, low-priority info)
muted_meta_fg = "#525865"
# role: report table border color
table_border_fg = "#2a3440"
# role: report summary badge background
summary_badge_bg = "#162235"
# Markdown rendering (15B-40). Previously hardcoded as Color::Rgb(...)
# literals in render_palette(); now configurable.
# role: inline code background pill
inline_code_bg = "#141a24"
# role: fenced code block background
code_block_bg = "#0f151e"
# role: warning badge text
warning_badge_fg = "#e8be60"
# role: error badge text
error_badge_fg = "#d67878"
# role: thinking-block body text (dim)
thinking_body_fg = "#4b5563"
# Dedicated source colors (decoupled from surfaces.title_fg).
# role: markdown H1 text
heading_1_fg = "#f8fafc"
# role: user message leading badge text
user_badge_fg = "#f8fafc"

[messages]
# role: user-authored message body text
user_fg = "#e2e8f0"
# role: agent-authored message body text
agent_fg = "#d7dde5"
# role: tool-run message body text
tool_fg = "#a8b0bc"
# role: error / failed-state message body text
error_fg = "#c97b7b"
# role: separator stroke between message groups
separator_fg = "#1f2937"

[overlay]
border_type = "rounded"
# role: overlay (picker, dialog) background
bg = "#121823"
# role: dim layer behind an active overlay
mask_bg = "#05070a"
# role: overlay border / edge
edge_fg = "#334155"
# role: overlay drop shadow
shadow_fg = "#1a2230"
# role: overlay button text (unselected)
button_fg = "#e2e8f0"
# role: overlay button text (selected)
selected_button_fg = "#0b1016"
# role: overlay button background (selected)
selected_button_bg = "#71d2c2"
"##;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmegaTheme {
    pub surfaces: SurfaceTheme,
    pub input: InputTheme,
    pub context_bar: ContextBarTheme,
    pub status_bar: StatusBarTheme,
    pub report: ReportTheme,
    pub messages: MessageTheme,
    pub overlay: OverlayTheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTheme {
    pub panel_border_type: BorderType,
    pub text_fg: Color,
    pub muted_text_fg: Color,
    pub border_dim_fg: Color,
    pub section_outline_fg: Color,
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
pub struct ReportTheme {
    pub section_header_fg: Color,
    pub metric_emphasis_fg: Color,
    pub code_fg: Color,
    pub muted_meta_fg: Color,
    pub table_border_fg: Color,
    pub summary_badge_bg: Color,
    // Markdown rendering (15B-40) — previously hardcoded Color::Rgb(...)
    // literals in `render_palette()`. Each color is now its own field so
    // theme.toml can override it without code changes.
    pub inline_code_bg: Color,
    pub code_block_bg: Color,
    pub warning_badge_fg: Color,
    pub error_badge_fg: Color,
    pub thinking_body_fg: Color,
    // Dedicated source for `heading_1_fg` and `user_badge_fg` — the
    // original code reused `surfaces.title_fg` for these, which made a
    // single upstream color change affect three unrelated visual roles.
    pub heading_1_fg: Color,
    pub user_badge_fg: Color,
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
    pub edge_fg: Color,
    pub shadow_fg: Color,
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
    pub section_outline: Color,
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
    pub section_header_fg: Color,
    pub metric_emphasis_fg: Color,
    pub code_fg: Color,
    pub muted_meta_fg: Color,
    pub table_border_fg: Color,
    pub summary_badge_bg: Color,
    pub user_message: Color,
    pub agent_message: Color,
    pub tool_message: Color,
    pub error_message: Color,
    pub separator_message: Color,
    pub overlay_bg: Color,
    pub overlay_mask_bg: Color,
    pub overlay_edge_fg: Color,
    pub overlay_shadow_fg: Color,
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

// ---------------------------------------------------------------------------
// Sub-palettes (Task 39H).
//
// `RenderPalette` is intentionally kept as a flat 61-field struct for
// backward compatibility — the existing render path in `omega-tui` reads
// it as a single `&RenderPalette`. The four sub-palettes below give new
// code a role-bounded view: each render function can take only the
// sub-palette it needs (e.g. a markdown renderer takes only
// `MarkdownPalette`) and the type system documents which colors it can
// read.
//
// New render code should prefer the sub-palettes; existing code keeps
// reading the flat `RenderPalette` until it is migrated slice by slice.
// ---------------------------------------------------------------------------

/// Surface-level chrome: panels, sidebars, focus indicators, borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfacePalette {
    pub text: Color,
    pub border_dim: Color,
    pub section_outline: Color,
    pub focus_border: Color,
    pub panel_bg: Color,
    pub sidebar_bg: Color,
    pub sidebar_rail_bg: Color,
    pub section_bg: Color,
    pub title_fg: Color,
    pub panel_border_type: BorderType,
}

/// Message-level palette: user / agent / tool / error / separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagePalette {
    pub user: Color,
    pub agent: Color,
    pub tool: Color,
    pub error: Color,
    pub separator: Color,
    pub user_badge_fg: Color,
    pub assistant_badge_fg: Color,
    pub warning_badge_fg: Color,
    pub error_badge_fg: Color,
}

/// Markdown / report / thinking / final-answer: rich-text rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownPalette {
    pub heading_1_fg: Color,
    pub heading_2_fg: Color,
    pub heading_3_fg: Color,
    pub inline_code_fg: Color,
    pub inline_code_bg: Color,
    pub code_block_bg: Color,
    pub code_lang_fg: Color,
    pub code_border_fg: Color,
    pub hr_fg: Color,
    pub final_answer_accent_fg: Color,
    pub final_answer_border_fg: Color,
    pub thinking_summary_fg: Color,
    pub thinking_body_fg: Color,
}

/// Overlay chrome: picker, shadow, mask, buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPalette {
    pub bg: Color,
    pub mask_bg: Color,
    pub edge_fg: Color,
    pub shadow_fg: Color,
    pub button_fg: Color,
    pub button_selected_fg: Color,
    pub button_selected_bg: Color,
    pub border_type: BorderType,
}

impl RenderPalette {
    pub fn as_surface(&self) -> SurfacePalette {
        SurfacePalette {
            text: self.text,
            border_dim: self.border_dim,
            section_outline: self.section_outline,
            focus_border: self.focus_border,
            panel_bg: self.panel_bg,
            sidebar_bg: self.sidebar_bg,
            sidebar_rail_bg: self.sidebar_rail_bg,
            section_bg: self.section_bg,
            title_fg: self.title_fg,
            panel_border_type: self.panel_border_type,
        }
    }

    pub fn as_message(&self) -> MessagePalette {
        MessagePalette {
            user: self.user_message,
            agent: self.agent_message,
            tool: self.tool_message,
            error: self.error_message,
            separator: self.separator_message,
            user_badge_fg: self.user_badge_fg,
            assistant_badge_fg: self.assistant_badge_fg,
            warning_badge_fg: self.warning_badge_fg,
            error_badge_fg: self.error_badge_fg,
        }
    }

    pub fn as_markdown(&self) -> MarkdownPalette {
        MarkdownPalette {
            heading_1_fg: self.heading_1_fg,
            heading_2_fg: self.heading_2_fg,
            heading_3_fg: self.heading_3_fg,
            inline_code_fg: self.inline_code_fg,
            inline_code_bg: self.inline_code_bg,
            code_block_bg: self.code_block_bg,
            code_lang_fg: self.code_lang_fg,
            code_border_fg: self.code_border_fg,
            hr_fg: self.hr_fg,
            final_answer_accent_fg: self.final_answer_accent_fg,
            final_answer_border_fg: self.final_answer_border_fg,
            thinking_summary_fg: self.thinking_summary_fg,
            thinking_body_fg: self.thinking_body_fg,
        }
    }

    pub fn as_overlay(&self) -> OverlayPalette {
        OverlayPalette {
            bg: self.overlay_bg,
            mask_bg: self.overlay_mask_bg,
            edge_fg: self.overlay_edge_fg,
            shadow_fg: self.overlay_shadow_fg,
            button_fg: self.overlay_button_fg,
            button_selected_fg: self.overlay_button_selected_fg,
            button_selected_bg: self.overlay_button_selected_bg,
            border_type: self.overlay_border_type,
        }
    }
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
                text_fg: Color::Rgb(215, 221, 229),
                muted_text_fg: Color::Rgb(107, 114, 128),
                border_dim_fg: Color::Rgb(31, 41, 55),
                section_outline_fg: Color::Rgb(36, 50, 71),
                focus_border_fg: Color::Rgb(113, 210, 194),
                panel_bg: Color::Rgb(12, 17, 23),
                sidebar_bg: Color::Rgb(16, 23, 34),
                sidebar_rail_bg: Color::Rgb(20, 29, 42),
                section_bg: Color::Rgb(24, 34, 49),
                title_fg: Color::Rgb(248, 250, 252),
            },
            input: InputTheme {
                border_type: BorderType::Rounded,
                bg: Color::Rgb(11, 16, 22),
                text_fg: Color::Rgb(226, 232, 240),
                placeholder_fg: Color::Rgb(105, 115, 131),
                normal_border_fg: Color::Rgb(91, 100, 112),
                insert_border_fg: Color::Rgb(113, 210, 194),
                cursor_fg: Color::Reset,
                cursor_bg: Color::Rgb(248, 250, 252),
            },
            context_bar: ContextBarTheme {
                bg: Color::Rgb(12, 17, 23),
                label_fg: Color::Rgb(215, 221, 229),
                hint_fg: Color::Rgb(158, 182, 255),
            },
            status_bar: StatusBarTheme {
                bg: Color::Rgb(12, 17, 23),
                label_fg: Color::Rgb(215, 221, 229),
                divider_fg: Color::Rgb(215, 221, 229),
                normal_mode_fg: Color::Rgb(203, 213, 223),
                insert_mode_fg: Color::Rgb(113, 210, 194),
                idle_fg: Color::Rgb(125, 220, 139),
                running_fg: Color::Rgb(242, 208, 137),
            },
            report: ReportTheme {
                section_header_fg: Color::Rgb(207, 224, 255),
                metric_emphasis_fg: Color::Rgb(242, 208, 137),
                code_fg: Color::Rgb(215, 221, 229),
                muted_meta_fg: Color::Rgb(82, 88, 101),
                table_border_fg: Color::Rgb(42, 52, 64),
                summary_badge_bg: Color::Rgb(22, 34, 53),
                inline_code_bg: Color::Rgb(20, 26, 36),
                code_block_bg: Color::Rgb(15, 21, 30),
                warning_badge_fg: Color::Rgb(232, 190, 96),
                error_badge_fg: Color::Rgb(214, 120, 120),
                thinking_body_fg: Color::Rgb(75, 85, 99),
                heading_1_fg: Color::Rgb(248, 250, 252),
                user_badge_fg: Color::Rgb(248, 250, 252),
            },
            messages: MessageTheme {
                user_fg: Color::Rgb(226, 232, 240),
                agent_fg: Color::Rgb(215, 221, 229),
                tool_fg: Color::Rgb(168, 176, 188),
                error_fg: Color::Rgb(201, 123, 123),
                separator_fg: Color::Rgb(31, 41, 55),
            },
            overlay: OverlayTheme {
                border_type: BorderType::Rounded,
                bg: Color::Rgb(18, 24, 35),
                mask_bg: Color::Rgb(5, 7, 10),
                edge_fg: Color::Rgb(51, 65, 85),
                shadow_fg: Color::Rgb(26, 34, 48),
                button_fg: Color::Rgb(226, 232, 240),
                selected_button_fg: Color::Rgb(11, 16, 22),
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
            section_outline: self.surfaces.section_outline_fg,
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
            section_header_fg: self.report.section_header_fg,
            metric_emphasis_fg: self.report.metric_emphasis_fg,
            code_fg: self.report.code_fg,
            muted_meta_fg: self.report.muted_meta_fg,
            table_border_fg: self.report.table_border_fg,
            summary_badge_bg: self.report.summary_badge_bg,
            user_message: self.messages.user_fg,
            agent_message: self.messages.agent_fg,
            tool_message: self.messages.tool_fg,
            error_message: self.messages.error_fg,
            separator_message: self.messages.separator_fg,
            overlay_bg: self.overlay.bg,
            overlay_mask_bg: self.overlay.mask_bg,
            overlay_edge_fg: self.overlay.edge_fg,
            overlay_shadow_fg: self.overlay.shadow_fg,
            overlay_button_fg: self.overlay.button_fg,
            overlay_button_selected_fg: self.overlay.selected_button_fg,
            overlay_button_selected_bg: self.overlay.selected_button_bg,
            // Markdown rendering
            heading_1_fg: self.report.heading_1_fg,
            heading_2_fg: self.report.section_header_fg,
            heading_3_fg: self.report.metric_emphasis_fg,
            inline_code_fg: self.report.code_fg,
            inline_code_bg: self.report.inline_code_bg,
            hr_fg: self.status_bar.divider_fg,
            // Code block
            code_block_bg: self.report.code_block_bg,
            code_lang_fg: self.context_bar.label_fg,
            code_border_fg: self.surfaces.border_dim_fg,
            // Message badges
            user_badge_fg: self.report.user_badge_fg,
            assistant_badge_fg: self.report.section_header_fg,
            warning_badge_fg: self.report.warning_badge_fg,
            error_badge_fg: self.report.error_badge_fg,
            // Final answer
            final_answer_accent_fg: self.surfaces.focus_border_fg,
            final_answer_border_fg: self.surfaces.border_dim_fg,
            // Thinking
            thinking_summary_fg: self.report.muted_meta_fg,
            thinking_body_fg: self.report.thinking_body_fg,
        }
    }

    pub fn load(root: &Path) -> LoadedTheme {
        let path = OmegaProjectLayout::new(root.to_path_buf()).theme_path();
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
                "surfaces.section_outline_fg",
                surfaces.section_outline_fg,
                &mut self.surfaces.section_outline_fg,
            )?;
            apply_color(
                "surfaces.focus_border_fg",
                surfaces.focus_border_fg,
                &mut self.surfaces.focus_border_fg,
            )?;
            apply_color(
                "surfaces.panel_bg",
                surfaces.panel_bg,
                &mut self.surfaces.panel_bg,
            )?;
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
            apply_color(
                "surfaces.title_fg",
                surfaces.title_fg,
                &mut self.surfaces.title_fg,
            )?;
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

        if let Some(report) = config.report {
            apply_color(
                "report.section_header_fg",
                report.section_header_fg,
                &mut self.report.section_header_fg,
            )?;
            apply_color(
                "report.metric_emphasis_fg",
                report.metric_emphasis_fg,
                &mut self.report.metric_emphasis_fg,
            )?;
            apply_color("report.code_fg", report.code_fg, &mut self.report.code_fg)?;
            apply_color(
                "report.muted_meta_fg",
                report.muted_meta_fg,
                &mut self.report.muted_meta_fg,
            )?;
            apply_color(
                "report.table_border_fg",
                report.table_border_fg,
                &mut self.report.table_border_fg,
            )?;
            apply_color(
                "report.summary_badge_bg",
                report.summary_badge_bg,
                &mut self.report.summary_badge_bg,
            )?;
            apply_color(
                "report.inline_code_bg",
                report.inline_code_bg,
                &mut self.report.inline_code_bg,
            )?;
            apply_color(
                "report.code_block_bg",
                report.code_block_bg,
                &mut self.report.code_block_bg,
            )?;
            apply_color(
                "report.warning_badge_fg",
                report.warning_badge_fg,
                &mut self.report.warning_badge_fg,
            )?;
            apply_color(
                "report.error_badge_fg",
                report.error_badge_fg,
                &mut self.report.error_badge_fg,
            )?;
            apply_color(
                "report.thinking_body_fg",
                report.thinking_body_fg,
                &mut self.report.thinking_body_fg,
            )?;
            apply_color(
                "report.heading_1_fg",
                report.heading_1_fg,
                &mut self.report.heading_1_fg,
            )?;
            apply_color(
                "report.user_badge_fg",
                report.user_badge_fg,
                &mut self.report.user_badge_fg,
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
                "overlay.edge_fg",
                overlay.edge_fg,
                &mut self.overlay.edge_fg,
            )?;
            apply_color(
                "overlay.shadow_fg",
                overlay.shadow_fg,
                &mut self.overlay.shadow_fg,
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
    report: Option<ReportOverrides>,
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
    section_outline_fg: Option<String>,
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
struct ReportOverrides {
    section_header_fg: Option<String>,
    metric_emphasis_fg: Option<String>,
    code_fg: Option<String>,
    muted_meta_fg: Option<String>,
    table_border_fg: Option<String>,
    summary_badge_bg: Option<String>,
    inline_code_bg: Option<String>,
    code_block_bg: Option<String>,
    warning_badge_fg: Option<String>,
    error_badge_fg: Option<String>,
    thinking_body_fg: Option<String>,
    heading_1_fg: Option<String>,
    user_badge_fg: Option<String>,
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
    edge_fg: Option<String>,
    shadow_fg: Option<String>,
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
        assert_eq!(theme.overlay.edge_fg, OmegaTheme::dark().overlay.edge_fg);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overlay_overrides_customize_depth_tokens() {
        let root = temp_root("overlay");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        let path = omega_dir.join("theme.toml");
        fs::write(
            &path,
            "theme = \"dark\"\n\n[surfaces]\nsection_outline_fg = \"#101820\"\n\n[overlay]\nedge_fg = \"#223344\"\nshadow_fg = \"#112233\"\n",
        )
        .unwrap();

        let theme = OmegaTheme::load_from_file(&path).unwrap();
        let palette = theme.render_palette();

        assert_eq!(theme.surfaces.section_outline_fg, Color::Rgb(16, 24, 32));
        assert_eq!(theme.overlay.edge_fg, Color::Rgb(34, 51, 68));
        assert_eq!(theme.overlay.shadow_fg, Color::Rgb(17, 34, 51));
        assert_eq!(palette.section_outline, Color::Rgb(16, 24, 32));
        assert_eq!(palette.overlay_edge_fg, Color::Rgb(34, 51, 68));
        assert_eq!(palette.overlay_shadow_fg, Color::Rgb(17, 34, 51));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn report_overrides_customize_structured_output_tokens() {
        let root = temp_root("report");
        let omega_dir = root.join(".omega");
        fs::create_dir_all(&omega_dir).unwrap();
        let path = omega_dir.join("theme.toml");
        fs::write(
            &path,
            "theme = \"dark\"\n\n[report]\nsection_header_fg = \"#123456\"\nmetric_emphasis_fg = \"#654321\"\n",
        )
        .unwrap();

        let theme = OmegaTheme::load_from_file(&path).unwrap();
        let palette = theme.render_palette();

        assert_eq!(theme.report.section_header_fg, Color::Rgb(18, 52, 86));
        assert_eq!(theme.report.metric_emphasis_fg, Color::Rgb(101, 67, 33));
        assert_eq!(palette.section_header_fg, Color::Rgb(18, 52, 86));
        assert_eq!(palette.metric_emphasis_fg, Color::Rgb(101, 67, 33));
        assert_eq!(palette.inline_code_fg, theme.report.code_fg);

        let _ = fs::remove_dir_all(root);
    }
}
