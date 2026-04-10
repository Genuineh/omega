use omega_session::SupervisionReadiness;

use crate::app::{App, Panel, ProjectStatusSummary};

pub(crate) fn project_placeholder_lines() -> Vec<String> {
    vec!["No project status snapshot yet.".to_string()]
}

impl App {
    pub fn open_project_detail(&mut self) -> bool {
        let Some(summary) = self.project_status.as_ref() else {
            return false;
        };
        self.open_detail_overlay(" Project ", build_project_detail_lines(summary));
        true
    }

    pub fn project_panel_title(&self) -> String {
        let mut title = "Project".to_string();
        if self.focused_panel == Panel::Project {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub(super) fn rebuild_project_lines(&mut self) {
        self.project_lines = self
            .project_status
            .as_ref()
            .map(build_project_panel_lines)
            .unwrap_or_else(project_placeholder_lines);
    }

    pub(super) fn clear_project_status(&mut self) {
        self.project_status = None;
        self.project_lines = project_placeholder_lines();
        self.project_state.select(None);
        self.project_displayed_count = 0;
        self.project_pinned = false;
    }
}

pub(crate) fn project_badge_text(summary: &ProjectStatusSummary) -> String {
    summary.snapshot.record.display_name.clone()
}

pub(super) fn build_project_detail_lines(summary: &ProjectStatusSummary) -> Vec<String> {
    let snapshot = &summary.snapshot;
    let mut lines = vec![
        format!("project: {}", snapshot.record.display_name),
        format!("project_id: {}", snapshot.record.project_id),
        format!("root: {}", snapshot.record.root.display()),
        format!("sessions: {}", snapshot.sessions.len()),
        format!("active_session: {}", snapshot.record.active_session_id.as_deref().unwrap_or("none")),
        format!(
            "document: readiness={} files={} chunks={} health={}",
            readiness_label(document_readiness(summary)),
            snapshot.knowledge.document.total_files_indexed,
            snapshot.knowledge.document.total_chunks,
            snapshot.knowledge.document.health_status.as_str(),
        ),
        format!(
            "memory: readiness={} turns={} queries={} observations={}",
            readiness_label(memory_readiness(summary)),
            snapshot.knowledge.memory.total_turns_archived,
            snapshot.knowledge.memory.memory_query_count,
            snapshot.knowledge.memory.observation_count,
        ),
        "".to_string(),
        "sessions:".to_string(),
    ];

    if snapshot.sessions.is_empty() {
        lines.push("- none".to_string());
    } else {
        for session in &snapshot.sessions {
            lines.push(format!(
                "- {} [{}] turns={} last_active={}",
                session.title,
                match session.status {
                    omega_project::ProjectSessionStatus::Active => "active",
                    omega_project::ProjectSessionStatus::Idle => "idle",
                    omega_project::ProjectSessionStatus::Archived => "archived",
                },
                session.turn_count,
                session.last_active_at,
            ));
            lines.push(format!(
                "  preview: {}",
                session.last_user_turn_preview.as_deref().unwrap_or("none")
            ));
        }
    }

    lines
}

fn build_project_panel_lines(summary: &ProjectStatusSummary) -> Vec<String> {
    let snapshot = &summary.snapshot;
    let lead_session = snapshot.sessions.first();
    let mut lines = vec![
        format!("project: {}", snapshot.record.display_name),
        format!("root: {}", snapshot.record.root.display()),
        format!(
            "active session: {}",
            snapshot.record.active_session_id.as_deref().unwrap_or("none")
        ),
        format!(
            "document totals: files={} chunks={}",
            snapshot.knowledge.document.total_files_indexed,
            snapshot.knowledge.document.total_chunks,
        ),
        format!(
            "memory totals: turns={} queries={} observations={}",
            snapshot.knowledge.memory.total_turns_archived,
            snapshot.knowledge.memory.memory_query_count,
            snapshot.knowledge.memory.observation_count,
        ),
    ];

    if let Some(session) = lead_session {
        lines.push(format!(
            "lead session: {} [{}] turns={}",
            session.title,
            match session.status {
                omega_project::ProjectSessionStatus::Active => "active",
                omega_project::ProjectSessionStatus::Idle => "idle",
                omega_project::ProjectSessionStatus::Archived => "archived",
            },
            session.turn_count,
        ));
    }

    lines
}

fn document_readiness(summary: &ProjectStatusSummary) -> SupervisionReadiness {
    let document = &summary.snapshot.knowledge.document;
    if document.last_promotion_error.as_deref().is_some_and(|value| !value.trim().is_empty()) {
        return SupervisionReadiness::Failed;
    }
    if document.pending_version.is_some() {
        return SupervisionReadiness::Degraded;
    }
    if document.active_version.is_none()
        && document.total_files_indexed == 0
        && document.total_chunks == 0
    {
        return SupervisionReadiness::Uninitialized;
    }
    if document.active_version.is_none() {
        return SupervisionReadiness::Degraded;
    }
    SupervisionReadiness::Ready
}

fn memory_readiness(summary: &ProjectStatusSummary) -> SupervisionReadiness {
    let memory = &summary.snapshot.knowledge.memory;
    if memory.total_turns_archived == 0
        && memory.memory_query_count == 0
        && memory.observation_count == 0
    {
        SupervisionReadiness::Uninitialized
    } else {
        SupervisionReadiness::Ready
    }
}

fn readiness_label(readiness: SupervisionReadiness) -> &'static str {
    readiness.as_str()
}