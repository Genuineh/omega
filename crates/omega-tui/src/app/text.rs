use ratatui::layout::Rect;

use super::{App, Panel, PanelTextPoint, PanelTextSelection, WrappedPanelLine};

pub(crate) fn wrap_text_segments(line: &str, width: usize) -> Vec<(usize, String)> {
    if width == 0 {
        return vec![(0, line.to_string())];
    }
    if line.is_empty() {
        return vec![(0, String::new())];
    }

    let chars: Vec<char> = line.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        result.push((start, chars[start..end].iter().collect()));
        start = end;
    }
    result
}

impl App {
    pub fn panel_lines(&self, panel: Panel) -> Vec<String> {
        match panel {
            Panel::Response => self.response_lines(),
            Panel::SidebarRail => Vec::new(),
            Panel::Diagnostics => self
                .diagnostics_lines
                .iter()
                .map(|line| line.text.clone())
                .collect(),
            Panel::Delivery => self.delivery_lines.clone(),
            Panel::Skills => self.skill_lines.clone(),
            Panel::Document => self.document_lines.clone(),
            Panel::Memory => self.memory_lines.clone(),
            Panel::Todo => self.todo_lines.clone(),
            Panel::Logs => self.log_lines.clone(),
        }
    }

    pub fn wrapped_panel_lines(&self, panel: Panel, width: usize) -> Vec<WrappedPanelLine> {
        self.panel_lines(panel)
            .into_iter()
            .enumerate()
            .flat_map(|(source_line_index, line)| {
                wrap_text_segments(&line, width).into_iter().map(
                    move |(source_column_start, text)| WrappedPanelLine {
                        source_line_index,
                        source_column_start,
                        text,
                    },
                )
            })
            .collect()
    }

    pub fn selection_for_panel(&self, panel: Panel) -> Option<(PanelTextPoint, PanelTextPoint)> {
        let selection = self.text_selection?;
        if selection.panel != panel {
            return None;
        }

        Some(normalize_points(selection.anchor, selection.focus))
    }

    pub fn selection_range_for_segment(
        &self,
        panel: Panel,
        source_line_index: usize,
        source_column_start: usize,
        source_column_end: usize,
    ) -> Option<(usize, usize)> {
        let (selection_start, selection_end) = self.selection_for_panel(panel)?;
        if selection_start == selection_end {
            return None;
        }

        let segment_start = PanelTextPoint {
            line_index: source_line_index,
            column: source_column_start,
        };
        let segment_end = PanelTextPoint {
            line_index: source_line_index,
            column: source_column_end,
        };

        let overlap_start = max_text_point(selection_start, segment_start);
        let overlap_end = min_text_point(selection_end, segment_end);
        if overlap_start >= overlap_end {
            return None;
        }

        Some((
            overlap_start.column.saturating_sub(source_column_start),
            overlap_end.column.saturating_sub(source_column_start),
        ))
    }

    pub fn clear_text_selection(&mut self) {
        self.text_selection = None;
        self.mouse_selection_active = false;
    }

    pub fn begin_mouse_selection(&mut self, panel: Panel, col: u16, row: u16) -> bool {
        let Some(point) = self.panel_text_point_at(panel, col, row) else {
            self.clear_text_selection();
            return false;
        };

        self.text_selection = Some(PanelTextSelection {
            panel,
            anchor: point,
            focus: point,
        });
        self.mouse_selection_active = true;
        true
    }

    pub fn update_mouse_selection(&mut self, col: u16, row: u16) -> bool {
        let Some(selection) = self.text_selection else {
            return false;
        };
        if !self.mouse_selection_active {
            return false;
        }

        let Some(point) = self.panel_text_point_at(selection.panel, col, row) else {
            return false;
        };

        if let Some(selection) = self.text_selection.as_mut() {
            selection.focus = point;
        }
        true
    }

    pub fn finish_mouse_selection(&mut self, col: u16, row: u16) -> Option<String> {
        if !self.mouse_selection_active {
            return None;
        }

        self.update_mouse_selection(col, row);
        self.mouse_selection_active = false;

        let text = self.selected_text();
        if text.as_deref().is_some_and(|value| !value.is_empty()) {
            text
        } else {
            self.text_selection = None;
            None
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let selection = self.text_selection?;
        let (start, end) = normalize_points(selection.anchor, selection.focus);
        if start == end {
            return None;
        }

        let lines = self.panel_lines(selection.panel);
        if start.line_index >= lines.len() {
            return None;
        }

        let final_line = end.line_index.min(lines.len().saturating_sub(1));
        let mut selected = Vec::new();

        for (offset, line) in lines[start.line_index..=final_line].iter().enumerate() {
            let line_index = start.line_index + offset;
            let line_len = line.chars().count();
            let start_column = if line_index == start.line_index {
                start.column.min(line_len)
            } else {
                0
            };
            let end_column = if line_index == final_line {
                end.column.min(line_len)
            } else {
                line_len
            };

            if line_index == final_line && start_column >= end_column {
                continue;
            }

            selected.push(slice_chars(line, start_column, end_column));
        }

        Some(selected.join("\n"))
    }

    pub fn panel_text_point_at(&self, panel: Panel, col: u16, row: u16) -> Option<PanelTextPoint> {
        let inner = self.panel_inner_rect(panel)?;
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        if col < inner.x
            || col >= inner.x.saturating_add(inner.width)
            || row < inner.y
            || row >= inner.y.saturating_add(inner.height)
        {
            return None;
        }

        let wrapped_lines = self.wrapped_panel_lines(panel, inner.width as usize);
        let viewport_index = self.panel_scroll_offset(panel) + (row - inner.y) as usize;
        let wrapped_line = wrapped_lines.get(viewport_index)?;
        let line_len = wrapped_line.text.chars().count();
        let column = (col - inner.x) as usize;

        Some(PanelTextPoint {
            line_index: wrapped_line.source_line_index,
            column: wrapped_line.source_column_start + column.min(line_len),
        })
    }

    pub fn panel_scroll_offset(&self, panel: Panel) -> usize {
        match panel {
            Panel::Response => self.response_state.offset(),
            Panel::Delivery => self.delivery_state.offset(),
            Panel::Skills => self.skills_state.offset(),
            Panel::Document => self.document_state.offset(),
            Panel::Memory => self.memory_state.offset(),
            Panel::Todo => self.todo_state.offset(),
            Panel::Logs => self.logs_state.offset(),
            Panel::Diagnostics => self.diagnostics_state.offset(),
            Panel::SidebarRail => 0,
        }
    }

    pub fn panel_inner_rect(&self, panel: Panel) -> Option<Rect> {
        let rect = match panel {
            Panel::Response => self.response_rect,
            Panel::Delivery => self.delivery_rect,
            Panel::Skills => self.skills_rect,
            Panel::Document => self.document_rect,
            Panel::Memory => self.memory_rect,
            Panel::Todo => self.todo_rect,
            Panel::Logs => self.logs_rect,
            Panel::Diagnostics => self.diagnostics_rect,
            Panel::SidebarRail => return None,
        };

        if rect.width < 2 || rect.height < 2 {
            return None;
        }

        Some(Rect::new(
            rect.x.saturating_add(1),
            rect.y.saturating_add(1),
            rect.width.saturating_sub(2),
            rect.height.saturating_sub(2),
        ))
    }
}

fn normalize_points(
    first: PanelTextPoint,
    second: PanelTextPoint,
) -> (PanelTextPoint, PanelTextPoint) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn max_text_point(first: PanelTextPoint, second: PanelTextPoint) -> PanelTextPoint {
    if first >= second {
        first
    } else {
        second
    }
}

fn min_text_point(first: PanelTextPoint, second: PanelTextPoint) -> PanelTextPoint {
    if first <= second {
        first
    } else {
        second
    }
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    text.chars().skip(start).take(end - start).collect()
}
