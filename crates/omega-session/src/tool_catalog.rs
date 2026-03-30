use std::collections::{BTreeMap, BTreeSet};

use omega_core::ToolDefinition;
use omega_workflow::StepToolRequest;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedToolSet {
    tool_names: Vec<String>,
    tool_definitions: Vec<ToolDefinition>,
}

impl ResolvedToolSet {
    pub fn new(tool_definitions: Vec<ToolDefinition>) -> Self {
        let tool_names = tool_definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect();
        Self {
            tool_names,
            tool_definitions,
        }
    }

    pub fn tool_names(&self) -> &[String] {
        &self.tool_names
    }

    pub fn tool_name_refs(&self) -> Vec<&str> {
        self.tool_definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect()
    }

    pub fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_definitions
    }

    pub fn is_empty(&self) -> bool {
        self.tool_definitions.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionToolCatalog {
    default_tool_names: BTreeSet<String>,
    available_tools: BTreeMap<String, ToolDefinition>,
}

impl SessionToolCatalog {
    pub fn new(default_tools: Vec<ToolDefinition>) -> Self {
        let default_tool_names = default_tools
            .iter()
            .map(|definition| definition.name.clone())
            .collect::<BTreeSet<_>>();
        let available_tools = default_tools
            .into_iter()
            .map(|definition| (definition.name.clone(), definition))
            .collect::<BTreeMap<_, _>>();
        Self {
            default_tool_names,
            available_tools,
        }
    }

    pub fn with_available_tools(
        default_tools: Vec<ToolDefinition>,
        available_tools: Vec<ToolDefinition>,
    ) -> Self {
        let default_tool_names = default_tools
            .iter()
            .map(|definition| definition.name.clone())
            .collect::<BTreeSet<_>>();
        let mut all_available = available_tools
            .into_iter()
            .map(|definition| (definition.name.clone(), definition))
            .collect::<BTreeMap<_, _>>();
        for definition in default_tools {
            all_available.insert(definition.name.clone(), definition);
        }
        Self {
            default_tool_names,
            available_tools: all_available,
        }
    }

    pub fn default_tool_names(&self) -> Vec<String> {
        self.default_tool_names.iter().cloned().collect()
    }

    pub fn resolve_for_step(&self, request: &StepToolRequest) -> ResolvedToolSet {
        let tool_names = match request {
            StepToolRequest::Inherit => self.default_tool_names.clone(),
            StepToolRequest::Extend(names) => {
                let mut resolved = self.default_tool_names.clone();
                resolved.extend(
                    names
                        .iter()
                        .filter(|name| self.available_tools.contains_key(name.as_str()))
                        .cloned(),
                );
                resolved
            }
            StepToolRequest::Block(names) => {
                let blocked = names.iter().map(String::as_str).collect::<BTreeSet<_>>();
                self.default_tool_names
                    .iter()
                    .filter(|name| !blocked.contains(name.as_str()))
                    .cloned()
                    .collect()
            }
        };

        ResolvedToolSet::new(
            tool_names
                .into_iter()
                .filter_map(|name| self.available_tools.get(&name).cloned())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::SessionToolCatalog;
    use omega_core::ToolDefinition;
    use omega_workflow::StepToolRequest;

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} description"),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn catalog() -> SessionToolCatalog {
        SessionToolCatalog::with_available_tools(
            vec![tool("bash"), tool("read_file")],
            vec![
                tool("bash"),
                tool("read_file"),
                tool("todo"),
            ],
        )
    }

    #[test]
    fn inherit_returns_default_tools() {
        let resolved = catalog().resolve_for_step(&StepToolRequest::Inherit);
        assert_eq!(resolved.tool_names(), ["bash", "read_file"]);
    }

    #[test]
    fn extend_adds_known_tools_only() {
        let resolved = catalog().resolve_for_step(&StepToolRequest::Extend(vec![
            "todo".to_string(),
            "missing".to_string(),
        ]));

        assert_eq!(resolved.tool_names(), ["bash", "read_file", "todo"]);
    }

    #[test]
    fn block_removes_only_default_tools() {
        let resolved = catalog().resolve_for_step(&StepToolRequest::Block(vec![
            "bash".to_string(),
            "todo".to_string(),
        ]));

        assert_eq!(resolved.tool_names(), ["read_file"]);
    }
}
