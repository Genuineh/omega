use omega_session::SkillLoadSummary;

use super::{App, Panel};

pub(crate) fn skill_placeholder_lines() -> Vec<String> {
    vec!["No routed skills loaded yet.".to_string()]
}

impl App {
    pub fn upsert_skill_load_summary(&mut self, section_id: String, summary: SkillLoadSummary) {
        self.latest_skill_load_section_id = Some(section_id.clone());
        self.skill_load_summaries.insert(section_id, summary);
        self.rebuild_skill_lines();
        self.refresh_delivery_panel();
    }

    pub(super) fn clear_skill_load_summaries(&mut self) {
        self.skill_load_summaries.clear();
        self.latest_skill_load_section_id = None;
        self.skill_lines = skill_placeholder_lines();
        self.skills_state.select(None);
        self.skills_displayed_count = 0;
        self.skills_pinned = false;
        self.refresh_delivery_panel();
    }

    pub fn skills_panel_title(&self) -> String {
        let mut title = "Skills".to_string();
        if self.focused_panel == Panel::Skills {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn open_skill_load_detail(&mut self) -> bool {
        let Some(section_id) = self.latest_skill_load_section_id.clone() else {
            return false;
        };
        self.open_skill_load_detail_for_section(&section_id)
    }

    pub fn open_skill_load_detail_for_section(&mut self, section_id: &str) -> bool {
        let Some(summary) = self.skill_load_summaries.get(section_id) else {
            return false;
        };
        self.open_detail_overlay(" Routed Skills ", build_skill_detail_lines(summary));
        true
    }

    fn rebuild_skill_lines(&mut self) {
        self.skill_lines = self
            .latest_skill_load_section_id
            .as_deref()
            .and_then(|section_id| self.skill_load_summaries.get(section_id))
            .map(build_skill_detail_lines)
            .unwrap_or_else(skill_placeholder_lines);
    }
}

fn build_skill_detail_lines(summary: &SkillLoadSummary) -> Vec<String> {
    let recognized = format_skill_ids(&summary.recognized_skill_ids);
    let loaded = format_skill_ids(&summary.loaded_skill_ids);
    let ignored = format_skill_ids(&summary.ignored_skill_ids);

    let mut lines = vec![
        format!("recognized: {}", summary.recognized_skill_ids.len()),
        format!("loaded: {}", summary.loaded_skill_ids.len()),
        format!("ignored: {}", summary.ignored_skill_ids.len()),
        String::new(),
        format!("recognized ids: {recognized}"),
        format!("loaded ids: {loaded}"),
        format!("ignored ids: {ignored}"),
    ];

    if let Some(step_id) = summary.source_step_id.as_deref() {
        lines.push(String::new());
        lines.push(format!("source step: {step_id}"));
    }
    if let Some(reason) = summary.selection_reason.as_deref() {
        lines.push(format!("reason: {reason}"));
    }

    lines
}

fn format_skill_ids(skill_ids: &[String]) -> String {
    if skill_ids.is_empty() {
        "none".to_string()
    } else {
        skill_ids.join(", ")
    }
}
