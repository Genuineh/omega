mod builtin;
mod host;
mod manifest;

pub use host::{
    HookAdvanceDecision, HookAdvanceDenial, HookAdvanceOutcome, HookDiagnostic,
    HookDiagnosticLevel, HookDiagnosticRecord, HookDispatchInput, HookDispatchOutput,
    HookDispatchSummary, HookEventKind, HookHost, HookSession, HookSessionContextSnapshot,
    HookStepKey, HookStepSummarySnapshot, HookTodoSnapshot, HookToolCallSnapshot,
    HookToolResultSnapshot, HookWorkflowRole,
};
pub use manifest::{
    HookCatalog, HookManifest, HookManifestEntry, DEFAULT_HOOKS_DIR, DEFAULT_HOOK_API_VERSION,
    DEFAULT_HOOK_MANIFEST_FILE,
};
