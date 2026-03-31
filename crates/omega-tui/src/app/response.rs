use omega_observability::strip_ansi;
use omega_session::{
    ResponseSection, ResponseSectionKind, ResponseSectionState, StepSubflowRef, StepSubflowState,
    ToolRun, ToolRunStatus,
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
        let next = Msg::from_response_section(section);
        if let Some(existing) = self
            .output_msgs
            .iter_mut()
            .find(|message| message.id == next.id)
        {
            *existing = next;
        } else {
            self.output_msgs.push(next);
        }
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
            ResponseLineAction::OpenStepSubflowDetail(id) => self
                .open_step_subflow_detail(&id)
                .map(ResponseActivation::StepSubflowDetailOpened),
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
        let mut index = 0usize;
        while let Some(message) = self.output_msgs.get(index) {
            if message.kind == MsgKind::Thinking && !self.show_thinking {
                index += 1;
                continue;
            }

            if let Some(subflow_ref) = message.subflow_ref.as_ref() {
                let mut group = vec![message];
                index += 1;
                while let Some(candidate) = self.output_msgs.get(index) {
                    let same_parent = candidate.subflow_ref.as_ref().is_some_and(|candidate_ref| {
                        candidate_ref.parent_workflow_id == subflow_ref.parent_workflow_id
                            && candidate_ref.parent_step_id == subflow_ref.parent_step_id
                    });
                    if !same_parent {
                        break;
                    }
                    group.push(candidate);
                    index += 1;
                }
                lines.extend(self.render_subflow_group(&group));
                continue;
            }

            lines.extend(self.render_message_lines(message));
            index += 1;
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

    fn render_subflow_group(&self, messages: &[&Msg]) -> Vec<ResponseDisplayLine> {
        let Some(first_ref) = messages
            .iter()
            .find_map(|message| message.subflow_ref.as_ref())
        else {
            return Vec::new();
        };
        let workflow_role = messages
            .iter()
            .find_map(|message| message.workflow_role)
            .unwrap_or(WorkflowRunRole::Child);
        let scene_id = messages.iter().find_map(|message| message.scene_id.clone());
        let parent_state = messages.iter().filter_map(|message| message.state).fold(
            ResponseSectionState::Complete,
            |state, next| match (state, next) {
                (_, ResponseSectionState::Failed) | (ResponseSectionState::Failed, _) => {
                    ResponseSectionState::Failed
                }
                (ResponseSectionState::Streaming, _) | (_, ResponseSectionState::Streaming) => {
                    ResponseSectionState::Streaming
                }
                _ => ResponseSectionState::Complete,
            },
        );
        let parent_message = Msg {
            kind: MsgKind::Step,
            text: String::new(),
            id: None,
            parent_id: None,
            title: Some(first_ref.parent_step_label.clone()),
            state: Some(parent_state),
            workflow_id: Some(first_ref.parent_workflow_id.clone()),
            workflow_role: Some(workflow_role),
            scene_id: scene_id.clone(),
            subflow_ref: None,
            collapsed: false,
        };

        let total = self.subflow_total(messages, first_ref);
        let complete_count = self
            .step_subflows
            .iter()
            .filter(|status| {
                status.workflow_id == first_ref.parent_workflow_id
                    && status.step_id == first_ref.parent_step_id
                    && status.status == StepSubflowState::Complete
            })
            .count();
        let current_status = self.current_subflow_status(first_ref);

        let mut lines = vec![ResponseDisplayLine {
            kind: MsgKind::Step,
            text: format_response_header(&parent_message),
            is_header: true,
            message_id: None,
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: parent_message.state,
            thinking_line_kind: None,
        }];

        if let Some(scene_id) = scene_id {
            lines.push(ResponseDisplayLine {
                kind: MsgKind::Step,
                text: format!("  scene {scene_id}"),
                is_header: false,
                message_id: None,
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
            });
        }

        let visible_index = current_status
            .map(|status| status.item_index)
            .unwrap_or_else(|| complete_count.min(total));
        let mut summary_parts = vec![format!("items {}/{}", visible_index, total)];
        if let Some(status) = current_status {
            summary_parts.push(format!("current {}", status.subflow_id));
            if let Some(item_id) = status.item_id.as_deref() {
                summary_parts.push(format!("todo #{item_id}"));
            }
            if status.repeat_count_for_item > 0 {
                summary_parts.push(format!("repeat {}", status.repeat_count_for_item));
            }
        }
        lines.push(ResponseDisplayLine {
            kind: MsgKind::Step,
            text: format!("  {}", summary_parts.join(" · ")),
            is_header: false,
            message_id: None,
            action: None,
            is_tool_line: false,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
        });

        for item_index in 1..=total.max(1) {
            lines.extend(self.render_subflow_item(messages, first_ref, item_index));
        }

        lines
    }

    fn render_subflow_item(
        &self,
        messages: &[&Msg],
        group_ref: &StepSubflowRef,
        item_index: usize,
    ) -> Vec<ResponseDisplayLine> {
        let item_messages: Vec<&Msg> = messages
            .iter()
            .copied()
            .filter(|message| {
                message
                    .subflow_ref
                    .as_ref()
                    .is_some_and(|subflow_ref| subflow_ref.item_index == item_index)
            })
            .collect();
        let primary = item_messages
            .iter()
            .copied()
            .find(|message| message.kind == MsgKind::Step || message.kind == MsgKind::FinalAnswer);
        let thinking = item_messages
            .iter()
            .copied()
            .find(|message| message.kind == MsgKind::Thinking);
        let subflow_ref = primary
            .and_then(|message| message.subflow_ref.as_ref())
            .or_else(|| thinking.and_then(|message| message.subflow_ref.as_ref()));
        let known_status = subflow_ref
            .and_then(|subflow_ref| self.step_subflow_status_for_ref(subflow_ref))
            .or_else(|| {
                self.step_subflows.iter().find(|status| {
                    status.workflow_id == group_ref.parent_workflow_id
                        && status.step_id == group_ref.parent_step_id
                        && status.item_index == item_index
                })
            });
        let todo_fallback = self.todo_subflow_fallback(item_index);

        if primary.is_none()
            && thinking.is_none()
            && known_status.is_none()
            && todo_fallback.is_none()
        {
            return vec![ResponseDisplayLine {
                kind: MsgKind::Step,
                text: format!(
                    "  subflow  {}-{}  [queued]",
                    group_ref.parent_step_id, item_index
                ),
                is_header: false,
                message_id: None,
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
            }];
        }

        let header_ref = subflow_ref.cloned().unwrap_or_else(|| StepSubflowRef {
            parent_workflow_id: group_ref.parent_workflow_id.clone(),
            parent_step_id: group_ref.parent_step_id.clone(),
            parent_step_label: group_ref.parent_step_label.clone(),
            subflow_id: format!("{}-{}", group_ref.parent_step_id, item_index),
            item_id: known_status
                .and_then(|status| status.item_id.clone())
                .or_else(|| {
                    todo_fallback
                        .as_ref()
                        .and_then(|fallback| fallback.item_id.clone())
                }),
            item_label: known_status
                .and_then(|status| status.item_label.clone())
                .or_else(|| {
                    todo_fallback
                        .as_ref()
                        .and_then(|fallback| fallback.item_label.clone())
                }),
            item_index,
            item_total: group_ref.item_total,
        });

        let status = known_status
            .map(|status| status.status)
            .or_else(|| todo_fallback.as_ref().map(|fallback| fallback.status))
            .unwrap_or(StepSubflowState::Queued);
        let mut header = format!("  subflow  {}", header_ref.subflow_id,);
        if let Some(item_id) = header_ref.item_id.as_deref() {
            header.push_str(&format!("  #{item_id}"));
        }
        if let Some(item_label) = header_ref.item_label.as_deref() {
            header.push_str(&format!("  {}", truncate_preview(item_label, 36)));
        }
        header.push_str(&format!("  [{}]", subflow_status_label(status)));
        if let Some(status) = known_status {
            if status.repeat_count_for_item > 0 {
                header.push_str(&format!("  repeat {}", status.repeat_count_for_item));
            }
        }

        let primary_id = primary.and_then(|message| message.id.clone());
        let mut lines = vec![ResponseDisplayLine {
            kind: MsgKind::Step,
            text: header,
            is_header: false,
            message_id: primary_id.clone(),
            action: primary_id
                .clone()
                .map(ResponseLineAction::OpenStepSubflowDetail),
            is_tool_line: false,
            tool_status: None,
            response_state: primary.and_then(|message| message.state),
            thinking_line_kind: None,
        }];

        let expanded = matches!(status, StepSubflowState::Running | StepSubflowState::Failed);
        if !expanded {
            return lines;
        }

        if let Some(primary) = primary {
            let body_lines = split_or_empty(&primary.text);
            if body_lines.len() == 1 && body_lines[0].is_empty() {
                lines.push(ResponseDisplayLine {
                    kind: MsgKind::Step,
                    text: "    …".to_string(),
                    is_header: false,
                    message_id: primary.id.clone(),
                    action: None,
                    is_tool_line: false,
                    tool_status: None,
                    response_state: None,
                    thinking_line_kind: None,
                });
            } else {
                lines.extend(body_lines.into_iter().map(|line| ResponseDisplayLine {
                    kind: MsgKind::Step,
                    text: format!("    {line}"),
                    is_header: false,
                    message_id: primary.id.clone(),
                    action: None,
                    is_tool_line: false,
                    tool_status: None,
                    response_state: None,
                    thinking_line_kind: None,
                }));
            }

            if self.show_thinking {
                if let Some(thinking) = thinking {
                    let thinking_state = thinking.state.unwrap_or(ResponseSectionState::Complete);
                    lines.push(ResponseDisplayLine {
                        kind: MsgKind::Thinking,
                        text: format!(
                            "    reasoning  {}",
                            summarize_thinking_text(&thinking.text, thinking_state)
                        ),
                        is_header: false,
                        message_id: thinking.id.clone(),
                        action: thinking
                            .id
                            .clone()
                            .map(ResponseLineAction::ToggleThinkingSection),
                        is_tool_line: false,
                        tool_status: None,
                        response_state: Some(thinking_state),
                        thinking_line_kind: Some(ThinkingLineKind::Summary),
                    });
                }
            }

            let tool_runs = primary
                .id
                .as_deref()
                .map(|section_id| self.tool_runs_for_section(section_id))
                .unwrap_or_default();
            if !tool_runs.is_empty() {
                lines.push(ResponseDisplayLine {
                    kind: MsgKind::Step,
                    text: format!("    {}", format_tool_lane_header(&tool_runs).trim()),
                    is_header: false,
                    message_id: primary.id.clone(),
                    action: None,
                    is_tool_line: true,
                    tool_status: None,
                    response_state: None,
                    thinking_line_kind: None,
                });
                lines.extend(tool_runs.into_iter().map(|tool_run| ResponseDisplayLine {
                    kind: MsgKind::Step,
                    text: format!("      {}", format_tool_summary(tool_run).trim()),
                    is_header: false,
                    message_id: primary.id.clone(),
                    action: Some(ResponseLineAction::OpenToolRunDetail(tool_run.id.clone())),
                    is_tool_line: true,
                    tool_status: Some(tool_run.status),
                    response_state: None,
                    thinking_line_kind: None,
                }));
            }
        }

        lines
    }

    fn subflow_total(&self, messages: &[&Msg], first_ref: &StepSubflowRef) -> usize {
        let from_messages = messages
            .iter()
            .filter_map(|message| {
                message
                    .subflow_ref
                    .as_ref()
                    .map(|subflow_ref| subflow_ref.item_total)
            })
            .max()
            .unwrap_or(first_ref.item_total);
        let from_status = self
            .step_subflows
            .iter()
            .filter(|status| {
                status.workflow_id == first_ref.parent_workflow_id
                    && status.step_id == first_ref.parent_step_id
            })
            .map(|status| status.item_total)
            .max()
            .unwrap_or(first_ref.item_total);
        from_messages.max(from_status)
    }

    fn current_subflow_status(
        &self,
        subflow_ref: &StepSubflowRef,
    ) -> Option<&omega_session::StepSubflowStatus> {
        self.step_subflows
            .iter()
            .find(|status| {
                status.workflow_id == subflow_ref.parent_workflow_id
                    && status.step_id == subflow_ref.parent_step_id
                    && matches!(
                        status.status,
                        StepSubflowState::Running | StepSubflowState::Failed
                    )
            })
            .or_else(|| {
                // When all subflows for this parent step are complete, return None so the summary
                // shows "items N/N" without a stale "current" clause pointing at the first item.
                let matching: Vec<_> = self
                    .step_subflows
                    .iter()
                    .filter(|s| {
                        s.workflow_id == subflow_ref.parent_workflow_id
                            && s.step_id == subflow_ref.parent_step_id
                    })
                    .collect();
                let all_complete = !matching.is_empty()
                    && matching
                        .iter()
                        .all(|s| s.status == StepSubflowState::Complete);
                if all_complete {
                    return None;
                }
                // Otherwise return the last matching subflow (e.g. the most recently queued one).
                matching.into_iter().last()
            })
    }

    fn todo_subflow_fallback(&self, item_index: usize) -> Option<TodoSubflowFallback> {
        let line = self.todo_lines.get(item_index.checked_sub(1)?)?;
        parse_todo_subflow_fallback(line)
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
            subflow_ref: None,
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
            subflow_ref: section.metadata.subflow_ref,
            collapsed: false,
        }
    }
}

fn subflow_status_label(status: StepSubflowState) -> &'static str {
    match status {
        StepSubflowState::Queued => "queued",
        StepSubflowState::Running => "running",
        StepSubflowState::Complete => "done",
        StepSubflowState::Failed => "failed",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TodoSubflowFallback {
    status: StepSubflowState,
    item_id: Option<String>,
    item_label: Option<String>,
}

fn parse_todo_subflow_fallback(line: &str) -> Option<TodoSubflowFallback> {
    let trimmed = line.trim_start();
    let (status, remainder) = if let Some(rest) = trimmed.strip_prefix("[x] ") {
        (StepSubflowState::Complete, rest)
    } else if let Some(rest) = trimmed.strip_prefix("[>] ") {
        (StepSubflowState::Running, rest)
    } else if let Some(rest) = trimmed.strip_prefix("[ ] ") {
        (StepSubflowState::Queued, rest)
    } else {
        return None;
    };

    let (item_id, item_label) = if let Some(rest) = remainder.strip_prefix('#') {
        if let Some((item_id, label)) = rest.split_once(':') {
            (
                Some(item_id.trim().to_string()),
                Some(label.trim().to_string()),
            )
        } else {
            (Some(rest.trim().to_string()), None)
        }
    } else {
        let label = remainder.trim();
        (None, (!label.is_empty()).then(|| label.to_string()))
    };

    Some(TodoSubflowFallback {
        status,
        item_id,
        item_label,
    })
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
