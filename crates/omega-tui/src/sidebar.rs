#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Diagnostics,
    Todos,
    Logs,
}

impl SidebarSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Diagnostics => "Diagnostics",
            Self::Todos => "Todos",
            Self::Logs => "Logs",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Diagnostics => Self::Todos,
            Self::Todos => Self::Logs,
            Self::Logs => Self::Diagnostics,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarState {
    pub shell_collapsed: bool,
    pub rail_selection: SidebarSection,
    pub diagnostics_expanded: bool,
    pub todos_expanded: bool,
    pub logs_expanded: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            shell_collapsed: false,
            rail_selection: SidebarSection::Diagnostics,
            diagnostics_expanded: true,
            todos_expanded: true,
            logs_expanded: true,
        }
    }
}

impl SidebarState {
    pub fn expanded_sections(self) -> usize {
        usize::from(self.diagnostics_expanded)
            + usize::from(self.todos_expanded)
            + usize::from(self.logs_expanded)
    }

    pub fn toggle_shell(&mut self) {
        self.shell_collapsed = !self.shell_collapsed;
    }

    pub fn cycle_next(&mut self) {
        self.rail_selection = self.rail_selection.next();
    }

    pub fn cycle_previous(&mut self) {
        self.rail_selection = self.rail_selection.previous();
    }

    pub fn is_expanded(self, section: SidebarSection) -> bool {
        match section {
            SidebarSection::Diagnostics => self.diagnostics_expanded,
            SidebarSection::Todos => self.todos_expanded,
            SidebarSection::Logs => self.logs_expanded,
        }
    }

    pub fn toggle_selected_section(&mut self) -> bool {
        if self.is_expanded(self.rail_selection) && self.expanded_sections() == 1 {
            return false;
        }

        match self.rail_selection {
            SidebarSection::Diagnostics => self.diagnostics_expanded = !self.diagnostics_expanded,
            SidebarSection::Todos => self.todos_expanded = !self.todos_expanded,
            SidebarSection::Logs => self.logs_expanded = !self.logs_expanded,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_last_expanded_section_keeps_sidebar_body_nonempty() {
        let mut state = SidebarState {
            diagnostics_expanded: false,
            todos_expanded: false,
            logs_expanded: true,
            rail_selection: SidebarSection::Logs,
            ..SidebarState::default()
        };

        let changed = state.toggle_selected_section();

        assert!(!changed);
        assert!(state.logs_expanded);
        assert_eq!(state.expanded_sections(), 1);
    }
}
