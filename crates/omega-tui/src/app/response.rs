use omega_observability::strip_ansi;
use omega_session::{
    ResponseDocumentKnowledge, ResponseMemoryKnowledge, ResponseSection,
    ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState, SectionOrigin,
    SessionRestoreSnapshot, StatusSlot, StatusValue, StepSubflowRef, StepSubflowState, ToolRun,
    ToolRunStatus,
};
use omega_project::SessionReplayEntryKind;

use crate::render::markdown::{parse_markdown_lines, MdLineKind, StyledSpan};

use super::delivery::{delivery_section_id, extract_turn_id_from_section_id};
use super::{
    App, Msg, MsgKind, ResponseActivation, ResponseCard, ResponseCardSection,
    ResponseCardSectionKind, ResponseDisplayLine, ResponseLineAction, ThinkingLineKind,
    WorkflowRunRole,
};

impl App {
    pub fn move_response_selection_up(&mut self) -> bool {
        self.move_response_selection_by(-1)
    }

    pub fn move_response_selection_down(&mut self) -> bool {
        self.move_response_selection_by(1)
    }

    pub fn select_response_line(&mut self, line_index: usize) -> bool {
        let total = self.response_display_lines().len();
        if total == 0 {
            self.response_state.select(None);
            return false;
        }

        let selected = line_index.min(total.saturating_sub(1));
        self.response_pinned = true;
        self.response_state.select(Some(selected));
        true
    }

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
        self.refresh_delivery_panel();
    }

    pub fn update_tool_run(&mut self, tool_run: ToolRun) {
        self.upsert_tool_run(tool_run, true);
        self.refresh_delivery_panel();
    }

    pub fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus) {
        if let Some(tool_run) = self.tool_runs.iter_mut().find(|tool_run| tool_run.id == id) {
            tool_run.status = status;
        }
        self.refresh_delivery_panel();
    }

    pub fn fail_running_tool_runs(&mut self) {
        for tool_run in &mut self.tool_runs {
            if tool_run.status == ToolRunStatus::Running {
                tool_run.status = ToolRunStatus::Failed;
            }
        }
        self.refresh_delivery_panel();
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

    pub fn fail_streaming_response_sections(&mut self) {
        for message in &mut self.output_msgs {
            if message.state == Some(ResponseSectionState::Streaming) {
                message.state = Some(ResponseSectionState::Failed);
                if message.kind == MsgKind::Thinking {
                    message.collapsed = true;
                }
            }
        }
    }

    pub fn restore_session(&mut self, snapshot: SessionRestoreSnapshot) {
        self.output_msgs.clear();
        self.tool_runs.clear();
        self.log_lines.clear();
        self.step_subflows.clear();
        self.clear_skill_load_summaries();
        self.step_knowledge_summaries.clear();
        self.clear_step_diagnostics();
        self.clear_context_supervision();
        self.response_state.select(None);
        self.logs_state.select(None);
        self.response_pinned = false;
        self.logs_pinned = false;
        self.overlay = None;

        self.set_status_slot(
            StatusSlot::Session,
            StatusValue::SessionRouting {
                root_workflow_id: snapshot.root_workflow_id.clone(),
                active_workflow_id: snapshot.active_workflow_id.clone(),
                active_workflow_role: snapshot.active_workflow_role,
                recognized_scene_id: snapshot.recognized_scene_id.clone(),
                selected_workflow_id: snapshot.selected_workflow_id.clone(),
            },
        );
        self.set_status_slot(
            StatusSlot::Project,
            StatusValue::ProjectSelection {
                snapshot: snapshot.project_snapshot,
            },
        );
        self.set_status_slot(StatusSlot::Agent, StatusValue::Label("Idle".to_string()));
        self.set_todo_snapshot(self.active_turn_id, &snapshot.todo_rendered);

        for (index, entry) in snapshot.replay_log.iter().enumerate() {
            match entry.kind {
                SessionReplayEntryKind::UserTurn => self.push_msg(MsgKind::User, &entry.body),
                SessionReplayEntryKind::AssistantResponse => {
                    self.push_msg(MsgKind::Agent, &entry.body)
                }
                SessionReplayEntryKind::SystemNotice => {
                    self.push_msg(MsgKind::Separator, &entry.body)
                }
                SessionReplayEntryKind::ToolSummary => {
                    self.add_log(format!("[tool] {}", entry.body));
                }
                SessionReplayEntryKind::CommandSection => {
                    let section_id = format!("restored:{}:{}", snapshot.session_id, index);
                    let title = entry.title.clone().unwrap_or_else(|| "Command".to_string());
                    let state = restored_response_state(entry.state.as_deref());
                    self.begin_response_section(ResponseSection {
                        id: section_id.clone(),
                        parent_id: None,
                        kind: ResponseSectionKind::Command,
                        title: title.clone(),
                        state,
                        metadata: ResponseSectionMetadata {
                            scene_id: None,
                            origin: SectionOrigin::Command {
                                command_name: title,
                                source: "restored".to_string(),
                            },
                            step_id: None,
                            step_label: None,
                            subflow_ref: None,
                        },
                    });
                    if !entry.body.trim().is_empty() {
                        self.append_response_section(&section_id, &entry.body);
                    }
                    self.complete_response_section(&section_id, state);
                }
            }
        }

        if !self.output_msgs.is_empty() {
            self.response_state
                .select(Some(self.output_msgs.len().saturating_sub(1)));
        }
        self.refresh_delivery_panel();
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
        self.activate_response_item_at_line(selected)
    }

    pub fn activate_response_item_at_line(
        &mut self,
        line_index: usize,
    ) -> Option<ResponseActivation> {
        let lines = self.response_display_lines();
        let action = lines.get(line_index)?.action.clone()?;

        self.activate_response_action(action)
    }

    fn activate_response_action(&mut self, action: ResponseLineAction) -> Option<ResponseActivation> {
        match action {
            ResponseLineAction::ToggleThinkingSection(id) => {
                let collapsed = self.toggle_thinking_section(&id)?;
                if collapsed {
                    Some(ResponseActivation::ThinkingCollapsed)
                } else {
                    Some(ResponseActivation::ThinkingExpanded)
                }
            }
            ResponseLineAction::ToggleCommandSection(id) => {
                let collapsed = self.toggle_command_section(&id)?;
                if collapsed {
                    Some(ResponseActivation::CommandCollapsed)
                } else {
                    Some(ResponseActivation::CommandExpanded)
                }
            }
            ResponseLineAction::ToggleToolLane(id) => {
                let collapsed = self.toggle_tool_lane(&id)?;
                if collapsed {
                    Some(ResponseActivation::ToolLaneCollapsed)
                } else {
                    Some(ResponseActivation::ToolLaneExpanded)
                }
            }
            ResponseLineAction::OpenToolRunDetail(id) => self
                .open_tool_run_detail(&id)
                .map(ResponseActivation::ToolDetailOpened),
            ResponseLineAction::OpenStepSubflowDetail(id) => self
                .open_step_subflow_detail(&id)
                .map(ResponseActivation::StepSubflowDetailOpened),
            ResponseLineAction::OpenDeliveryDetail(turn_id) => self
                .open_delivery_detail_for_turn(turn_id)
                .then_some(ResponseActivation::DeliveryDetailOpened),
            ResponseLineAction::OpenSkillLoadDetail(id) => self
                .open_skill_load_detail_for_section(&id)
                .then_some(ResponseActivation::SkillLoadDetailOpened),
            ResponseLineAction::OpenDocumentKnowledgeDetail(id) => self
                .open_document_knowledge_detail(&id)
                .map(|()| ResponseActivation::DocumentKnowledgeDetailOpened),
            ResponseLineAction::OpenMemoryKnowledgeDetail(id) => self
                .open_memory_knowledge_detail(&id)
                .map(|()| ResponseActivation::MemoryKnowledgeDetailOpened),
        }
    }

    fn move_response_selection_by(&mut self, delta: isize) -> bool {
        let total = self.response_display_lines().len();
        if total == 0 {
            self.response_state.select(None);
            return false;
        }

        let last = total.saturating_sub(1);
        let current = self.response_state.selected().unwrap_or(0);
        let next = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as usize).min(last)
        };

        self.response_pinned = true;
        self.response_state.select(Some(next));
        true
    }

    pub fn response_lines(&self) -> Vec<String> {
        self.response_display_lines()
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    pub(crate) fn response_cards(&self) -> Vec<ResponseCard> {
        let mut cards = Vec::new();
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
                cards.push(self.build_subflow_card(&group));
                continue;
            }

            cards.push(self.build_message_card(message));
            index += 1;
        }

        cards
    }

    pub fn response_display_lines(&self) -> Vec<ResponseDisplayLine> {
        self.response_cards()
            .into_iter()
            .flat_map(project_response_card)
            .collect()
    }

    fn toggle_thinking_section(&mut self, id: &str) -> Option<bool> {
        let message = self.output_msgs.iter_mut().find(|message| {
            message.id.as_deref() == Some(id) && message.kind == MsgKind::Thinking
        })?;
        message.collapsed = !message.collapsed;
        Some(message.collapsed)
    }

    fn toggle_command_section(&mut self, id: &str) -> Option<bool> {
        let message = self.output_msgs.iter_mut().find(|message| {
            message.id.as_deref() == Some(id) && message.kind == MsgKind::Command
        })?;
        message.collapsed = !message.collapsed;
        Some(message.collapsed)
    }

    fn toggle_tool_lane(&mut self, id: &str) -> Option<bool> {
        let message = self.output_msgs.iter_mut().find(|message| {
            message.id.as_deref() == Some(id)
                && matches!(message.kind, MsgKind::Step | MsgKind::FinalAnswer)
        })?;
        message.tool_lane_collapsed = !message.tool_lane_collapsed;
        Some(message.tool_lane_collapsed)
    }

    fn build_message_card(&self, message: &Msg) -> ResponseCard {
        match message.kind {
            MsgKind::User | MsgKind::Agent | MsgKind::Error | MsgKind::Separator => {
                let lines = split_or_empty(&message.text)
                    .into_iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let badge_prefix = if i == 0 {
                            match message.kind {
                                MsgKind::User => Some("▶ "),
                                MsgKind::Error => Some("✗ "),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let display_text = if let Some(prefix) = badge_prefix {
                            format!("{prefix}{text}")
                        } else {
                            text
                        };
                        ResponseDisplayLine {
                            kind: message.kind,
                            text: display_text,
                            is_header: false,
                            message_id: message.id.clone(),
                            action: None,
                            is_tool_line: false,
                            tool_status: None,
                            response_state: None,
                            thinking_line_kind: None,
                            spans: Vec::new(),
                        }
                    })
                    .collect::<Vec<_>>();
                card_from_rendered_lines(message.id.clone(), message.kind, lines)
            }
            MsgKind::Routing
            | MsgKind::Step
            | MsgKind::FinalAnswer
            | MsgKind::Thinking
            | MsgKind::Command => {
                let message_state = message.state.unwrap_or(ResponseSectionState::Complete);
                let default_action = match message.kind {
                    MsgKind::Thinking => message
                        .id
                        .clone()
                        .map(ResponseLineAction::ToggleThinkingSection),
                    MsgKind::Command => message
                        .id
                        .clone()
                        .map(ResponseLineAction::ToggleCommandSection),
                    _ => None,
                };
                let mut prelude_lines = Vec::new();
                let mut sections = Vec::new();

                // Final Answer: decorative top rule (15B-43)
                if message.kind == MsgKind::FinalAnswer {
                    prelude_lines.push(ResponseDisplayLine {
                        kind: message.kind,
                        text: "━".repeat(40),
                        is_header: false,
                        message_id: message.id.clone(),
                        action: None,
                        is_tool_line: false,
                        tool_status: None,
                        response_state: None,
                        thinking_line_kind: None,
                        spans: Vec::new(),
                    });
                }

                let header_line = ResponseDisplayLine {
                    kind: message.kind,
                    text: format_response_header(message),
                    is_header: true,
                    message_id: message.id.clone(),
                    action: default_action.clone(),
                    is_tool_line: false,
                    tool_status: None,
                    response_state: message.state,
                    thinking_line_kind: None,
                    spans: Vec::new(),
                };

                if !matches!(message.kind, MsgKind::Thinking | MsgKind::Command) {
                    if let Some(scene_id) = message.scene_id.as_deref() {
                        sections.push(ResponseCardSection {
                            kind: ResponseCardSectionKind::Meta,
                            title: None,
                            header_line: None,
                            lines: vec![ResponseDisplayLine {
                                kind: message.kind,
                                text: format!("  scene {scene_id}"),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: None,
                                is_tool_line: false,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                                spans: Vec::new(),
                            }],
                        });
                    }
                }

                match message.kind {
                    MsgKind::Routing => {
                        if let Some(preview) = first_non_empty_line(&message.text) {
                            sections.push(ResponseCardSection {
                                kind: ResponseCardSectionKind::ResultsSummary,
                                title: None,
                                header_line: None,
                                lines: vec![ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!("  result {preview}"),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: None,
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: None,
                                    thinking_line_kind: None,
                                    spans: Vec::new(),
                                }],
                            });
                        }
                    }
                    MsgKind::Step | MsgKind::FinalAnswer | MsgKind::Command => {
                        let tool_runs = message
                            .id
                            .as_deref()
                            .map(|section_id| self.tool_runs_for_section(section_id))
                            .unwrap_or_default();
                        let skill_summary = message
                            .id
                            .as_deref()
                            .and_then(|section_id| self.skill_load_summary_for_section(section_id));
                        let knowledge_summary = message
                            .id
                            .as_deref()
                            .and_then(|section_id| self.knowledge_summary_for_section(section_id));
                        let body_lines = split_or_empty(&message.text);
                        let colors = self.theme_palette();
                        let base_style = ratatui::style::Style::default();
                        let body_indent = if message.kind == MsgKind::FinalAnswer {
                            "  │ "
                        } else if message.kind == MsgKind::Command {
                            "  » "
                        } else {
                            "  "
                        };
                        let body_indent_style = if message.kind == MsgKind::FinalAnswer {
                            base_style.fg(colors.final_answer_border_fg)
                        } else if message.kind == MsgKind::Command {
                            base_style
                                .fg(colors.context_label)
                                .add_modifier(ratatui::style::Modifier::BOLD)
                        } else {
                            base_style
                        };
                        if message.kind == MsgKind::Command && message.collapsed {
                            sections.push(ResponseCardSection {
                                kind: ResponseCardSectionKind::RawDetail,
                                title: None,
                                header_line: None,
                                lines: vec![ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!(
                                        "{body_indent}▸ {}",
                                        summarize_command_text(&message.text, message_state)
                                    ),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: default_action.clone(),
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: Some(message_state),
                                    thinking_line_kind: None,
                                    spans: Vec::new(),
                                }],
                            });
                        } else if body_lines.len() == 1
                            && body_lines[0].is_empty()
                            && tool_runs.is_empty()
                            && knowledge_summary.is_none()
                        {
                            sections.push(ResponseCardSection {
                                kind: ResponseCardSectionKind::RawDetail,
                                title: None,
                                header_line: None,
                                lines: vec![ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!("{body_indent}…"),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: None,
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: None,
                                    thinking_line_kind: None,
                                    spans: Vec::new(),
                                }],
                            });
                        } else if !(body_lines.len() == 1 && body_lines[0].is_empty()) {
                            sections.extend(render_report_body_sections(
                                message,
                                &message.text,
                                body_indent,
                                body_indent_style,
                                base_style,
                                &colors,
                            ));
                        }
                        if let Some(section_id) = message.id.as_deref() {
                            let delivery_lines =
                                self.render_delivery_lane(section_id, message.kind, "  ");
                            if !delivery_lines.is_empty() {
                                sections.push(ResponseCardSection {
                                    kind: ResponseCardSectionKind::Delivery,
                                    title: None,
                                    header_line: None,
                                    lines: delivery_lines,
                                });
                            }
                            let skill_lines = self.render_skill_load_lane(
                                section_id,
                                skill_summary,
                                message.kind,
                                "  ",
                            );
                            if !skill_lines.is_empty() {
                                sections.push(ResponseCardSection {
                                    kind: ResponseCardSectionKind::SkillLoad,
                                    title: None,
                                    header_line: None,
                                    lines: skill_lines,
                                });
                            }
                            let knowledge_lines = self.render_knowledge_lane(
                                section_id,
                                knowledge_summary,
                                message.kind,
                                "  ",
                            );
                            if !knowledge_lines.is_empty() {
                                sections.push(ResponseCardSection {
                                    kind: ResponseCardSectionKind::Knowledge,
                                    title: None,
                                    header_line: None,
                                    lines: knowledge_lines,
                                });
                            }
                        }
                        // Tool lane with folding (15B-44)
                        if !tool_runs.is_empty() {
                            let can_toggle = tool_runs.len() >= 6;
                            let collapsed = can_toggle && message.tool_lane_collapsed;
                            let mut tool_lines = vec![ResponseDisplayLine {
                                kind: message.kind,
                                text: format_tool_lane_header(&tool_runs, can_toggle, collapsed),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: can_toggle.then(|| {
                                    ResponseLineAction::ToggleToolLane(
                                        message.id.clone().unwrap_or_default(),
                                    )
                                }),
                                is_tool_line: true,
                                tool_status: None,
                                response_state: None,
                                thinking_line_kind: None,
                                spans: Vec::new(),
                            }];
                            if !collapsed {
                                let name_width = tool_name_width(&tool_runs);
                                tool_lines.extend(tool_runs.into_iter().map(|tool_run| {
                                    ResponseDisplayLine {
                                        kind: message.kind,
                                        text: format_tool_summary(tool_run, name_width),
                                        is_header: false,
                                        message_id: message.id.clone(),
                                        action: Some(ResponseLineAction::OpenToolRunDetail(
                                            tool_run.id.clone(),
                                        )),
                                        is_tool_line: true,
                                        tool_status: Some(tool_run.status),
                                        response_state: None,
                                        thinking_line_kind: None,
                                        spans: Vec::new(),
                                    }
                                }));
                            }
                            sections.push(ResponseCardSection {
                                kind: ResponseCardSectionKind::ToolRuns,
                                title: None,
                                header_line: None,
                                lines: tool_lines,
                            });
                        }
                    }
                    MsgKind::Thinking => {
                        let lines = if message.collapsed {
                            vec![ResponseDisplayLine {
                                kind: message.kind,
                                text: format!(
                                    "    ▸ {}",
                                    summarize_thinking_text(&message.text, message_state)
                                ),
                                is_header: false,
                                message_id: message.id.clone(),
                                action: default_action.clone(),
                                is_tool_line: false,
                                tool_status: None,
                                response_state: Some(message_state),
                                thinking_line_kind: Some(ThinkingLineKind::Summary),
                                spans: Vec::new(),
                            }]
                        } else {
                            let body_lines = visible_thinking_body_lines(&message.text, message_state);
                            let thinking_prefix = thinking_body_prefix(message_state, self.spinner_tick);
                            if body_lines.len() == 1 && body_lines[0].is_empty() {
                                vec![ResponseDisplayLine {
                                    kind: message.kind,
                                    text: format!(
                                        "    {thinking_prefix} {}",
                                        thinking_placeholder_text(message_state)
                                    ),
                                    is_header: false,
                                    message_id: message.id.clone(),
                                    action: default_action.clone(),
                                    is_tool_line: false,
                                    tool_status: None,
                                    response_state: Some(message_state),
                                    thinking_line_kind: Some(ThinkingLineKind::Placeholder),
                                    spans: Vec::new(),
                                }]
                            } else {
                                body_lines
                                    .into_iter()
                                    .map(|line| ResponseDisplayLine {
                                        kind: message.kind,
                                        text: format!("    {thinking_prefix} {line}"),
                                        is_header: false,
                                        message_id: message.id.clone(),
                                        action: default_action.clone(),
                                        is_tool_line: false,
                                        tool_status: None,
                                        response_state: Some(message_state),
                                        thinking_line_kind: Some(ThinkingLineKind::Body),
                                        spans: Vec::new(),
                                    })
                                    .collect()
                            }
                        };
                        sections.push(ResponseCardSection {
                            kind: ResponseCardSectionKind::Thinking,
                            title: None,
                            header_line: None,
                            lines,
                        });
                    }
                    _ => {}
                }

                ResponseCard {
                    id: message.id.clone(),
                    kind: message.kind,
                    prelude_lines,
                    header_line,
                    sections,
                }
            }
        }
    }

    fn build_subflow_card(&self, messages: &[&Msg]) -> ResponseCard {
        let mut rendered = self.render_subflow_group(messages);
        if rendered.is_empty() {
            return ResponseCard {
                id: None,
                kind: MsgKind::Step,
                prelude_lines: Vec::new(),
                header_line: ResponseDisplayLine::plain(MsgKind::Step, String::new()),
                sections: Vec::new(),
            };
        }
        let header_line = rendered.remove(0);
        ResponseCard {
            id: header_line.message_id.clone(),
            kind: MsgKind::Step,
            prelude_lines: Vec::new(),
            header_line,
            sections: vec![ResponseCardSection {
                kind: ResponseCardSectionKind::Subflow,
                title: None,
                header_line: None,
                lines: rendered,
            }],
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
            tool_lane_collapsed: true,
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
            spans: Vec::new(),
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
                spans: Vec::new(),
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
            spans: Vec::new(),
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
        let active_blocking_status = self.active_blocking_subflow_status(group_ref);

        if primary.is_none()
            && thinking.is_none()
            && known_status.is_none()
            && todo_fallback.is_none()
        {
            return vec![ResponseDisplayLine {
                kind: MsgKind::Step,
                text: format!(
                    "  subflow  {}-{}  ◦",
                    group_ref.parent_step_id, item_index
                ),
                is_header: false,
                message_id: None,
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
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
        let status = if active_blocking_status
            .is_some_and(|active| active.item_index < item_index)
        {
            StepSubflowState::Queued
        } else {
            status
        };
        let mut header = format!("  subflow  {}", header_ref.subflow_id,);
        if let Some(item_id) = header_ref.item_id.as_deref() {
            header.push_str(&format!("  #{item_id}"));
        }
        if let Some(item_label) = header_ref.item_label.as_deref() {
            header.push_str(&format!("  {}", truncate_preview(item_label, 36)));
        }
        header.push_str(&format!("  {}", subflow_status_label(status)));
        if let Some(known_status) = known_status {
            if known_status.status == status && known_status.repeat_count_for_item > 0 {
                header.push_str(&format!("  repeat {}", known_status.repeat_count_for_item));
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
            spans: Vec::new(),
        }];

        let expanded = matches!(status, StepSubflowState::Running | StepSubflowState::Failed);
        if !expanded {
            return lines;
        }

        if let Some(primary) = primary {
            let knowledge_summary = primary
                .id
                .as_deref()
                .and_then(|section_id| self.knowledge_summary_for_section(section_id));
            let body_lines = split_or_empty(&primary.text);
            if body_lines.len() == 1
                && body_lines[0].is_empty()
                && knowledge_summary.is_none()
            {
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
                    spans: Vec::new(),
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
                    spans: Vec::new(),
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
                        spans: Vec::new(),
                    });
                }
            }

            let tool_runs = primary
                .id
                .as_deref()
                .map(|section_id| self.tool_runs_for_section(section_id))
                .unwrap_or_default();
            if let Some(section_id) = primary.id.as_deref() {
                lines.extend(self.render_knowledge_lane(
                    section_id,
                    knowledge_summary,
                    MsgKind::Step,
                    "    ",
                ));
            }
            if !tool_runs.is_empty() {
                let can_toggle = tool_runs.len() >= 6;
                let collapsed = can_toggle && primary.tool_lane_collapsed;
                lines.push(ResponseDisplayLine {
                    kind: MsgKind::Step,
                    text: format!(
                        "    {}",
                        format_tool_lane_header(&tool_runs, can_toggle, collapsed).trim()
                    ),
                    is_header: false,
                    message_id: primary.id.clone(),
                    action: can_toggle.then(|| {
                        ResponseLineAction::ToggleToolLane(primary.id.clone().unwrap_or_default())
                    }),
                    is_tool_line: true,
                    tool_status: None,
                    response_state: None,
                    thinking_line_kind: None,
                    spans: Vec::new(),
                });
                if !collapsed {
                    let name_width = tool_name_width(&tool_runs);
                    lines.extend(tool_runs.into_iter().map(|tool_run| ResponseDisplayLine {
                        kind: MsgKind::Step,
                        text: format!(
                            "      {}",
                            format_tool_summary(tool_run, name_width).trim()
                        ),
                        is_header: false,
                        message_id: primary.id.clone(),
                        action: Some(ResponseLineAction::OpenToolRunDetail(tool_run.id.clone())),
                        is_tool_line: true,
                        tool_status: Some(tool_run.status),
                        response_state: None,
                        thinking_line_kind: None,
                        spans: Vec::new(),
                    }));
                }
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

    fn active_blocking_subflow_status(
        &self,
        subflow_ref: &StepSubflowRef,
    ) -> Option<&omega_session::StepSubflowStatus> {
        self.step_subflows.iter().find(|status| {
            status.workflow_id == subflow_ref.parent_workflow_id
                && status.step_id == subflow_ref.parent_step_id
                && matches!(status.status, StepSubflowState::Running | StepSubflowState::Failed)
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

    fn skill_load_summary_for_section(
        &self,
        section_id: &str,
    ) -> Option<&omega_session::SkillLoadSummary> {
        self.skill_load_summaries.get(section_id)
    }

    fn knowledge_summary_for_section(
        &self,
        section_id: &str,
    ) -> Option<&omega_session::StepKnowledgeSummary> {
        self.step_knowledge_summaries.get(section_id)
    }

    fn render_skill_load_lane(
        &self,
        section_id: &str,
        summary: Option<&omega_session::SkillLoadSummary>,
        kind: MsgKind,
        indent: &str,
    ) -> Vec<ResponseDisplayLine> {
        let Some(summary) = summary else {
            return Vec::new();
        };

        vec![
            ResponseDisplayLine {
                kind,
                text: format!(
                    "{indent}skills  recognized={} loaded={} ignored={}",
                    summary.recognized_skill_ids.len(),
                    summary.loaded_skill_ids.len(),
                    summary.ignored_skill_ids.len(),
                ),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenSkillLoadDetail(section_id.to_string())),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            },
            ResponseDisplayLine {
                kind,
                text: format!(
                    "{indent}  loaded ids: {}",
                    summarize_skill_ids(&summary.loaded_skill_ids)
                ),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenSkillLoadDetail(section_id.to_string())),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            },
        ]
    }

    fn render_delivery_lane(
        &self,
        section_id: &str,
        kind: MsgKind,
        indent: &str,
    ) -> Vec<ResponseDisplayLine> {
        let Some(turn_id) = extract_turn_id_from_section_id(section_id) else {
            return Vec::new();
        };
        if section_id != delivery_section_id(turn_id) {
            return Vec::new();
        }
        let Some(summary) = self.delivery_summary_for_turn(turn_id) else {
            return Vec::new();
        };

        vec![
            ResponseDisplayLine {
                kind,
                text: format!(
                    "{indent}delivery  {}",
                    summary.summary_line()
                ),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenDeliveryDetail(turn_id)),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            },
            ResponseDisplayLine {
                kind,
                text: format!(
                    "{indent}  knowledge  doc={} · mem={} · obs={}  |  files {}",
                    summary.document_search_count,
                    summary.memory_search_count,
                    summary.observation_search_count,
                    summary.changed_files.len(),
                ),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenDeliveryDetail(turn_id)),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            },
        ]
    }

    fn render_knowledge_lane(
        &self,
        section_id: &str,
        summary: Option<&omega_session::StepKnowledgeSummary>,
        kind: MsgKind,
        indent: &str,
    ) -> Vec<ResponseDisplayLine> {
        let Some(summary) = summary else {
            return Vec::new();
        };

        let mut lines = vec![ResponseDisplayLine {
            kind,
            text: format!("{indent}knowledge"),
            is_header: false,
            message_id: Some(section_id.to_string()),
            action: None,
            is_tool_line: true,
            tool_status: None,
            response_state: None,
            thinking_line_kind: None,
            spans: Vec::new(),
        }];

        if let Some(document) = summary.document.as_ref() {
            lines.push(ResponseDisplayLine {
                kind,
                text: format!(
                    "{indent}  {}",
                    format_document_knowledge_summary(document)
                ),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenDocumentKnowledgeDetail(
                    section_id.to_string(),
                )),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            });
        }

        if let Some(memory) = summary.memory.as_ref() {
            lines.push(ResponseDisplayLine {
                kind,
                text: format!("{indent}  {}", format_memory_knowledge_summary(memory)),
                is_header: false,
                message_id: Some(section_id.to_string()),
                action: Some(ResponseLineAction::OpenMemoryKnowledgeDetail(
                    section_id.to_string(),
                )),
                is_tool_line: true,
                tool_status: None,
                response_state: None,
                thinking_line_kind: None,
                spans: Vec::new(),
            });
        }

        lines
    }

    fn open_document_knowledge_detail(&mut self, section_id: &str) -> Option<()> {
        let summary = self.step_knowledge_summaries.get(section_id)?.document.as_ref()?;
        self.open_detail_overlay(
            " Document Knowledge ",
            build_document_knowledge_detail_lines(summary),
        );
        Some(())
    }

    fn open_memory_knowledge_detail(&mut self, section_id: &str) -> Option<()> {
        let summary = self.step_knowledge_summaries.get(section_id)?.memory.as_ref()?;
        self.open_detail_overlay(
            " Memory Knowledge ",
            build_memory_knowledge_detail_lines(summary),
        );
        Some(())
    }
}

fn restored_response_state(value: Option<&str>) -> ResponseSectionState {
    match value {
        Some("failed") => ResponseSectionState::Failed,
        _ => ResponseSectionState::Complete,
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
            tool_lane_collapsed: true,
        }
    }

    fn from_response_section(section: ResponseSection) -> Self {
        let (workflow_id, workflow_role, title) = match &section.metadata.origin {
            SectionOrigin::Workflow {
                workflow_id,
                workflow_role,
            } => (
                Some(workflow_id.clone()),
                Some(*workflow_role),
                Some(section.title.clone()),
            ),
            SectionOrigin::Command {
                command_name,
                source,
            } => (Some(source.clone()), None, Some(command_name.clone())),
        };
        Self {
            kind: match section.kind {
                ResponseSectionKind::Routing => MsgKind::Routing,
                ResponseSectionKind::Step => MsgKind::Step,
                ResponseSectionKind::FinalAnswer => MsgKind::FinalAnswer,
                ResponseSectionKind::Thinking => MsgKind::Thinking,
                ResponseSectionKind::Command => MsgKind::Command,
            },
            text: String::new(),
            id: Some(section.id),
            parent_id: section.parent_id,
            title,
            state: Some(section.state),
            workflow_id,
            workflow_role,
            scene_id: section.metadata.scene_id,
            subflow_ref: section.metadata.subflow_ref,
            collapsed: false,
            tool_lane_collapsed: true,
        }
    }
}

fn subflow_status_label(status: StepSubflowState) -> &'static str {
    match status {
        StepSubflowState::Queued => "◦",
        StepSubflowState::Running => "◉",
        StepSubflowState::Complete => "●",
        StepSubflowState::Failed => "✕",
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
    } else if let Some(rest) = trimmed.strip_prefix("✓ ") {
        (StepSubflowState::Complete, rest)
    } else if let Some(rest) = trimmed.strip_prefix("[>] ") {
        (StepSubflowState::Running, rest)
    } else if let Some(rest) = trimmed.strip_prefix("→ ") {
        (StepSubflowState::Running, rest)
    } else if let Some(rest) = trimmed.strip_prefix("[ ] ") {
        (StepSubflowState::Queued, rest)
    } else if let Some(rest) = trimmed.strip_prefix("○ ") {
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

fn project_response_card(card: ResponseCard) -> Vec<ResponseDisplayLine> {
    let mut lines = card.prelude_lines;
    lines.push(card.header_line);
    for section in card.sections {
        if let Some(header_line) = section.header_line {
            lines.push(header_line);
        }
        lines.extend(section.lines);
    }
    lines
}

fn card_from_rendered_lines(
    id: Option<String>,
    kind: MsgKind,
    mut lines: Vec<ResponseDisplayLine>,
) -> ResponseCard {
    let header_line = if lines.is_empty() {
        ResponseDisplayLine::plain(kind, String::new())
    } else {
        lines.remove(0)
    };

    let sections = if lines.is_empty() {
        Vec::new()
    } else {
        vec![ResponseCardSection {
            kind: ResponseCardSectionKind::RawDetail,
            title: None,
            header_line: None,
            lines,
        }]
    };

    ResponseCard {
        id,
        kind,
        prelude_lines: Vec::new(),
        header_line,
        sections,
    }
}

fn render_report_body_sections(
    message: &Msg,
    text: &str,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    base_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> Vec<ResponseCardSection> {
    split_report_sections(text)
        .into_iter()
        .filter_map(|(title, body)| {
            let lines = render_markdown_body_lines(
                message,
                &body,
                body_indent,
                body_indent_style,
                base_style,
                colors,
            );
            if lines.is_empty() {
                return None;
            }

            let kind = title
                .as_deref()
                .map(classify_report_section)
                .unwrap_or(ResponseCardSectionKind::RawDetail);
            let header_line = title.as_deref().map(|section_title| {
                build_report_section_header_line(
                    message,
                    section_title,
                    summarize_report_section(kind, &body),
                    colors,
                )
            });

            Some(ResponseCardSection {
                kind,
                title,
                header_line,
                lines,
            })
        })
        .collect()
}

fn render_markdown_body_lines(
    message: &Msg,
    text: &str,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    base_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> Vec<ResponseDisplayLine> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let raw_lines: Vec<&str> = text.lines().collect();
    let mut rendered = Vec::new();
    let mut markdown_buffer = Vec::new();
    let mut index = 0usize;

    while index < raw_lines.len() {
        let line = raw_lines[index];
        if is_markdown_table_header(raw_lines.as_slice(), index) {
            if !markdown_buffer.is_empty() {
                rendered.extend(render_markdown_buffer(
                    message,
                    &markdown_buffer.join("\n"),
                    body_indent,
                    body_indent_style,
                    base_style,
                    colors,
                ));
                markdown_buffer.clear();
            }

            let mut table_block = vec![raw_lines[index].to_string(), raw_lines[index + 1].to_string()];
            index += 2;
            while index < raw_lines.len() && is_markdown_table_row(raw_lines[index]) {
                table_block.push(raw_lines[index].to_string());
                index += 1;
            }
            rendered.extend(render_markdown_table_lines(
                message,
                body_indent,
                body_indent_style,
                colors,
                &table_block,
            ));
            continue;
        }

        markdown_buffer.push(line.to_string());
        index += 1;
    }

    if !markdown_buffer.is_empty() {
        rendered.extend(render_markdown_buffer(
            message,
            &markdown_buffer.join("\n"),
            body_indent,
            body_indent_style,
            base_style,
            colors,
        ));
    }

    rendered
}

fn split_report_sections(text: &str) -> Vec<(Option<String>, String)> {
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut saw_heading = false;

    for line in text.lines() {
        if let Some(title) = parse_section_heading(line) {
            saw_heading = true;
            push_report_section(&mut sections, current_title.take(), &mut current_lines);
            current_title = Some(title);
            continue;
        }
        current_lines.push(line.to_string());
    }

    push_report_section(&mut sections, current_title.take(), &mut current_lines);

    if !saw_heading {
        return vec![(None, text.to_string())];
    }

    sections
}

fn push_report_section(
    sections: &mut Vec<(Option<String>, String)>,
    title: Option<String>,
    lines: &mut Vec<String>,
) {
    let body = lines.join("\n");
    lines.clear();
    if title.is_none() && body.trim().is_empty() {
        return;
    }
    if title.is_some() && body.trim().is_empty() {
        return;
    }
    sections.push((title, body));
}

fn parse_section_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    for prefix in ["## ", "### ", "# "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

fn classify_report_section(title: &str) -> ResponseCardSectionKind {
    let normalized = normalize_section_title(title);
    match normalized.as_str() {
        "results summary" | "result summary" | "summary" => {
            ResponseCardSectionKind::ResultsSummary
        }
        "changes made" | "changes" | "change summary" => ResponseCardSectionKind::ChangesMade,
        "verification" | "validation" | "tests" | "test results" => {
            ResponseCardSectionKind::Verification
        }
        "usage" | "how to use" => ResponseCardSectionKind::Usage,
        "optional next step" | "optional next steps" | "next step" | "next steps" => {
            ResponseCardSectionKind::OptionalNextStep
        }
        "key points" | "highlights" | "key takeaways" => ResponseCardSectionKind::KeyPoints,
        _ => ResponseCardSectionKind::Custom,
    }
}

fn normalize_section_title(title: &str) -> String {
    let mut normalized = String::with_capacity(title.len());
    let mut previous_space = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_report_section_header_line(
    message: &Msg,
    title: &str,
    summary: Option<String>,
    colors: &omega_theme::RenderPalette,
) -> ResponseDisplayLine {
    let title_style = ratatui::style::Style::default()
        .fg(colors.section_header_fg)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let divider_style = ratatui::style::Style::default().fg(colors.table_border_fg);
    let summary_style = ratatui::style::Style::default()
        .fg(colors.muted_meta_fg)
        .bg(colors.summary_badge_bg)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let text = if let Some(summary) = summary.as_deref() {
        format!("  {title}  {summary}")
    } else {
        format!("  {title}")
    };
    let mut spans = vec![
        StyledSpan {
            text: "  ".to_string(),
            style: divider_style,
        },
        StyledSpan {
            text: title.to_string(),
            style: title_style,
        },
    ];
    if let Some(summary) = summary {
        spans.push(StyledSpan {
            text: "  ".to_string(),
            style: ratatui::style::Style::default(),
        });
        spans.push(StyledSpan {
            text: summary,
            style: summary_style,
        });
    }
    ResponseDisplayLine {
        kind: message.kind,
        text,
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans,
    }
}

fn render_markdown_buffer(
    message: &Msg,
    text: &str,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    base_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> Vec<ResponseDisplayLine> {
    parse_markdown_lines(text, base_style, colors)
        .into_iter()
        .map(|md_line| {
            let plain: String = md_line.spans.iter().map(|span| span.text.as_str()).collect();
            let prefixed_spans = {
                let mut spans = vec![StyledSpan {
                    text: body_indent.to_string(),
                    style: body_indent_style,
                }];
                spans.extend(md_line.spans);
                spans
            };
            ResponseDisplayLine {
                kind: message.kind,
                text: format!("{body_indent}{plain}"),
                is_header: false,
                message_id: message.id.clone(),
                action: None,
                is_tool_line: false,
                tool_status: None,
                response_state: None,
                thinking_line_kind: md_line
                    .kind
                    .eq(&MdLineKind::BlankLine)
                    .then_some(ThinkingLineKind::Body),
                spans: prefixed_spans,
            }
        })
        .collect()
}

fn is_markdown_table_header(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len()
        && is_markdown_table_row(lines[index])
        && is_markdown_table_separator(lines[index + 1])
}

fn is_markdown_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed[1..trimmed.len() - 1].contains('|')
}

fn is_markdown_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return false;
    }
    trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .all(|part| !part.is_empty() && part.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn render_markdown_table_lines(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
    block: &[String],
) -> Vec<ResponseDisplayLine> {
    if block.len() < 2 {
        return Vec::new();
    }

    let rows: Vec<Vec<String>> = block.iter().map(|line| parse_markdown_table_row(line)).collect();
    let Some(header) = rows.first() else {
        return Vec::new();
    };
    let data_rows = &rows[2..];
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = (0..column_count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();

    let border_style = ratatui::style::Style::default().fg(colors.table_border_fg);
    let header_style = ratatui::style::Style::default()
        .fg(colors.section_header_fg)
        .add_modifier(ratatui::style::Modifier::BOLD);

    let mut lines = vec![table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '╭',
        '┬',
        '╮',
    )];
    lines.push(table_content_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        header,
        header_style,
        colors,
    ));
    lines.push(table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '├',
        '┼',
        '┤',
    ));
    for row in data_rows {
        lines.push(table_content_line(
            message,
            body_indent,
            body_indent_style,
            border_style,
            &widths,
            row,
            ratatui::style::Style::default().fg(colors.text),
            colors,
        ));
    }
    lines.push(table_border_line(
        message,
        body_indent,
        body_indent_style,
        border_style,
        &widths,
        '╰',
        '┴',
        '╯',
    ));
    lines
}

fn parse_markdown_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn table_border_line(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    border_style: ratatui::style::Style,
    widths: &[usize],
    left: char,
    middle: char,
    right: char,
) -> ResponseDisplayLine {
    let mut text = String::from(body_indent);
    text.push(left);
    for (index, width) in widths.iter().enumerate() {
        text.push_str(&"─".repeat(width + 2));
        if index + 1 < widths.len() {
            text.push(middle);
        }
    }
    text.push(right);
    ResponseDisplayLine {
        kind: message.kind,
        text: text.clone(),
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans: vec![
            StyledSpan {
                text: body_indent.to_string(),
                style: body_indent_style,
            },
            StyledSpan {
                text: text.trim_start_matches(body_indent).to_string(),
                style: border_style,
            },
        ],
    }
}

fn table_content_line(
    message: &Msg,
    body_indent: &str,
    body_indent_style: ratatui::style::Style,
    border_style: ratatui::style::Style,
    widths: &[usize],
    row: &[String],
    default_cell_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> ResponseDisplayLine {
    let mut text = String::from(body_indent);
    let mut spans = vec![StyledSpan {
        text: body_indent.to_string(),
        style: body_indent_style,
    }];
    for (index, width) in widths.iter().enumerate() {
        let cell = row.get(index).cloned().unwrap_or_default();
        let padded = format!(" {:width$} ", cell, width = width);
        text.push('│');
        text.push_str(&padded);
        spans.push(StyledSpan {
            text: "│".to_string(),
            style: border_style,
        });
        spans.push(StyledSpan {
            text: padded,
            style: table_cell_style(&cell, default_cell_style, colors),
        });
    }
    text.push('│');
    spans.push(StyledSpan {
        text: "│".to_string(),
        style: border_style,
    });

    ResponseDisplayLine {
        kind: message.kind,
        text,
        is_header: false,
        message_id: message.id.clone(),
        action: None,
        is_tool_line: false,
        tool_status: None,
        response_state: None,
        thinking_line_kind: None,
        spans,
    }
}

fn table_cell_style(
    cell: &str,
    default_style: ratatui::style::Style,
    colors: &omega_theme::RenderPalette,
) -> ratatui::style::Style {
    if looks_like_metric(cell) {
        ratatui::style::Style::default()
            .fg(colors.metric_emphasis_fg)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else if looks_like_code_token(cell) {
        ratatui::style::Style::default().fg(colors.code_fg)
    } else {
        default_style
    }
}

fn summarize_report_section(kind: ResponseCardSectionKind, body: &str) -> Option<String> {
    let non_empty_lines: Vec<&str> = body.lines().filter(|line| !line.trim().is_empty()).collect();
    if non_empty_lines.is_empty() {
        return None;
    }

    let bullet_count = non_empty_lines
        .iter()
        .filter(|line| is_bullet_or_ordered_item(line.trim_start()))
        .count();
    let table_rows = count_markdown_table_rows(body);
    let command_count = non_empty_lines
        .iter()
        .filter(|line| line.trim_start().starts_with('$') || line.contains('`'))
        .count();

    match kind {
        ResponseCardSectionKind::ResultsSummary
        | ResponseCardSectionKind::ChangesMade
        | ResponseCardSectionKind::KeyPoints
        | ResponseCardSectionKind::OptionalNextStep => Some(if bullet_count > 0 {
            format!("{} items", bullet_count)
        } else {
            format!("{} lines", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Verification => Some(if table_rows > 0 {
            format!("{} rows", table_rows)
        } else {
            format!("{} checks", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Usage => Some(if command_count > 0 {
            format!("{} commands", command_count)
        } else {
            format!("{} lines", non_empty_lines.len())
        }),
        ResponseCardSectionKind::Custom => Some(format!("{} lines", non_empty_lines.len())),
        _ => None,
    }
}

fn count_markdown_table_rows(body: &str) -> usize {
    let lines: Vec<&str> = body.lines().collect();
    let mut index = 0usize;
    let mut rows = 0usize;
    while index < lines.len() {
        if is_markdown_table_header(lines.as_slice(), index) {
            index += 2;
            while index < lines.len() && is_markdown_table_row(lines[index]) {
                rows += 1;
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    rows
}

fn is_bullet_or_ordered_item(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line
            .find(". ")
            .is_some_and(|index| index > 0 && line[..index].chars().all(|ch| ch.is_ascii_digit()))
}

fn looks_like_metric(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.contains('%')
        || trimmed.chars().any(|ch| ch.is_ascii_digit())
        || matches!(trimmed, "pass" | "passed" | "failed" | "complete" | "eliminated")
}

fn looks_like_code_token(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('/')
        || trimmed.contains("::")
        || trimmed.contains('.')
        || trimmed.contains('_')
        || trimmed.starts_with('$')
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

fn format_tool_lane_header(tool_runs: &[&ToolRun], can_toggle: bool, collapsed: bool) -> String {
    let running = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Running)
        .count();
    let failed = tool_runs
        .iter()
        .filter(|tool_run| tool_run.status == ToolRunStatus::Failed)
        .count();
    let total = tool_runs.len();

    let mut text = if running > 0 {
        format!("  tools  {total} total · {running} running")
    } else if failed > 0 {
        format!("  tools  {total} total · {failed} failed")
    } else {
        format!("  tools  {total} total")
    };
    if can_toggle {
        text.push_str(if collapsed { "  expand" } else { "  collapse" });
    }
    text
}

fn tool_name_width(tool_runs: &[&ToolRun]) -> usize {
    tool_runs
        .iter()
        .map(|tool_run| tool_run.tool_name.chars().count())
        .max()
        .unwrap_or(0)
}

fn format_tool_summary(tool_run: &ToolRun, name_width: usize) -> String {
    let mut summary = format!(
        "    {tool_name:<name_width$}  {status}  {invoke}",
        tool_name = tool_run.tool_name,
        status = tool_run_status_label(tool_run.status),
        invoke = tool_run.invocation_preview,
        name_width = name_width,
    );
    if let Some(result_preview) = tool_run.result_preview.as_deref() {
        summary.push_str(" -> ");
        summary.push_str(result_preview);
    }
    summary
}

fn format_document_knowledge_summary(summary: &ResponseDocumentKnowledge) -> String {
    let mut text = format!(
        "document  [{}]  {} hits",
        summary.readiness.as_str(),
        summary.result_count,
    );
    if !summary.query.trim().is_empty() {
        text.push_str(&format!("  ·  {}", truncate_preview(&summary.query, 28)));
    } else if let Some(query) = summary.planned_queries.first() {
        text.push_str(&format!("  ·  {}", truncate_preview(query, 28)));
    }
    if let Some(reason) = summary.reason.as_deref() {
        text.push_str(&format!("  ·  {reason}"));
    } else if let Some(hit) = summary.top_hits.first() {
        text.push_str(&format!("  ·  {}", truncate_preview(&hit.path, 28)));
    }
    text
}

fn format_memory_knowledge_summary(summary: &ResponseMemoryKnowledge) -> String {
    let mut text = format!(
        "memory  {} selected  ·  {} archived  ·  {} observations",
        summary.selected_summary_count,
        summary.memory_hit_count,
        summary.observation_hit_count,
    );
    if let Some(query) = summary.planned_queries.first() {
        text.push_str(&format!("  ·  {}", truncate_preview(query, 28)));
    } else if let Some(query) = summary.memory_query.as_deref() {
        text.push_str(&format!("  ·  {}", truncate_preview(query, 28)));
    } else if let Some(query) = summary.observation_query.as_deref() {
        text.push_str(&format!("  ·  {}", truncate_preview(query, 28)));
    } else if let Some(item) = summary.top_selected_summaries.first() {
        text.push_str(&format!("  ·  {}", truncate_preview(&item.title, 28)));
    }
    text
}

fn build_document_knowledge_detail_lines(summary: &ResponseDocumentKnowledge) -> Vec<String> {
    let mut lines = vec![format!("readiness: {}", summary.readiness.as_str())];
    if !summary.raw_query.trim().is_empty() {
        lines.push(format!("raw query: {}", summary.raw_query));
    }
    if !summary.planned_queries.is_empty() {
        lines.push(format!("planned queries: {}", summary.planned_queries.join(" | ")));
    }
    if let Some(reason) = summary.rewrite_reason.as_deref() {
        lines.push(format!("rewrite reason: {reason}"));
    }
    if !summary.rewrite_queries.is_empty() {
        lines.push(format!("rewrite queries: {}", summary.rewrite_queries.join(" | ")));
    }
    if let Some(path) = summary.recovery_path.as_deref() {
        lines.push(format!("recovery path: {path}"));
    }
    if !summary.query.trim().is_empty() {
        lines.push(format!("query: {}", summary.query));
    }
    lines.push(format!("mode: {}", summary.mode));
    if let Some(degraded_from) = summary.degraded_from.as_deref() {
        lines.push(format!("degraded from: {degraded_from}"));
    }
    lines.push(format!("result count: {}", summary.result_count));
    if let Some(reason) = summary.reason.as_deref() {
        lines.push(format!("reason: {reason}"));
    }

    if !summary.top_hits.is_empty() {
        lines.push(String::new());
        lines.push("top hits:".to_string());
        for hit in &summary.top_hits {
            lines.push(format!("- {}", hit.path));
            if !hit.preview.trim().is_empty() {
                lines.push(format!("  {}", hit.preview));
            }
        }
    }

    lines
}

fn build_memory_knowledge_detail_lines(summary: &ResponseMemoryKnowledge) -> Vec<String> {
    let mut lines = vec![format!("selected summaries: {}", summary.selected_summary_count)];
    if let Some(raw_query) = summary.raw_query.as_deref() {
        if !raw_query.trim().is_empty() {
            lines.push(format!("raw query: {raw_query}"));
        }
    }
    if !summary.planned_queries.is_empty() {
        lines.push(format!("planned queries: {}", summary.planned_queries.join(" | ")));
    }
    if let Some(reason) = summary.rewrite_reason.as_deref() {
        lines.push(format!("rewrite reason: {reason}"));
    }
    if !summary.rewrite_queries.is_empty() {
        lines.push(format!("rewrite queries: {}", summary.rewrite_queries.join(" | ")));
    }
    if let Some(path) = summary.recovery_path.as_deref() {
        lines.push(format!("recovery path: {path}"));
    }
    if let Some(query) = summary.memory_query.as_deref() {
        lines.push(format!("memory query: {query}"));
    }
    if let Some(query) = summary.observation_query.as_deref() {
        lines.push(format!("observation query: {query}"));
    }
    lines.push(format!("memory hit count: {}", summary.memory_hit_count));
    lines.push(format!(
        "observation hit count: {}",
        summary.observation_hit_count
    ));

    if !summary.top_selected_summaries.is_empty() {
        lines.push(String::new());
        lines.push("selected summaries:".to_string());
        for item in &summary.top_selected_summaries {
            lines.push(format!("- {}:{}  {}", item.workflow_id, item.step_id, item.title));
            if !item.preview.trim().is_empty() {
                lines.push(format!("  {}", item.preview));
            }
        }
    }

    if !summary.top_memory_hits.is_empty() {
        lines.push(String::new());
        lines.push("archived memory hits:".to_string());
        for item in &summary.top_memory_hits {
            lines.push(format!("- [{}] {}", item.profile, item.title));
            if !item.preview.trim().is_empty() {
                lines.push(format!("  {}", item.preview));
            }
        }
    }

    if !summary.top_observations.is_empty() {
        lines.push(String::new());
        lines.push("observations:".to_string());
        for item in &summary.top_observations {
            lines.push(format!("- {} [{}]", item.title, item.freshness.as_str()));
            if !item.summary.trim().is_empty() {
                lines.push(format!("  {}", item.summary));
            }
        }
    }

    lines
}

fn tool_run_status_label(status: ToolRunStatus) -> &'static str {
    match status {
        ToolRunStatus::Running => "◉",
        ToolRunStatus::Complete => "●",
        ToolRunStatus::Failed => "✕",
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().find(|line| !line.trim().is_empty())
}

fn summarize_skill_ids(skill_ids: &[String]) -> String {
    if skill_ids.is_empty() {
        "none".to_string()
    } else {
        skill_ids.join(", ")
    }
}

fn format_response_header(message: &Msg) -> String {
    let state = message.state.unwrap_or(ResponseSectionState::Complete);
    let badge = match message.kind {
        MsgKind::Routing => "route",
        MsgKind::Step => "step",
        MsgKind::FinalAnswer => "final",
        MsgKind::Command => "command",
        MsgKind::Thinking => "  reasoning",
        _ => "msg",
    };
    let title = match message.kind {
        MsgKind::Thinking => thinking_header_title(state),
        _ => message.title.as_deref().unwrap_or("Section"),
    };
    let state = match state {
        ResponseSectionState::Streaming => "◉",
        ResponseSectionState::Complete => "●",
        ResponseSectionState::Failed => "✕",
    };

    if message.kind == MsgKind::Command {
        let source = message.workflow_id.as_deref().unwrap_or("builtin");
        let toggle = if message.collapsed { "  expand" } else { "  collapse" };
        format!("{badge}  {source}  {title}  {state}{toggle}")
    } else {
        let workflow_role = message
            .workflow_role
            .map(WorkflowRunRole::as_str)
            .unwrap_or("unknown");
        let workflow_id = message.workflow_id.as_deref().unwrap_or("workflow");
        format!("{badge}  {workflow_role}:{workflow_id}  {title}  {state}")
    }
}

fn summarize_command_text(text: &str, state: ResponseSectionState) -> String {
    let preview = first_non_empty_line(text)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| truncate_preview(line, 28))
        .unwrap_or_else(|| command_placeholder_text(state).to_string());
    let line_count = text.lines().filter(|line| !line.trim().is_empty()).count();

    if line_count == 0 {
        preview
    } else if line_count == 1 {
        format!("1 line · {preview}")
    } else {
        format!("{line_count} lines · {preview}")
    }
}

fn command_placeholder_text(state: ResponseSectionState) -> &'static str {
    match state {
        ResponseSectionState::Streaming => "running command",
        ResponseSectionState::Complete => "command complete",
        ResponseSectionState::Failed => "command failed",
    }
}

fn summarize_thinking_text(text: &str, state: ResponseSectionState) -> String {
    let preview = first_non_empty_line(text)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| truncate_preview(line, 24))
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

fn visible_thinking_body_lines(text: &str, state: ResponseSectionState) -> Vec<String> {
    let lines = split_or_empty(text);
    if state != ResponseSectionState::Streaming || lines.len() <= 2 {
        return lines;
    }

    lines[lines.len().saturating_sub(2)..].to_vec()
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

fn thinking_body_prefix(state: ResponseSectionState, spinner_tick: u8) -> String {
    match state {
        ResponseSectionState::Streaming => {
            const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            SPINNER_FRAMES[(spinner_tick as usize / 2) % SPINNER_FRAMES.len()].to_string()
        }
        ResponseSectionState::Complete | ResponseSectionState::Failed => "│".to_string(),
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
