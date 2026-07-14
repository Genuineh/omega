use std::path::{Path, PathBuf};

pub const OMEGA_CONFIG_DIR_NAME: &str = ".omega";
pub const OMEGA_STATE_DIR_NAME: &str = ".omega-state";
pub const OMEGA_CONFIG_DIR_PATH: &str = ".omega";
pub const OMEGA_STATE_DIR_PATH: &str = ".omega-state";
pub const PROJECT_MANIFEST_FILE: &str = "project.toml";
pub const PROJECT_STATE_FILE: &str = "project.json";
pub const PROJECT_MANIFEST_PATH: &str = ".omega/project.toml";
pub const PROJECT_STATE_PATH: &str = ".omega-state/project.json";
pub const LEGACY_PROJECT_STATE_PATH: &str = ".omega/project.json";
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const SESSION_RECORD_FILE: &str = "session.json";
pub const SESSION_CONTEXT_LEDGER_FILE: &str = "session.context.jsonl";
pub const SESSION_SNAPSHOT_SUFFIX: &str = ".snapshot.json";
pub const SESSION_REPLAY_LOG_SUFFIX: &str = ".log.jsonl";
pub const SESSIONS_DIR_PATH: &str = ".omega-state/sessions";
pub const MEMORY_DIR_NAME: &str = "memory";
pub const MEMORY_TURNS_DIR_NAME: &str = "turns";
pub const MEMORY_OBSERVATIONS_FILE: &str = "observations.jsonl";
pub const MEMORY_DIR_PATH: &str = ".omega-state/memory";
pub const MEMORY_TURNS_DIR_PATH: &str = ".omega-state/memory/turns";
pub const MEMORY_OBSERVATIONS_PATH: &str = ".omega-state/memory/observations.jsonl";
pub const STORE_DIR_NAME: &str = "store";
pub const STORE_MANIFEST_FILE: &str = "files.jsonl";
pub const STORE_TODOS_FILE: &str = "todos.jsonl";
pub const STORE_TANTIVY_DIR_NAME: &str = "tantivy";
pub const STORE_LANCE_DIR_NAME: &str = "lance";
pub const STORE_COMMIT_LOG_FILE: &str = "index-commit-log.json";
pub const STORE_VERSION_FILE: &str = "store-version.json";
pub const STORE_HISTORY_DIR_NAME: &str = "history";
pub const STORE_STAGING_DIR_NAME: &str = "staging";
pub const STORE_DIR_PATH: &str = ".omega-state/store";
pub const STORE_MANIFEST_PATH: &str = ".omega-state/store/files.jsonl";
pub const STORE_TODOS_PATH: &str = ".omega-state/store/todos.jsonl";
pub const STORE_TANTIVY_DIR_PATH: &str = ".omega-state/store/tantivy";
pub const STORE_LANCE_DIR_PATH: &str = ".omega-state/store/lance";
pub const STORE_COMMIT_LOG_PATH: &str = ".omega-state/store/index-commit-log.json";
pub const STORE_VERSION_PATH: &str = ".omega-state/store/store-version.json";
pub const STORE_HISTORY_DIR_PATH: &str = ".omega-state/store/history";
pub const STORE_STAGING_DIR_PATH: &str = ".omega-state/store/staging";
pub const HOOKS_DIR_NAME: &str = "hooks";
pub const HOOK_SOURCE_DIR_PATH: &str = ".omega/hooks";
pub const HOOK_ARTIFACTS_DIR_PATH: &str = ".omega-state/hooks";
pub const ENV_CONFIG_FILE: &str = "env.toml";
pub const MODEL_CONFIG_FILE: &str = "model.toml";
pub const KEYMAP_CONFIG_FILE: &str = "keymap.toml";
pub const THEME_CONFIG_FILE: &str = "theme.toml";
pub const TUI_CONFIG_FILE: &str = "tui.toml";
pub const SCENES_CONFIG_FILE: &str = "scenes.toml";
pub const LEGACY_WORKFLOW_CONFIG_FILE: &str = "workflow.toml";
pub const ENV_CONFIG_PATH: &str = ".omega/env.toml";
pub const MODEL_CONFIG_PATH: &str = ".omega/model.toml";
pub const KEYMAP_CONFIG_PATH: &str = ".omega/keymap.toml";
pub const THEME_CONFIG_PATH: &str = ".omega/theme.toml";
pub const TUI_CONFIG_PATH: &str = ".omega/tui.toml";
pub const SCENES_CONFIG_PATH: &str = ".omega/scenes.toml";
pub const LEGACY_WORKFLOW_CONFIG_PATH: &str = ".omega/workflow.toml";
pub const WORKFLOWS_DIR_NAME: &str = "workflows";
pub const PROMPT_DIR_NAME: &str = "prompt";
pub const STEP_PROMPT_DIR: &str = "step";
pub const SCHEMA_DIR_NAME: &str = "schema";
pub const STEP_SCHEMA_DIR: &str = "step";
pub const STOREIGNORE_FILE: &str = ".storeignore";
pub const DOC_RULES_FILE: &str = "doc-rules.toml";
pub const DOCS_DATA_DIR_NAME: &str = "docs-data";
pub const DOCS_DATA_MANIFEST_FILE: &str = "manifest.json";
pub const DOCS_DATA_RECORDS_DIR_NAME: &str = "records";
pub const DOCS_DATA_TASKS_DIR_NAME: &str = "tasks";
pub const DOCS_DATA_PROJECT_TASKS_FILE: &str = "project-tasks.jsonl";
pub const DOCS_DATA_PROJECT_PLAN_MANIFEST_FILE: &str = "project-plan.toml";
pub const DOCS_DATA_PROJECT_TASK_LOGS_DIR_NAME: &str = "logs";
pub const DOCS_DATA_RELATIONS_DIR_NAME: &str = "relations";
pub const DOCS_DATA_RENDER_DIR_NAME: &str = "render";
pub const DOCS_DATA_RENDER_STATE_FILE: &str = "render-state.json";
pub const DOCS_DATA_LINKS_FILE: &str = "links.jsonl";
pub const WORKFLOWS_DIR_PATH: &str = ".omega/workflows";
pub const PROMPT_DIR_PATH: &str = ".omega/prompt";
pub const STEP_PROMPT_DIR_PATH: &str = ".omega/prompt/step";
pub const SCHEMA_DIR_PATH: &str = ".omega/schema";
pub const STEP_SCHEMA_DIR_PATH: &str = ".omega/schema/step";
pub const STOREIGNORE_PATH: &str = ".omega/.storeignore";
pub const DOC_RULES_PATH: &str = ".omega/doc-rules.toml";
pub const DOCS_DATA_DIR_PATH: &str = "docs-data";
pub const DOCS_DATA_MANIFEST_PATH: &str = "docs-data/manifest.json";
pub const DOCS_DATA_RECORDS_DIR_PATH: &str = "docs-data/records";
pub const DOCS_DATA_TASKS_DIR_PATH: &str = "docs-data/tasks";
pub const DOCS_DATA_PROJECT_TASKS_PATH: &str = "docs-data/tasks/project-tasks.jsonl";
pub const DOCS_DATA_PROJECT_PLAN_MANIFEST_PATH: &str = "docs-data/tasks/project-plan.toml";
pub const DOCS_DATA_PROJECT_TASK_LOGS_DIR_PATH: &str = "docs-data/tasks/logs";
pub const DOCS_DATA_RELATIONS_DIR_PATH: &str = "docs-data/relations";
pub const DOCS_DATA_RENDER_DIR_PATH: &str = "docs-data/render";
pub const DOCS_DATA_RENDER_STATE_PATH: &str = "docs-data/render/render-state.json";
pub const DOCS_DATA_LINKS_PATH: &str = "docs-data/relations/links.jsonl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmegaProjectLayout {
    root: PathBuf,
}

impl OmegaProjectLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_root(&self) -> PathBuf {
        self.root.join(OMEGA_CONFIG_DIR_NAME)
    }

    pub fn state_root(&self) -> PathBuf {
        self.root.join(OMEGA_STATE_DIR_NAME)
    }

    pub fn legacy_state_root(&self) -> PathBuf {
        self.root.join(OMEGA_CONFIG_DIR_NAME)
    }

    pub fn project_manifest_path(&self) -> PathBuf {
        self.config_root().join(PROJECT_MANIFEST_FILE)
    }

    pub fn project_state_path(&self) -> PathBuf {
        self.state_root().join(PROJECT_STATE_FILE)
    }

    pub fn legacy_project_state_path(&self) -> PathBuf {
        self.legacy_state_root().join(PROJECT_STATE_FILE)
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.state_root().join(SESSIONS_DIR_NAME)
    }

    pub fn legacy_sessions_dir(&self) -> PathBuf {
        self.legacy_state_root().join(SESSIONS_DIR_NAME)
    }

    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    pub fn session_record_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join(SESSION_RECORD_FILE)
    }

    pub fn session_context_ledger_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(SESSION_CONTEXT_LEDGER_FILE)
    }

    pub fn legacy_session_record_path(&self, session_id: &str) -> PathBuf {
        self.legacy_sessions_dir()
            .join(format!("{session_id}.json"))
    }

    pub fn legacy_snapshot_path(&self, session_id: &str) -> PathBuf {
        self.legacy_sessions_dir()
            .join(format!("{session_id}{SESSION_SNAPSHOT_SUFFIX}"))
    }

    pub fn legacy_replay_log_path(&self, session_id: &str) -> PathBuf {
        self.legacy_sessions_dir()
            .join(format!("{session_id}{SESSION_REPLAY_LOG_SUFFIX}"))
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.state_root().join(MEMORY_DIR_NAME)
    }

    pub fn memory_turns_dir(&self) -> PathBuf {
        self.memory_dir().join(MEMORY_TURNS_DIR_NAME)
    }

    pub fn memory_observations_path(&self) -> PathBuf {
        self.memory_dir().join(MEMORY_OBSERVATIONS_FILE)
    }

    pub fn store_dir(&self) -> PathBuf {
        self.state_root().join(STORE_DIR_NAME)
    }

    pub fn store_manifest_path(&self) -> PathBuf {
        self.store_dir().join(STORE_MANIFEST_FILE)
    }

    pub fn store_todos_path(&self) -> PathBuf {
        self.store_dir().join(STORE_TODOS_FILE)
    }

    pub fn store_tantivy_dir(&self) -> PathBuf {
        self.store_dir().join(STORE_TANTIVY_DIR_NAME)
    }

    pub fn store_lance_dir(&self) -> PathBuf {
        self.store_dir().join(STORE_LANCE_DIR_NAME)
    }

    pub fn store_commit_log_path(&self) -> PathBuf {
        self.store_dir().join(STORE_COMMIT_LOG_FILE)
    }

    pub fn store_version_path(&self) -> PathBuf {
        self.store_dir().join(STORE_VERSION_FILE)
    }

    pub fn store_history_dir(&self) -> PathBuf {
        self.store_dir().join(STORE_HISTORY_DIR_NAME)
    }

    pub fn store_staging_dir(&self) -> PathBuf {
        self.store_dir().join(STORE_STAGING_DIR_NAME)
    }

    pub fn hook_source_dir(&self) -> PathBuf {
        self.config_root().join(HOOKS_DIR_NAME)
    }

    pub fn hook_artifacts_dir(&self) -> PathBuf {
        self.state_root().join(HOOKS_DIR_NAME)
    }

    pub fn env_config_path(&self) -> PathBuf {
        self.config_root().join(ENV_CONFIG_FILE)
    }

    pub fn model_config_path(&self) -> PathBuf {
        self.config_root().join(MODEL_CONFIG_FILE)
    }

    pub fn keymap_path(&self) -> PathBuf {
        self.config_root().join(KEYMAP_CONFIG_FILE)
    }

    pub fn theme_path(&self) -> PathBuf {
        self.config_root().join(THEME_CONFIG_FILE)
    }

    pub fn tui_config_path(&self) -> PathBuf {
        self.config_root().join(TUI_CONFIG_FILE)
    }

    pub fn scenes_path(&self) -> PathBuf {
        self.config_root().join(SCENES_CONFIG_FILE)
    }

    pub fn legacy_workflow_path(&self) -> PathBuf {
        self.config_root().join(LEGACY_WORKFLOW_CONFIG_FILE)
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.config_root().join(WORKFLOWS_DIR_NAME)
    }

    pub fn prompt_dir(&self) -> PathBuf {
        self.config_root().join(PROMPT_DIR_NAME)
    }

    pub fn step_prompt_dir(&self) -> PathBuf {
        self.prompt_dir().join(STEP_PROMPT_DIR)
    }

    pub fn schema_dir(&self) -> PathBuf {
        self.config_root().join(SCHEMA_DIR_NAME)
    }

    pub fn step_schema_dir(&self) -> PathBuf {
        self.schema_dir().join(STEP_SCHEMA_DIR)
    }

    pub fn docs_data_dir(&self) -> PathBuf {
        self.root.join(DOCS_DATA_DIR_NAME)
    }

    pub fn docs_data_manifest_path(&self) -> PathBuf {
        self.docs_data_dir().join(DOCS_DATA_MANIFEST_FILE)
    }

    pub fn docs_data_records_dir(&self) -> PathBuf {
        self.docs_data_dir().join(DOCS_DATA_RECORDS_DIR_NAME)
    }

    pub fn docs_data_tasks_dir(&self) -> PathBuf {
        self.docs_data_dir().join(DOCS_DATA_TASKS_DIR_NAME)
    }

    pub fn docs_data_relations_dir(&self) -> PathBuf {
        self.docs_data_dir().join(DOCS_DATA_RELATIONS_DIR_NAME)
    }

    pub fn docs_data_render_dir(&self) -> PathBuf {
        self.docs_data_dir().join(DOCS_DATA_RENDER_DIR_NAME)
    }

    pub fn docs_data_render_state_path(&self) -> PathBuf {
        self.docs_data_render_dir()
            .join(DOCS_DATA_RENDER_STATE_FILE)
    }

    pub fn docs_data_record_path(&self, record_set: &str) -> PathBuf {
        self.docs_data_records_dir()
            .join(format!("{record_set}.jsonl"))
    }

    pub fn docs_data_project_tasks_path(&self) -> PathBuf {
        self.docs_data_tasks_dir()
            .join(DOCS_DATA_PROJECT_TASKS_FILE)
    }

    pub fn docs_data_project_plan_manifest_path(&self) -> PathBuf {
        self.docs_data_tasks_dir()
            .join(DOCS_DATA_PROJECT_PLAN_MANIFEST_FILE)
    }

    pub fn docs_data_project_task_logs_dir(&self) -> PathBuf {
        self.docs_data_tasks_dir()
            .join(DOCS_DATA_PROJECT_TASK_LOGS_DIR_NAME)
    }

    pub fn docs_data_project_task_log_path(&self, task_id: &str) -> PathBuf {
        self.docs_data_project_task_logs_dir()
            .join(format!("{task_id}.jsonl"))
    }

    pub fn docs_data_links_path(&self) -> PathBuf {
        self.docs_data_relations_dir().join(DOCS_DATA_LINKS_FILE)
    }

    pub fn storeignore_path(&self) -> PathBuf {
        self.config_root().join(STOREIGNORE_FILE)
    }

    pub fn doc_rules_path(&self) -> PathBuf {
        self.config_root().join(DOC_RULES_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::OmegaProjectLayout;
    use std::path::PathBuf;

    #[test]
    fn resolves_config_and_state_roots_separately() {
        let root = PathBuf::from("/tmp/omega-layout-test");
        let layout = OmegaProjectLayout::new(root.clone());

        assert_eq!(layout.config_root(), root.join(".omega"));
        assert_eq!(layout.state_root(), root.join(".omega-state"));
        assert_eq!(
            layout.project_manifest_path(),
            root.join(".omega/project.toml")
        );
        assert_eq!(
            layout.project_state_path(),
            root.join(".omega-state/project.json")
        );
        assert_eq!(
            layout.legacy_project_state_path(),
            root.join(".omega/project.json")
        );
    }

    #[test]
    fn resolves_runtime_state_paths_under_state_root() {
        let root = PathBuf::from("/tmp/omega-layout-test");
        let layout = OmegaProjectLayout::new(root.clone());

        assert_eq!(
            layout.session_record_path("session-a"),
            root.join(".omega-state/sessions/session-a/session.json")
        );
        assert_eq!(
            layout.session_context_ledger_path("session-a"),
            root.join(".omega-state/sessions/session-a/session.context.jsonl")
        );
        assert_eq!(
            layout.memory_turns_dir(),
            root.join(".omega-state/memory/turns")
        );
        assert_eq!(layout.store_dir(), root.join(".omega-state/store"));
        assert_eq!(layout.hook_artifacts_dir(), root.join(".omega-state/hooks"));
    }

    #[test]
    fn resolves_structured_docs_paths_under_project_root() {
        let root = PathBuf::from("/tmp/omega-layout-test");
        let layout = OmegaProjectLayout::new(root.clone());

        assert_eq!(layout.docs_data_dir(), root.join("docs-data"));
        assert_eq!(
            layout.docs_data_manifest_path(),
            root.join("docs-data/manifest.json")
        );
        assert_eq!(
            layout.docs_data_record_path("specs"),
            root.join("docs-data/records/specs.jsonl")
        );
        assert_eq!(
            layout.docs_data_links_path(),
            root.join("docs-data/relations/links.jsonl")
        );
        assert_eq!(
            layout.docs_data_render_state_path(),
            root.join("docs-data/render/render-state.json")
        );
    }
}
