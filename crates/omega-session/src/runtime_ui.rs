use std::sync::{mpsc, Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUiEnvelope {
    Message {
        turn_id: u64,
        message: RuntimeUiMessage,
    },
    Effect {
        turn_id: u64,
        effect: RuntimeUiEffect,
    },
}

impl RuntimeUiEnvelope {
    pub fn message(turn_id: u64, message: RuntimeUiMessage) -> Self {
        Self::Message { turn_id, message }
    }

    pub fn effect(turn_id: u64, effect: RuntimeUiEffect) -> Self {
        Self::Effect { turn_id, effect }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUiMessage {
    pub target: UiTarget,
    pub source: UiSource,
    pub kind: UiMessageKind,
    pub content: UiContent,
    pub priority: Option<UiPriority>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUiEffect {
    SetStatusSlot {
        slot: StatusSlot,
        value: StatusValue,
    },
    ClearStatusSlot {
        slot: StatusSlot,
    },
    ReplacePanel {
        target: UiTarget,
        content: UiContent,
    },
    ShowOverlay(OverlayRequest),
    HideOverlay {
        target: OverlayTarget,
    },
    FocusHint {
        target: UiTarget,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiTarget {
    Response,
    Activity(ActivityTarget),
    Todo,
    StatusBar(StatusSlot),
    Overlay(OverlayTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTarget {
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSlot {
    Workflow,
    Agent,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusValue {
    Label(String),
    WorkflowStep {
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    Search,
    Confirm,
    Detail,
    Picker,
    InputPrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRequest {
    pub target: OverlayTarget,
    pub content: UiContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiSource {
    User,
    Assistant,
    WorkflowStep {
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    Tool {
        tool_name: String,
    },
    SkillLoader,
    Subagent {
        agent_id: String,
    },
    BackgroundTask {
        task_id: String,
    },
    MessageBus,
    Team,
    Worktree,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMessageKind {
    Narrative,
    Result,
    Log,
    Warning,
    Error,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiContent {
    Text(String),
}

impl UiContent {
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text(text) => text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPriority {
    Normal,
    Low,
    High,
}

pub trait RuntimeUiBridge: Send + Sync {
    fn send(&self, envelope: RuntimeUiEnvelope);
}

impl RuntimeUiBridge for mpsc::Sender<RuntimeUiEnvelope> {
    fn send(&self, envelope: RuntimeUiEnvelope) {
        let _ = mpsc::Sender::send(self, envelope);
    }
}

pub trait RuntimeUiSink {
    fn try_recv(&self) -> Option<RuntimeUiEnvelope>;
}

impl RuntimeUiSink for mpsc::Receiver<RuntimeUiEnvelope> {
    fn try_recv(&self) -> Option<RuntimeUiEnvelope> {
        self.try_recv().ok()
    }
}

pub struct SessionRuntimeContext {
    pub ui_bridge: Arc<dyn RuntimeUiBridge>,
}
