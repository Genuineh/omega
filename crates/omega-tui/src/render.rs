pub(crate) mod markdown;
mod layout;
mod overlay;
mod sidebar;
mod status;
mod style;

pub(crate) use layout::render;

#[cfg(test)]
use sidebar::wrap_text;
#[cfg(test)]
use status::{bottom_status_line, bottom_status_text, input_context_line, input_context_text};
#[cfg(test)]
use style::{response_line_style, response_status_symbol_style};

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
