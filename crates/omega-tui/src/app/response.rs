use omega_observability::strip_ansi;
use omega_session::{
    ResponseSection, ResponseSectionKind, ResponseSectionState, ToolRun, ToolRunStatus,
};

use super::{
    App, Msg, MsgKind, ResponseActivation, ResponseDisplayLine, ResponseLineAction,
    ThinkingLineKind, WorkflowRunRole,
};

impl App {
    pub fn push_msg(&mut self, kind: MsgKind, text: &str) {
        let clean = strip_ansi(text);
        self.output_msgs.push(Msg::plain(kind, clean));
    }

    pub fn begin_response_section(&mut self, section: ResponseSection) {
        self.output_msgs.push(Msg::from_response_section(section));
    }

    pub fn begin_tool_run(&mut self, tool_run: ToolRun) {
        self.upsert_tool_run(tool_run, true);
    }

    pub fn update_tool_run(&mut self, tool_run: ToolRun) {
        self.upsert_tool_run(tool_run, true);
    }

    pub fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus) {
        if let Some(tool_run) = self.tool_runs.iter_mut().find(|tool_run| tool_run.id == id) {
            tool_run.status = status;
        }
    }

    pub fn append_response_section(&mut self, id: &str, delta: &str) {
        if let Some(message) = self
            .output_msgs
            .iter_mut()
            .find(|message| message.id.as_deref() == Some(id))
        {
            message.text.push_str(&strip_ansi(delta));
        }
    }

    pub fn complete_response_section(&mut self, id: &str, state: ResponseSectionState) {
        if let Some(message) = self
            .output_msgs
            .iter_mut()
            .find(|message| message.id.as_deref() == Some(id))
        {
            message.state = Some(state);
            if message.kind == MsgKind::Thinking {
                message.collapsed = true;
            }
        }
    }

    pub fn set_show_thinking(&mut self, show_thinking: bool) {
        self.show_thinking = show_thinking;
    }

    #[cfg(test)]
    pub fn toggle_selected_thinking_section(&mut self) -> Option<bool> {
        let selected = self.response_state.selected()?;
        let lines = self.response_display_lines();
        let message_id = lines.get(selected)?.message_id.as_deref()?;

        self.toggle_thinking_section(message_id)
    }

    pub fn activate_selected_response_item(&mut self) -> Option<ResponseActivation> {
        let selected = self.response_state.selected()?;
        let lines = self.response_display_lines();
        let action = lines.get(selected)?.action.clone()?;

        match action {
            ResponseLineAction::ToggleThinkingSection(id) => {
                let collapsed = self.toggle_thinking_section(&id)?;
                if collapsed {
                    Some(ResponseActivation::ThinkingCollapsed)
                } else {
                    Some(ResponseActivation::ThinkingExpanded)
                }
            }
            ResponseLineAction::OpenToolRunDetail(id) => self
                .open_tool_run_detail(&id)
                .map(ResponseActivation::ToolDetailOpened),
        }
    }

    pub fn response_lines(&self) -> Vec<String> {
        self.response_display_lines()
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    pub fn response_display_lines(&self) -> Vec<ResponseDisplayLine> {
        let mut lines = Vec::new();
        for message in &self.output_msgs {
            if message.kind == MsgKind::Thinking && !self.show_thinking {
                continue;
            }
            lines.extend(self.render_message_lines(message));
        }
        lines
    }

    fn toggle_thinking_section(&mut self, id: &str) -> Option<bool> {
        let message = self.output_msgs.iter_mut().find(|message| {
            message.id.as_deref() == Some(id) && message.kind == MsgKind::Thinking
        })?;
        message.collapsed = !message.collapsed;
        Some(message.collapsed)
    }

    fn render_message_lines(&self, message: &Msg) -> Vec<ResponseDisplayLine> {
        match message.kind {
            MsgKind::User | MsgKind::Agent | MsgKind::Error | MsgKind::Separator => {
                split_or_empty(&message.text)
                    .into_iter()
                    .map(|text| ResponseDisplayLine {
                        kind: message.kind,
                        text,
                        is_header: false,
                        message_id: message.id.clone(),
                        action: None,
                        is_tool_line: false,
                        tool_status: None,
                        response_state: None,
                        thinking_line_kind: None,
                    })
                    .collect()
            }
            MsgKind::Routing | MsgKind::Step | MsgKind::FinalAnswer | MsgKind::Thinking => {
                let mut lines = Vec::new();
                let message_state = message.state.unwrap_or(ResponseSectionState::Complete);
                let default_action = if message.kind == MsgKind::Thinking {
                    message
                        .id
                        .clone()
                        .map(ResponseLineAction::ToggleThinkingSection)
                } else {
                    None
                };
                lines.push(ResponseDisplayLine {
                    kind: message.kind,
                    text: format_response_header(message),
                    is_header: true,
                    message_id: message.id.clone(),
                    action: default_action.clone(),
                    is_tool_line: false,
                    tool_status: None,
                    response_state: message.state,
                    thinking_line_kind: None,
                });

                if message.kind != MsgKind::Thinking {
                    if let Some(scene_id) = message.scene_id.as_deref() {
                        lines.push(ResponseDisplayLine {
                            kind: message.kind,
                            text: format!("  scene {scene_id}"),
                            is_header: false,
                            message_id: message.id.clone(),
                            action: None,
                            is_tool_line: false,
                            tool_status: None,
                            response_state: None,
                            thinking_line_kind: None,
                        });
                    }
                }

                match message.kind {
                    MsgKind::Routing => {
                        if let Some(preview) = first_non_empty_line(&message.text) {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format!("  result {preview}"),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                        }
                    }
                    MsgKind::Step | MsgKind::FinalAnswer => {
                        let tool_runs = message
                            .id
                            .as_deref()
                            .map(|section_id| self.tool_runs_for_section(section_id))
                            .unwrap_or_default();
                        let body_lines = split_or_empty(&message.text);
                        if body_lines.len() == 1 && body_lines[0].is_empty() && tool_runs.is_empty()
                        {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: "  …".to_string(),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                        } else if !(body_lines.len() == 1 && body_lines[0].is_empty()) {
                            lines.extend(body_lines.into_iter().map(|line| ResponseDisplayLine {
                                kind: message.kind,
                                text: format!("  {line}"),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            }));
                        }
                        if !tool_runs.is_empty() {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format_tool_lane_header(&tool_runs),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: true,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                            });
                            lines.extend(tool_runs.into_iter().map(|tool_run| {
                                ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format_tool_summary(tool_run),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: Some(ResponseLineAction::OpenToolRunDetail(
                                        tool_run.id.clone(),
                                    )),
                                    is_tool_line: true,
                                    tool_status: Some(tool_run.status),
                                    response_state: None,
                                    thinking_line_kind: None,
                                }
                            }));
                        }
                    }
                    MsgKind::Thinking => {
                        if message.collapsed {
                            lines.push(ResponseDisplayLine {
                                kind: message.kind,
                                text: format!(
                                    "    = {}",
                                    summarize_thinking_text(&message.text, message_state)
                                ),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: default_action.clone(),
                                is_tool_line: false,
                                tool_status: None,
                                response_state: Some(message_state),
                                thinking_line_kind: Some(ThinkingLineKind::Summary),
                            });
                        } else {
                            let body_lines = split_or_empty(&message.text);
                            if body_lines.len() == 1 && body_lines[0].is_empty() {
                                lines.push(ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!(
                                        "    | {}",
                                        thinking_placeholder_text(message_state)
                                    ),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: default_action.clone(),
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: Some(message_state),
                                    thinking_line_kind: Some(ThinkingLineKind::Placeholder),
                                });
                            } else {
                                lines.extend(body_lines.into_iter().map(|line| {
                                    ResponseDisplayLine {
                                        kind: message.kind,
                                        text: format!("    | {line}"),
                                        is_header: false,
                                        message_id: message.id.clone(),
                                        action: default_action.clone(),
                                        is_tool_line: false,
                                        tool_status: None,
                                        response_state: Some(message_state),
                                        thinking_line_kind: Some(ThinkingLineKind::Body),
                                    }
                                }));
                            }
                        }
                    }
                    _ => {}
                }

                lines
            }
        }
    }

    fn upsert_tool_run(&mut self, tool_run: ToolRun, append_if_missing: bool) {
        let sanitized = sanitize_tool_run(tool_run);
        if let Some(existing) = self
            .tool_runs
            .iter_mut()
            .find(|existing| existing.id == sanitized.id)
        {
            *existing = sanitized;
        } else if append_if_missing {
            self.tool_runs.push(sanitized);
        }
    }

    fn tool_runs_for_section(&self, section_id: &str) -> Vec<&ToolRun> {
        self.tool_runs
            .iter()
            .filter(|tool_run| tool_run.parent_section_id == section_id)
            .collect()
    }
}

impl Msg {
    fn plain(kind: MsgKind, text: String) -> Self {
        Self {
            kind,
            text,
            id: None,
            parent_id: None,
            title: None,
            state: None,
            workflow_id: None,
            workflow_role: None,
            scene_id: None,
            collapsed: false,
        }
    }

    fn from_response_section(section: ResponseSection) -> Self {
        Self {
            kind: match section.kind {
                ResponseSectionKind::Routing => MsgKind::Routing,
                ResponseSectionKind::Step => MsgKind::Step,
                ResponseSectionKind::FinalAnswer => MsgKind::FinalAnswer,
                ResponseSectionKind::Thinking => MsgKind::Thinking,
            },
            text: String::new(),
            id: Some(section.id),
            parent_id: section.parent_id,
            title: Some(section.title),
            state: Some(section.state),
            workflow_id: Some(section.metadata.workflow_id),
            workflow_role: Some(section.metadata.workflow_role),
            scene_id: section.metadata.scene_id,
            collapsed: false,
        }
    }
}

fn split_or_empty(text: &str) -> Vec<String> {
    if text.is_empty() {
        vec![String::new()]
    } else {
        text.lines().map(ToOwned::to_owned).collect()
    }
}

fn sanitize_tool_run(mut tool_run: ToolRun) -> ToolRun {
    tool_run.invocation_preview = strip_ansi(&tool_run.invocation_preview);
    tool_run.result_preview = tool_run.result_preview.map(|text| strip_ansi(&text));
    tool_run.detail.title = strip_ansi(&tool_run.detail.title);
    tool_run.detail.lines = tool_run
        .detail
        .lines
        .into_iter()
        .map(|line| strip_ansi(&line))
        .collect();
    tool_run
}

fn format_tool_lane_header(tool_runs: &[&ToolRun]) -> String {
    let running = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Running)
        .count();
    let failed = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Failed)
        .count();
    let total = tool_runs.len();

    if running > 0 {
        format!("  tools  {total} total · {running} running")
    } else if failed > 0 {
        format!("  tools  {total} total · {failed} failed")
    } else {
        format!("  tools  {total} total")
    }
}

fn format_tool_summary(tool_run: &ToolRun) -> String {
    let mut summary = format!(
        "    {}  [{}]  {}",
        tool_run.tool_name,
        tool_run_status_label(tool_run.status),
        tool_run.invocation_preview
    );
    if let Some(result_preview) = tool_run.result_preview.as_deref() {
        summary.push_str(" -> ");
        summary.push_str(result_preview);
    }
    summary
}

fn tool_run_status_label(status: ToolRunStatus) -> &'static str {
    match status {
        ToolRunStatus::Running => "running",
        ToolRunStatus::Complete => "done",
        ToolRunStatus::Failed => "failed",
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().find(|line| !line.trim().is_empty())
}

fn format_response_header(message: &Msg) -> String {
    let state = message.state.unwrap_or(ResponseSectionState::Complete);
    let badge = match message.kind {
        MsgKind::Routing => "route",
        MsgKind::Step => "step",
        MsgKind::FinalAnswer => "final",
        MsgKind::Thinking => "  reasoning",
        _ => "msg",
    };
    let workflow_role = message
        .workflow_role
        .map(WorkflowRunRole::as_str)
        .unwrap_or("unknown");
    let workflow_id = message.workflow_id.as_deref().unwrap_or("workflow");
    let title = match message.kind {
        MsgKind::Thinking => thinking_header_title(state),
        _ => message.title.as_deref().unwrap_or("Section"),
    };
    let state = match state {
        ResponseSectionState::Streaming => "streaming",
        ResponseSectionState::Complete => "done",
        ResponseSectionState::Failed => "failed",
    };

    format!("{badge}  {workflow_role}:{workflow_id}  {title}  [{state}]")
}

fn summarize_thinking_text(text: &str, state: ResponseSectionState) -> String {
    let preview = first_non_empty_line(text)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| truncate_preview(line, 56))
        .unwrap_or_else(|| thinking_placeholder_text(state).to_string());
    let line_count = text.lines().filter(|line| !line.trim().is_empty()).count();
    let label = thinking_summary_label(state);

    if line_count == 0 {
        format!("{label} · {preview}")
    } else if line_count == 1 {
        format!("{label} · 1 line · {preview}")
    } else {
        format!("{label} · {line_count} lines · {preview}")
    }
}

fn thinking_header_title(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "Reasoning live",
        ResponseSectionState::Complete => "Reasoning",
        ResponseSectionState::Failed => "Reasoning failed",
    }
}

fn thinking_summary_label(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "reasoning live",
        ResponseSectionState::Complete => "reasoning",
        ResponseSectionState::Failed => "reasoning failed",
    }
}

fn thinking_placeholder_text(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "waiting for reasoning...",
        ResponseSectionState::Complete => "no reasoning captured",
        ResponseSectionState::Failed => "reasoning ended before content arrived",
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
