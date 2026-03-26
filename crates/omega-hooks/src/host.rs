use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use anyhow::{anyhow, bail, Context, Result};
use libloading::Library;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::builtin::invoke_builtin;
use crate::manifest::{HookCatalog, HookManifestEntry};

type HookStorage = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookWorkflowRole {
    Root,
    Child,
}

impl HookWorkflowRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Child => "child",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    BeforeStep,
    BeforeModelTurn,
    AfterModelTurn,
    BeforeToolCall,
    AfterToolCall,
    BeforeAdvance,
    AfterStep,
    StepFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl HookDiagnosticLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDiagnostic {
    pub level: HookDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookAdvanceDecision {
    Allow,
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookAdvanceDenial {
    pub hook_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HookAdvanceOutcome {
    #[default]
    Allow,
    Deny { reasons: Vec<HookAdvanceDenial> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookDiagnosticRecord {
    pub hook_id: String,
    pub level: HookDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookToolResultSnapshot {
    pub output_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookToolCallSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub tool_name: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<HookToolResultSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookTodoSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
    pub has_open_items: bool,
    pub rounds_without_update: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookStepSummarySnapshot {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub summary: String,
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookSessionContextSnapshot {
    pub latest_user_turn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recognized_scene_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_workflow_id: Option<String>,
    pub active_workflow_id: String,
    pub active_workflow_role: HookWorkflowRole,
    #[serde(default)]
    pub step_summaries: Vec<HookStepSummarySnapshot>,
    #[serde(default)]
    pub step_outputs: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookDispatchInput {
    pub event: HookEventKind,
    pub workflow_id: String,
    pub workflow_role: HookWorkflowRole,
    pub step_id: String,
    pub step_label: String,
    pub step_index: usize,
    pub step_total: usize,
    #[serde(default)]
    pub visible_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<HookToolCallSnapshot>,
    pub todo: HookTodoSnapshot,
    pub session_context: HookSessionContextSnapshot,
    #[serde(default)]
    pub storage: HookStorage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HookDispatchOutput {
    #[serde(default)]
    pub diagnostics: Vec<HookDiagnostic>,
    #[serde(default)]
    pub storage: HookStorage,
    #[serde(default)]
    pub advance: Option<HookAdvanceDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HookStepKey {
    pub workflow_id: String,
    pub workflow_role: HookWorkflowRole,
    pub step_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct HookSession {
    active_steps: BTreeSet<HookStepKey>,
    storage: BTreeMap<HookStepKey, BTreeMap<String, HookStorage>>,
}

impl HookSession {
    pub fn activate_step(&mut self, step_key: HookStepKey) -> bool {
        self.active_steps.insert(step_key)
    }

    pub fn is_step_active(&self, step_key: &HookStepKey) -> bool {
        self.active_steps.contains(step_key)
    }

    pub fn deactivate_step(&mut self, step_key: &HookStepKey) {
        self.active_steps.remove(step_key);
        self.storage.remove(step_key);
    }

    pub fn hook_storage(&self, step_key: &HookStepKey, hook_id: &str) -> Option<&HookStorage> {
        self.storage.get(step_key).and_then(|storage| storage.get(hook_id))
    }

    fn set_hook_storage(&mut self, step_key: &HookStepKey, hook_id: &str, storage: HookStorage) {
        self.storage
            .entry(step_key.clone())
            .or_default()
            .insert(hook_id.to_string(), storage);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookDispatchSummary {
    pub diagnostics: Vec<HookDiagnosticRecord>,
    pub advance: HookAdvanceOutcome,
}

#[derive(Debug, Clone)]
pub struct HookHost {
    catalog: HookCatalog,
}

impl HookHost {
    pub fn load(root: &std::path::Path) -> Result<Self> {
        Ok(Self {
            catalog: HookCatalog::load(root)?,
        })
    }

    pub fn catalog(&self) -> &HookCatalog {
        &self.catalog
    }

    pub fn start_session(&self) -> HookSession {
        HookSession::default()
    }

    pub fn dispatch(
        &self,
        session: &mut HookSession,
        step_key: &HookStepKey,
        hook_ids: &[String],
        input: HookDispatchInput,
    ) -> Result<HookDispatchSummary> {
        if hook_ids.is_empty() {
            return Ok(HookDispatchSummary::default());
        }

        let mut diagnostics = Vec::new();
        let mut denials = Vec::new();

        for hook_id in hook_ids {
            let mut hook_input = input.clone();
            hook_input.storage = session
                .hook_storage(step_key, hook_id)
                .cloned()
                .unwrap_or_default();

            let output = if let Some(entry) = self.catalog.manifest(hook_id) {
                invoke_hook(entry, &hook_input)
                    .with_context(|| format!("failed to dispatch hook '{}'", hook_id))?
            } else if let Some(output) = invoke_builtin(hook_id, &hook_input) {
                output
            } else {
                return Err(anyhow!(
                    "workflow step references missing hook manifest '{}'",
                    hook_id
                ));
            };
            session.set_hook_storage(step_key, hook_id, output.storage);

            diagnostics.extend(output.diagnostics.into_iter().map(|diagnostic| {
                HookDiagnosticRecord {
                    hook_id: hook_id.clone(),
                    level: diagnostic.level,
                    message: diagnostic.message,
                }
            }));

            if let Some(HookAdvanceDecision::Deny { reason }) = output.advance {
                denials.push(HookAdvanceDenial {
                    hook_id: hook_id.clone(),
                    reason,
                });
            }
        }

        if matches!(input.event, HookEventKind::AfterStep | HookEventKind::StepFailed) {
            session.deactivate_step(step_key);
        }

        Ok(HookDispatchSummary {
            diagnostics,
            advance: if denials.is_empty() {
                HookAdvanceOutcome::Allow
            } else {
                HookAdvanceOutcome::Deny { reasons: denials }
            },
        })
    }
}

fn invoke_hook(entry: &HookManifestEntry, input: &HookDispatchInput) -> Result<HookDispatchOutput> {
    if !entry.artifact_path.exists() {
        bail!(
            "hook artifact for '{}' is missing at {}",
            entry.manifest.id,
            entry.artifact_path.display()
        );
    }

    let request_json = serde_json::to_string(input)?;
    let request = CString::new(request_json)
        .map_err(|_| anyhow!("hook request for '{}' contained interior NUL", entry.manifest.id))?;

    unsafe {
        let library = Library::new(&entry.artifact_path).with_context(|| {
            format!(
                "failed to load hook artifact {}",
                entry.artifact_path.display()
            )
        })?;
        let api_version = library
            .get::<unsafe extern "C" fn() -> u32>(b"omega_hook_api_version\0")
            .with_context(|| {
                format!(
                    "hook '{}' is missing omega_hook_api_version",
                    entry.manifest.id
                )
            })?;
        let exported_version = api_version();
        if exported_version != entry.manifest.api_version {
            bail!(
                "hook '{}' api_version mismatch: manifest={}, artifact={}",
                entry.manifest.id,
                entry.manifest.api_version,
                exported_version
            );
        }

        let invoke = library
            .get::<unsafe extern "C" fn(*const c_char) -> *mut c_char>(
                b"omega_hook_invoke_json\0",
            )
            .with_context(|| {
                format!("hook '{}' is missing omega_hook_invoke_json", entry.manifest.id)
            })?;
        let free = library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"omega_hook_free_string\0")
            .with_context(|| {
                format!("hook '{}' is missing omega_hook_free_string", entry.manifest.id)
            })?;

        let response_ptr = invoke(request.as_ptr());
        if response_ptr.is_null() {
            bail!("hook '{}' returned a null response", entry.manifest.id);
        }

        let response_json = CStr::from_ptr(response_ptr)
            .to_str()
            .with_context(|| format!("hook '{}' returned non-utf8 response", entry.manifest.id))?
            .to_string();
        free(response_ptr);

        serde_json::from_str::<HookDispatchOutput>(&response_json).with_context(|| {
            format!("hook '{}' returned invalid response JSON", entry.manifest.id)
        })
    }
}