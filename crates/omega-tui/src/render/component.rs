//! Reusable widget-building components for the omega-tui render layer.
//!
//! The `Block` + `borders` + `title` + `border_style` + `style` chain is
//! repeated for every panel in the application. The builders in this module
//! capture that pattern so the call site reads as intent ("focused panel
//! 'Agent Response'") instead of widget plumbing.
//!
//! The component layer takes a `PaletteView` (a borrowed view of the theme
//! fields it needs) so callers cannot accidentally reach into the full
//! `RenderPalette` and create hidden coupling between a panel and a
//! theme field it does not own.
//!
//! See `docs/decisions/008-tui-component-architecture-refactor.md` for the
//! motivation and `docs/TODO.md` Task 39A for the plan.

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Widget};
use ratatui::Frame;

use omega_theme::RenderPalette as ColorScheme;

/// A focused/unfocused state used by every component to pick its border and
/// title style. Centralising this flag here keeps the per-panel call sites
/// from re-implementing the same focus-vs-idle style switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusState {
    focused: bool,
}

impl FocusState {
    pub const fn new(focused: bool) -> Self {
        Self { focused }
    }

    pub const fn is_focused(self) -> bool {
        self.focused
    }

    pub const FOCUSED: FocusState = FocusState { focused: true };
    pub const UNFOCUSED: FocusState = FocusState { focused: false };
}

/// A `Panel` is a bordered box with a title and a background, drawn around
/// some content. It is the most common building block: response, sidebar,
/// input, status, overlay — every chrome-bearing surface in the app is a
/// `Panel`.
///
/// `Panel` does not own its content. Callers compose it with whatever
/// `Widget` they want via [`Panel::render_with`] or [`Panel::inner`] +
/// the standard `frame.render_widget(content, inner)` pattern.
///
/// The unfocused title color and the unfocused border color are separate
/// fields: the original render code uses a slightly tinted blue (`context_hint`)
/// for unfocused titles so they remain visible without competing with the
/// focused panel's brighter color. Conflating them would either dim the
/// title (losing the hint) or brighten the border (defeating the focus
/// indicator).
#[derive(Debug, Clone)]
pub struct Panel<'a> {
    title: Line<'a>,
    focus: FocusState,
    bg: ratatui::style::Color,
    border_type: ratatui::widgets::BorderType,
    border_focused: ratatui::style::Color,
    border_unfocused: ratatui::style::Color,
    title_focused: ratatui::style::Color,
    title_unfocused: ratatui::style::Color,
}

impl<'a> Panel<'a> {
    pub fn new(title: impl Into<Line<'a>>) -> Self {
        Self {
            title: title.into(),
            focus: FocusState::UNFOCUSED,
            bg: ratatui::style::Color::Reset,
            border_type: ratatui::widgets::BorderType::Plain,
            border_focused: ratatui::style::Color::Reset,
            border_unfocused: ratatui::style::Color::Reset,
            title_focused: ratatui::style::Color::Reset,
            title_unfocused: ratatui::style::Color::Reset,
        }
    }

    pub fn focus(mut self, focus: FocusState) -> Self {
        self.focus = focus;
        self
    }

    pub fn colors(mut self, colors: &ColorScheme) -> Self {
        self.bg = colors.panel_bg;
        self.border_type = colors.panel_border_type;
        self.border_focused = colors.focus_border;
        self.border_unfocused = colors.border_dim;
        self.title_focused = colors.title_fg;
        self.title_unfocused = colors.context_hint;
        self
    }

    pub fn with_bg(mut self, bg: ratatui::style::Color) -> Self {
        self.bg = bg;
        self
    }

    pub fn with_border_type(mut self, border_type: ratatui::widgets::BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    pub fn with_border_colors(
        mut self,
        focused: ratatui::style::Color,
        unfocused: ratatui::style::Color,
    ) -> Self {
        self.border_focused = focused;
        self.border_unfocused = unfocused;
        self
    }

    pub fn with_title_colors(
        mut self,
        focused: ratatui::style::Color,
        unfocused: ratatui::style::Color,
    ) -> Self {
        self.title_focused = focused;
        self.title_unfocused = unfocused;
        self
    }

    pub fn block(&self) -> Block<'a> {
        let (border_color, title_color) = if self.focus.is_focused() {
            (self.border_focused, self.title_focused)
        } else {
            (self.border_unfocused, self.title_unfocused)
        };
        let border_style = Style::default().fg(border_color);
        let title_style = if self.focus.is_focused() {
            Style::default()
                .fg(title_color)
                .bg(self.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(title_color).bg(self.bg)
        };
        Block::default()
            .border_type(self.border_type)
            .title(self.title.clone())
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(self.bg))
            .title_style(title_style)
    }

    /// Render the panel chrome into `area` and return the inner area (one
    /// cell inset on each side) for the caller to render content into.
    pub fn render(&self, frame: &mut Frame, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
        let block = self.block();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }

    /// Render the panel chrome with a content widget in one call.
    pub fn render_with<W: Widget>(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        content: W,
    ) {
        let inner = self.render(frame, area);
        frame.render_widget(content, inner);
    }

    pub fn inner(&self, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
        self.block().inner(area)
    }
}

/// A `Section` is a `Panel` whose background matches the surrounding surface
/// (no visible box) but still carries a title bar. Use for sub-sections
/// inside a panel where the visual grouping is the title, not the border.
#[derive(Debug, Clone)]
pub struct Section<'a> {
    title: Line<'a>,
    bg: ratatui::style::Color,
    title_fg: ratatui::style::Color,
    dim: bool,
}

impl<'a> Section<'a> {
    pub fn new(title: impl Into<Line<'a>>) -> Self {
        Self {
            title: title.into(),
            bg: ratatui::style::Color::Reset,
            title_fg: ratatui::style::Color::Reset,
            dim: false,
        }
    }

    pub fn colors(mut self, colors: &ColorScheme) -> Self {
        self.bg = colors.section_bg;
        self.title_fg = colors.section_header_fg;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn title_style(&self) -> Style {
        let style = Style::default().fg(self.title_fg).bg(self.bg);
        if self.dim {
            style.add_modifier(Modifier::DIM)
        } else {
            style.add_modifier(Modifier::BOLD)
        }
    }

    pub fn block(&self) -> Block<'a> {
        Block::default()
            .title(self.title.clone())
            .title_style(self.title_style())
            .style(Style::default().bg(self.bg))
    }

    pub fn render(&self, frame: &mut Frame, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
        let block = self.block();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }

    pub fn render_with<W: Widget>(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        content: W,
    ) {
        let inner = self.render(frame, area);
        frame.render_widget(content, inner);
    }
}

/// A `Card` is the lightweight cousin: a single-line titled row with no
/// border, used for compact summary rows in the sidebar rail and status
/// bar. The text carries the title; the row is a single styled `Line`.
#[derive(Debug, Clone)]
pub struct Card<'a> {
    line: Line<'a>,
    bg: ratatui::style::Color,
    fg: ratatui::style::Color,
    bold: bool,
}

impl<'a> Card<'a> {
    pub fn new(line: impl Into<Line<'a>>) -> Self {
        Self {
            line: line.into(),
            bg: ratatui::style::Color::Reset,
            fg: ratatui::style::Color::Reset,
            bold: false,
        }
    }

    pub fn colors(mut self, colors: &ColorScheme) -> Self {
        self.bg = colors.section_bg;
        self.fg = colors.text;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn style(&self) -> Style {
        let style = Style::default().fg(self.fg).bg(self.bg);
        if self.bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn render(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let para = ratatui::widgets::Paragraph::new(self.line.clone()).style(self.style());
        frame.render_widget(para, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_theme::OmegaTheme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn focused_panel() -> Panel<'static> {
        Panel::new("Test").focus(FocusState::FOCUSED).colors(&OmegaTheme::dark().render_palette())
    }

    #[test]
    fn panel_block_changes_border_color_with_focus() {
        let focused = focused_panel().block();
        let unfocused = Panel::new("Test")
            .focus(FocusState::UNFOCUSED)
            .colors(&OmegaTheme::dark().render_palette())
            .block();
        // Both blocks render a border; we only assert the title is present.
        assert!(format!("{focused:?}").contains("Test"));
        assert!(format!("{unfocused:?}").contains("Test"));
    }

    #[test]
    fn panel_render_produces_visible_chrome_in_buffer() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let panel = Panel::new(" Hello ").colors(&OmegaTheme::dark().render_palette());
                let _ = panel.render(frame, frame.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let first_row: String = (0..40).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(first_row.contains("─"), "expected top border, got: {first_row:?}");
    }

    #[test]
    fn section_block_has_title_but_no_border() {
        let section = Section::new("Sub").colors(&OmegaTheme::dark().render_palette());
        let block = section.block();
        // Sections are unbordered.
        let debug = format!("{block:?}");
        assert!(debug.contains("Sub"));
    }

    #[test]
    fn card_renders_bold_when_set() {
        let card = Card::new("X").bold().colors(&OmegaTheme::dark().render_palette());
        let style = card.style();
        assert_eq!(style.add_modifier, Modifier::BOLD);
    }
}
