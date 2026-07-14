//! Flex-like layout primitives (Task 52).
//!
//! A `FlexContainer` is a column or row of children, where each child
//! declares how much space it wants via [`FlexSize`]. The layout
//! algorithm is the same one used by CSS flexbox:
//!
//! 1. Pre-allocate children with `Length(N)` — they get exactly N
//!    rows (column) or cols (row).
//! 2. Sum the requested `Fraction` weights.
//! 3. Distribute remaining space proportionally, rounding down.
//! 4. Hand any leftover rows/cols (from rounding) to the last
//!    `Fraction` or `Fill` child.
//! 5. `Fixed` children (currently a synonym for `Length(1)`) sit in
//!    step 1 alongside explicit `Length` children.
//! 6. `gap` columns/rows of background-coloured padding sit between
//!    every pair of children.
//!
//! The container is rendered with [`FlexContainer::render`], which
//! invokes each child's `render` closure with its computed `Rect`.
//!
//! See `docs/specs/omega-tui-flex-layout-and-step-unit.md` §A and
//! `docs/decisions/009-tui-flex-layout-primitives.md` for the
//! motivation.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

/// Flex direction: how children stack inside a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
}

/// Size policy: how a child claims space within a flex container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexSize {
    /// Take a single row/column (synonym for `Length(1)`). Reserved
    /// for future use when a content_size callback lets the child
    /// report its intrinsic size.
    Fixed,
    /// Take exactly N rows (column) or cols (row).
    Length(u16),
    /// Take a fraction of the remaining space (after `Length`/`Fixed`
    /// are pre-allocated). 0.0–1.0. Fractions are summed and split
    /// proportionally; leftover rows from rounding go to the last
    /// `Fraction` or `Fill` child.
    Fraction(f32),
    /// Take all remaining space (after `Length`/`Fixed` are
    /// pre-allocated).
    Fill,
}

impl FlexSize {
    /// Convert `Fixed` to `Length(1)` for layout math. A future
    /// iteration can replace this with a content_size callback.
    pub(crate) fn to_length(self) -> FlexSize {
        match self {
            FlexSize::Fixed => FlexSize::Length(1),
            other => other,
        }
    }
}

/// A single child of a flex container. The `render` closure is
/// invoked once with the rect computed for this child.
pub struct FlexChild {
    pub size: FlexSize,
    pub render: Box<dyn FnOnce(&mut Frame, Rect)>,
}

impl FlexChild {
    /// Convenience constructor: a child that takes exactly 1 row/col.
    pub fn length<F>(size: FlexSize, render: F) -> Self
    where
        F: FnOnce(&mut Frame, Rect) + 'static,
    {
        Self {
            size,
            render: Box::new(render),
        }
    }

    /// Convenience constructor: a child whose render takes ownership
    /// of a pre-computed `Line` (most common case in this codebase).
    pub fn line(size: FlexSize, line: Line<'static>) -> Self {
        Self::length(size, move |frame, rect| {
            use ratatui::widgets::Paragraph;
            let p = Paragraph::new(line);
            frame.render_widget(p, rect);
        })
    }
}

impl std::fmt::Debug for FlexChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexChild").field("size", &self.size).finish()
    }
}

/// A flex container. Render it inside a `Rect` to lay out and draw
/// its children.
pub struct FlexContainer {
    pub direction: FlexDirection,
    pub gap: u16,
    pub children: Vec<FlexChild>,
}

impl FlexContainer {
    pub fn new(direction: FlexDirection) -> Self {
        Self {
            direction,
            gap: 0,
            children: Vec::new(),
        }
    }

    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    pub fn child(mut self, child: FlexChild) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: Vec<FlexChild>) -> Self {
        self.children = children;
        self
    }

    /// Compute the per-child rects *without* rendering. Useful for
    /// tests and for callers that want to inspect layout.
    pub fn layout(&self, area: Rect) -> Vec<Rect> {
        if area.width == 0 || area.height == 0 {
            return Vec::new();
        }
        let total = self.children.len();
        if total == 0 {
            return Vec::new();
        }

        let axis_len = match self.direction {
            FlexDirection::Column => area.height,
            FlexDirection::Row => area.width,
        };
        let total_gap = self.gap.saturating_mul(total.saturating_sub(1) as u16);
        let content_axis = axis_len.saturating_sub(total_gap);

        // Step 1: pre-allocate Length/Fixed. If their total exceeds
        // the available content axis (overflow), shrink each one
        // proportionally and clamp trailing children to 0.
        let mut sizes: Vec<u16> = Vec::with_capacity(total);
        let mut fixed_sum: u16 = 0;
        let mut fraction_weight: f32 = 0.0;
        let mut fill_count: usize = 0;
        for child in &self.children {
            let s = child.size.to_length();
            match s {
                FlexSize::Length(n) => {
                    sizes.push(n);
                    fixed_sum = fixed_sum.saturating_add(n);
                }
                FlexSize::Fraction(w) => {
                    sizes.push(0); // placeholder, filled in step 3
                    fraction_weight += w;
                }
                FlexSize::Fill => {
                    sizes.push(0);
                    fill_count += 1;
                }
                FlexSize::Fixed => unreachable!("Fixed is normalised to Length(1)"),
            }
        }

        // Overflow handling: if Length children alone exceed the
        // available content axis, scale them down proportionally
        // (and clamp trailing children to 0). Fraction/Fill are
        // not affected by this — they always get a share of
        // whatever's left after Length.
        if fixed_sum > content_axis {
            let scale = (content_axis as f32) / (fixed_sum as f32);
            let mut allocated: u16 = 0;
            let mut last_length_idx: Option<usize> = None;
            for (i, s) in sizes.iter_mut().enumerate() {
                if *s == 0 {
                    continue; // Fraction/Fill, handled below
                }
                last_length_idx = Some(i);
                let scaled = ((*s as f32) * scale) as u16;
                *s = scaled;
                allocated = allocated.saturating_add(scaled);
            }
            // Hand leftover from rounding to the last Length child.
            let leftover = content_axis.saturating_sub(allocated);
            if leftover > 0 {
                if let Some(last) = last_length_idx {
                    sizes[last] = sizes[last].saturating_add(leftover);
                }
            }
            // Any remaining Length children that got scaled to 0
            // are already at 0; the rects will be empty.
            fixed_sum = content_axis;
        }

        let remaining = content_axis.saturating_sub(fixed_sum);

        // Step 2/3: distribute remaining space among Fraction + Fill.
        // Fill is treated as Fraction(1) for distribution.
        let total_weight = fraction_weight + fill_count as f32;
        let mut assigned: Vec<u16> = vec![0; total];
        if total_weight > 0.0 && remaining > 0 {
            let mut distributed: u16 = 0;
            for (i, s) in sizes.iter().enumerate() {
                if *s != 0 {
                    continue;
                }
                let w = match self.children[i].size.to_length() {
                    FlexSize::Fraction(fw) => fw,
                    FlexSize::Fill => 1.0,
                    _ => unreachable!(),
                };
                let share = ((remaining as f32) * (w / total_weight)) as u16;
                assigned[i] = share;
                distributed = distributed.saturating_add(share);
            }
            // Step 4: hand leftover to the last Fraction/Fill child.
            let leftover = remaining.saturating_sub(distributed);
            if leftover > 0 {
                if let Some(last) = self
                    .children
                    .iter()
                    .rposition(|c| matches!(c.size.to_length(), FlexSize::Fraction(_) | FlexSize::Fill))
                {
                    assigned[last] = assigned[last].saturating_add(leftover);
                }
            }
        } else if fill_count > 0 && remaining > 0 {
            let per = remaining / fill_count as u16;
            let leftover = remaining % fill_count as u16;
            for (i, s) in sizes.iter().enumerate() {
                if *s == 0 {
                    assigned[i] = per;
                }
            }
            if let Some(last) = self
                .children
                .iter()
                .rposition(|c| matches!(c.size.to_length(), FlexSize::Fill))
            {
                assigned[last] = assigned[last].saturating_add(leftover);
            }
        }

        for (i, s) in sizes.iter_mut().enumerate() {
            if *s == 0 {
                *s = assigned[i];
            }
        }

        // Compute rects.
        let mut rects = Vec::with_capacity(total);
        let mut offset: u16 = 0;
        for (i, &size) in sizes.iter().enumerate() {
            if i > 0 {
                offset = offset.saturating_add(self.gap);
            }
            let rect = match self.direction {
                FlexDirection::Column => Rect {
                    x: area.x,
                    y: area.y.saturating_add(offset),
                    width: area.width,
                    height: size,
                },
                FlexDirection::Row => Rect {
                    x: area.x.saturating_add(offset),
                    y: area.y,
                    width: size,
                    height: area.height,
                },
            };
            rects.push(rect);
            offset = offset.saturating_add(size);
        }

        rects
    }

    /// Render the container: compute rects, fill gap rows/cols with
    /// background-coloured padding, then invoke each child's render
    /// closure. Takes `&mut self` because the `FnOnce` closures must
    /// be moved out to call them.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) -> Vec<Rect> {
        let rects = self.layout(area);
        if rects.is_empty() {
            return rects;
        }

        // Fill gaps with background.
        if self.gap > 0 {
            self.fill_gaps(frame, area, &rects);
        }

        // Drain children so we can call each FnOnce closure.
        let children = std::mem::take(&mut self.children);
        for (rect, child) in rects.iter().copied().zip(children.into_iter()) {
            (child.render)(frame, rect);
        }
        rects
    }

    fn fill_gaps(&self, frame: &mut Frame, area: Rect, rects: &[Rect]) {
        // Render a single-row background fill for each gap row.
        // For Column direction: gaps are horizontal lines between
        // children. For Row direction: gaps are vertical lines.
        match self.direction {
            FlexDirection::Column => {
                for window in rects.windows(2) {
                    let gap_y = window[0].y.saturating_add(window[0].height);
                    if gap_y < area.y + area.height {
                        let gap_rect = Rect {
                            x: area.x,
                            y: gap_y,
                            width: area.width,
                            height: 1,
                        };
                        // The panel's own background bleeds through;
                        // a transparent widget is enough.
                        let p = ratatui::widgets::Paragraph::new(Line::from(""));
                        frame.render_widget(p, gap_rect);
                    }
                }
            }
            FlexDirection::Row => {
                for window in rects.windows(2) {
                    let gap_x = window[0].x.saturating_add(window[0].width);
                    if gap_x < area.x + area.width {
                        let gap_rect = Rect {
                            x: gap_x,
                            y: area.y,
                            width: 1,
                            height: area.height,
                        };
                        let p = ratatui::widgets::Paragraph::new(Line::from(""));
                        frame.render_widget(p, gap_rect);
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for FlexContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexContainer")
            .field("direction", &self.direction)
            .field("gap", &self.gap)
            .field("children.len", &self.children.len())
            .finish()
    }
}

/// Build a transparent block (no borders, no fill) suitable for
/// wrapping content inside a flex child. Useful as a default
/// chrome for flex layouts.
pub fn transparent_block() -> Block<'static> {
    Block::default().borders(Borders::NONE)
}

/// Compute a "divider" rect: a row of `─` characters in the
/// border_dim color. Useful for visual rhythm in popup footers.
pub fn divider_rect(area: Rect, color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::NONE)
        .style(Style::default().fg(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_count<F: FnOnce(&mut Frame, Rect) + 'static>(f: F) -> FlexChild {
        FlexChild::length(FlexSize::Length(1), f)
    }

    #[test]
    fn three_length_children_sum_to_container_height() {
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            render_count(|_, _| {}),
            render_count(|_, _| {}),
            render_count(|_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 3));
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 1);
        assert_eq!(rects[2].height, 1);
    }

    #[test]
    fn length_two_plus_fill_takes_rest() {
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            render_count(|_, _| {}),
            FlexChild::length(FlexSize::Fill, |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 5));
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 4);
    }

    #[test]
    fn fraction_half_and_half_split_remaining() {
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            FlexChild::length(FlexSize::Fraction(0.5), |_, _| {}),
            FlexChild::length(FlexSize::Fraction(0.5), |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 10));
        assert_eq!(rects[0].height, 5);
        assert_eq!(rects[1].height, 5);
    }

    #[test]
    fn fraction_with_rounding_leftover_goes_to_last() {
        // 1 row of length(2) + 2 fractions = 3 rows split over
        // 10-row container → 4 each, but rounding will give 3 or 4.
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            FlexChild::length(FlexSize::Length(2), |_, _| {}),
            FlexChild::length(FlexSize::Fraction(0.5), |_, _| {}),
            FlexChild::length(FlexSize::Fraction(0.5), |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 10));
        // Length(2) takes 2, leaving 8 → 4 + 4.
        assert_eq!(rects[0].height, 2);
        assert_eq!(rects[1].height + rects[2].height, 8);
    }

    #[test]
    fn zero_height_container_returns_empty() {
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            render_count(|_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 0));
        assert!(rects.is_empty());
    }

    #[test]
    fn zero_width_container_returns_empty() {
        let c = FlexContainer::new(FlexDirection::Row).children(vec![
            render_count(|_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 0, 10));
        assert!(rects.is_empty());
    }

    #[test]
    fn no_children_returns_empty() {
        let c = FlexContainer::new(FlexDirection::Column);
        let rects = c.layout(Rect::new(0, 0, 10, 10));
        assert!(rects.is_empty());
    }

    #[test]
    fn gap_inserts_padding_between_children() {
        // 2 Length(1) + gap(1) in 5-row container: each child
        // takes 1 row, gap is 1 row, 2 rows of unused space at
        // the bottom. Total used = 3 rows, leftover = 2.
        let c = FlexContainer::new(FlexDirection::Column)
            .gap(1)
            .children(vec![
                render_count(|_, _| {}),
                render_count(|_, _| {}),
            ]);
        let rects = c.layout(Rect::new(0, 0, 10, 5));
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 1);
        // The two children should be 1 row apart in y (gap row in between).
        assert_eq!(rects[1].y, rects[0].y + 1 + 1);
    }

    #[test]
    fn fixed_is_treated_as_length_one() {
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            FlexChild::length(FlexSize::Fixed, |_, _| {}),
            FlexChild::length(FlexSize::Fill, |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 5));
        assert_eq!(rects[0].height, 1);
        assert_eq!(rects[1].height, 4);
    }

    #[test]
    fn row_direction_computes_widths() {
        let c = FlexContainer::new(FlexDirection::Row).children(vec![
            FlexChild::length(FlexSize::Length(20), |_, _| {}),
            FlexChild::length(FlexSize::Fill, |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 100, 10));
        assert_eq!(rects[0].width, 20);
        assert_eq!(rects[1].width, 80);
    }

    #[test]
    fn overflow_scales_children_proportionally() {
        // 3 children each Length(5) in a 10-row container: the
        // total demand (15) exceeds the available (10), so each
        // child is scaled by 10/15 = 0.667 → 3 rows each, plus 1
        // row leftover from rounding goes to the last child.
        let c = FlexContainer::new(FlexDirection::Column).children(vec![
            FlexChild::length(FlexSize::Length(5), |_, _| {}),
            FlexChild::length(FlexSize::Length(5), |_, _| {}),
            FlexChild::length(FlexSize::Length(5), |_, _| {}),
        ]);
        let rects = c.layout(Rect::new(0, 0, 10, 10));
        let total: u16 = rects.iter().map(|r| r.height).sum();
        assert_eq!(total, 10, "all heights must sum to container height");
        // Last child gets the leftover from rounding.
        assert!(rects[2].height >= rects[0].height);
    }
}
