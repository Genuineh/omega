use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};
use omega_tools_builtin::{default_bash_allowed_commands, default_batch_max_requests};

use crate::constants::{
    CHAT_BLOCKED_GROUP, FEATURE_NON_EXECUTE_BLOCKED_GROUP, ROOT_ROUTING_BLOCKED_GROUP,
};

const KNOWN_TOOL_POLICY_TOOL_NAMES: &[&str] = &[
    "bash",
    "batch",
    "read_file",
    "list_dir",
    "glob_search",
    "grep_search",
    "web_search",
    "web_fetch",
    "apply_patch",
    "create_file",
    "edit_file",
    "todo",
    "todo_read",
    "todo_write",
    "ask_user_question",
    "task",
    "write_file",
    "load_skill",
    "manage_document",
    "search_codebase",
];

fn default_tool_policy_groups() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        (
            ROOT_ROUTING_BLOCKED_GROUP.to_string(),
            vec![
                "bash",
                "batch",
                "read_file",
                "list_dir",
                "glob_search",
                "grep_search",
                "web_search",
                "web_fetch",
                "apply_patch",
                "create_file",
                "edit_file",
                "todo",
                "todo_read",
                "todo_write",
                "ask_user_question",
                "task",
                "write_file",
                "load_skill",
                "manage_document",
                "search_codebase",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
        (
            CHAT_BLOCKED_GROUP.to_string(),
            vec![
                "apply_patch",
                "create_file",
                "edit_file",
                "todo",
                "todo_read",
                "todo_write",
                "ask_user_question",
                "write_file",
                "manage_document",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
        (
            FEATURE_NON_EXECUTE_BLOCKED_GROUP.to_string(),
            vec![
                "bash",
                "apply_patch",
                "create_file",
                "edit_file",
                "todo",
                "todo_read",
                "todo_write",
                "ask_user_question",
                "write_file",
                "manage_document",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
    ])
}

pub(crate) fn dedupe_preserve_order(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        if seen.insert(item.clone()) {
            deduped.push(item);
        }
    }
    deduped
}

pub(crate) fn normalize_allowed_commands(allowed_commands: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    let mut seen = BTreeSet::new();

    for command in allowed_commands {
        let command = command.trim().to_ascii_lowercase();
        if command.is_empty() {
            bail!("tools.bash.allowed_commands must not contain empty entries");
        }
        if seen.insert(command.clone()) {
            normalized.push(command);
        }
    }

    Ok(normalized)
}

pub(crate) fn normalize_tool_group(name: &str, items: Vec<String>) -> Result<Vec<String>> {
    let normalized_name = name.trim();
    if normalized_name.is_empty() {
        bail!("tools.groups keys must be non-empty");
    }

    let known = KNOWN_TOOL_POLICY_TOOL_NAMES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut normalized = Vec::new();
    let mut seen = BTreeSet::new();

    for item in items {
        let item = item.trim().to_string();
        if item.is_empty() {
            bail!("tools.groups.{normalized_name} must not contain empty tool names");
        }
        if !known.contains(item.as_str()) {
            bail!("tools.groups.{normalized_name} references unknown tool '{item}'");
        }
        if seen.insert(item.clone()) {
            normalized.push(item);
        }
    }

    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicyConfig {
    pub bash_allowed_commands: Vec<String>,
    pub batch_max_requests: usize,
    pub(crate) groups: BTreeMap<String, Vec<String>>,
}

impl ToolPolicyConfig {
    pub fn builtin_default() -> Self {
        Self {
            bash_allowed_commands: default_bash_allowed_commands(),
            batch_max_requests: default_batch_max_requests(),
            groups: default_tool_policy_groups(),
        }
    }

    pub fn group_items(&self, group_name: &str) -> Option<&[String]> {
        self.groups.get(group_name).map(Vec::as_slice)
    }

    pub(crate) fn resolve_groups(&self, group_names: &[String]) -> Result<Vec<String>> {
        let mut resolved = Vec::new();
        for group_name in group_names {
            let group_name = group_name.trim();
            if group_name.is_empty() {
                bail!("tool_request groups must not contain empty names");
            }
            let items = self
                .groups
                .get(group_name)
                .ok_or_else(|| anyhow::anyhow!("unknown tool policy group '{group_name}'"))?;
            resolved.extend(items.iter().cloned());
        }
        Ok(dedupe_preserve_order(resolved))
    }
}
