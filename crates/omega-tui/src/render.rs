pub(crate) mod chat_turn;
mod chrome;
mod component;
mod flex;
mod frame;
mod layout;
pub(crate) mod markdown;
mod overlay;
mod response_card;
mod selection;
mod sidebar;
mod status;
mod step_unit;
mod style;

pub(crate) use chrome::{Glyph, PanelTitle, panel_title_with_focus, panel_title_with_focus_suffix};
pub(crate) use component::{Card, FocusState, Panel as PanelChrome, Section};
pub(crate) use frame::Frame;

pub(crate) use layout::render;

#[cfg(test)]
use layout::input_viewport_lines;
#[cfg(test)]
use sidebar::wrap_text;
#[cfg(test)]
use status::{
    bottom_status_line, bottom_status_text, input_context_line, input_context_text,
    input_info_line, input_info_text,
};
#[cfg(test)]
use style::{response_line_style, response_status_symbol_style};

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
