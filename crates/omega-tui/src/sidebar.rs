#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Diagnostics,
    Delivery,
    Skills,
    Knowledge,
    Todos,
    Logs,
}

impl SidebarSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Diagnostics => "Diagnostics",
            Self::Delivery => "Delivery",
            Self::Skills => "Skills",
            Self::Knowledge => "Knowledge",
            Self::Todos => "Todos",
            Self::Logs => "Logs",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Diagnostics => Self::Delivery,
            Self::Delivery => Self::Skills,
            Self::Skills => Self::Knowledge,
            Self::Knowledge => Self::Todos,
            Self::Todos => Self::Logs,
            Self::Logs => Self::Diagnostics,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Diagnostics => Self::Logs,
            Self::Delivery => Self::Diagnostics,
            Self::Skills => Self::Delivery,
            Self::Knowledge => Self::Skills,
            Self::Todos => Self::Knowledge,
            Self::Logs => Self::Todos,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarState {
    pub shell_collapsed: bool,
    pub rail_selection: SidebarSection,
    pub diagnostics_expanded: bool,
    pub delivery_expanded: bool,
    pub skills_expanded: bool,
    pub knowledge_expanded: bool,
    pub todos_expanded: bool,
    pub logs_expanded: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            shell_collapsed: false,
            rail_selection: SidebarSection::Diagnostics,
            diagnostics_expanded: false,
            delivery_expanded: false,
            skills_expanded: false,
            knowledge_expanded: true,
            todos_expanded: true,
            logs_expanded: false,
        }
    }
}

impl SidebarState {
    pub fn expanded_sections(self) -> usize {
        usize::from(self.diagnostics_expanded)
            + usize::from(self.delivery_expanded)
            + usize::from(self.skills_expanded)
            + usize::from(self.knowledge_expanded)
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
            SidebarSection::Delivery => self.delivery_expanded,
            SidebarSection::Skills => self.skills_expanded,
            SidebarSection::Knowledge => self.knowledge_expanded,
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
            SidebarSection::Delivery => self.delivery_expanded = !self.delivery_expanded,
            SidebarSection::Skills => self.skills_expanded = !self.skills_expanded,
            SidebarSection::Knowledge => self.knowledge_expanded = !self.knowledge_expanded,
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
            delivery_expanded: false,
            skills_expanded: false,
            knowledge_expanded: false,
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

    #[test]
    fn delivery_and_skills_start_collapsed() {
        let state = SidebarState::default();

        assert!(!state.delivery_expanded);
        assert!(!state.skills_expanded);
    }
}
