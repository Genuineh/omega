use std::collections::{BTreeMap, BTreeSet};

use omega_core::{CoreToolManifestMetadata, ToolDefinition};
use omega_workflow::StepToolRequest;

#[derive(Debug, Clone, PartialEq)]
struct ToolCatalogEntry {
    definition: ToolDefinition,
    manifest: CoreToolManifestMetadata,
}

impl ToolCatalogEntry {
    fn from_definition(definition: ToolDefinition) -> Self {
        let manifest = CoreToolManifestMetadata::legacy(
            definition.name.clone(),
            definition.description.clone(),
            definition.input_schema.clone(),
        );
        Self {
            definition,
            manifest,
        }
    }

    fn from_manifest(manifest: CoreToolManifestMetadata) -> Self {
        let definition = ToolDefinition {
            name: manifest.id.clone(),
            description: manifest.description.clone(),
            input_schema: manifest.input_schema.clone(),
        };
        Self {
            definition,
            manifest,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedToolSet {
    tool_names: Vec<String>,
    tool_definitions: Vec<ToolDefinition>,
    tool_manifests: Vec<CoreToolManifestMetadata>,
}

impl ResolvedToolSet {
    pub fn new(tool_definitions: Vec<ToolDefinition>) -> Self {
        let entries = tool_definitions
            .into_iter()
            .map(ToolCatalogEntry::from_definition)
            .collect();
        Self::from_entries(entries)
    }

    fn from_entries(entries: Vec<ToolCatalogEntry>) -> Self {
        let tool_names = entries
            .iter()
            .map(|entry| entry.definition.name.clone())
            .collect();
        let tool_definitions = entries
            .iter()
            .map(|entry| entry.definition.clone())
            .collect();
        let tool_manifests = entries
            .into_iter()
            .map(|entry| entry.manifest)
            .collect();
        Self {
            tool_names,
            tool_definitions,
            tool_manifests,
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

    pub fn tool_manifests(&self) -> &[CoreToolManifestMetadata] {
        &self.tool_manifests
    }

    pub fn is_empty(&self) -> bool {
        self.tool_definitions.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionToolCatalog {
    default_tool_names: BTreeSet<String>,
    available_tools: BTreeMap<String, ToolCatalogEntry>,
}

impl SessionToolCatalog {
    pub fn new(default_tools: Vec<ToolDefinition>) -> Self {
        let default_entries = default_tools
            .into_iter()
            .map(ToolCatalogEntry::from_definition)
            .collect::<Vec<_>>();
        Self::from_entries(default_entries.clone(), default_entries)
    }

    pub fn from_manifests(default_tools: Vec<CoreToolManifestMetadata>) -> Self {
        let default_entries = default_tools
            .into_iter()
            .map(ToolCatalogEntry::from_manifest)
            .collect::<Vec<_>>();
        Self::from_entries(default_entries.clone(), default_entries)
    }

    fn from_entries(
        default_tools: Vec<ToolCatalogEntry>,
        available_tools: Vec<ToolCatalogEntry>,
    ) -> Self {
        let default_tool_names = default_tools
            .iter()
            .map(|entry| entry.definition.name.clone())
            .collect::<BTreeSet<_>>();
        let available_tools = available_tools
            .into_iter()
            .map(|entry| (entry.definition.name.clone(), entry))
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
        let default_entries = default_tools
            .into_iter()
            .map(ToolCatalogEntry::from_definition)
            .collect::<Vec<_>>();
        let available_entries = available_tools
            .into_iter()
            .map(ToolCatalogEntry::from_definition)
            .collect::<Vec<_>>();
        Self::with_available_entries(default_entries, available_entries)
    }

    pub fn with_available_manifests(
        default_tools: Vec<CoreToolManifestMetadata>,
        available_tools: Vec<CoreToolManifestMetadata>,
    ) -> Self {
        let default_entries = default_tools
            .into_iter()
            .map(ToolCatalogEntry::from_manifest)
            .collect::<Vec<_>>();
        let available_entries = available_tools
            .into_iter()
            .map(ToolCatalogEntry::from_manifest)
            .collect::<Vec<_>>();
        Self::with_available_entries(default_entries, available_entries)
    }

    fn with_available_entries(
        default_tools: Vec<ToolCatalogEntry>,
        available_tools: Vec<ToolCatalogEntry>,
    ) -> Self {
        let default_tool_names = default_tools
            .iter()
            .map(|entry| entry.definition.name.clone())
            .collect::<BTreeSet<_>>();
        let mut all_available = available_tools
            .into_iter()
            .map(|entry| (entry.definition.name.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        for entry in default_tools {
            all_available.insert(entry.definition.name.clone(), entry);
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

        ResolvedToolSet::from_entries(
            tool_names
                .into_iter()
                .filter_map(|name| self.available_tools.get(&name).cloned())
                .collect(),
        )
    }

    pub fn available_tool_names(&self) -> Vec<String> {
        self.available_tools.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionToolCatalog;
    use omega_core::{CoreToolFamily, CoreToolManifestMetadata, ToolDefinition};
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
            vec![tool("bash"), tool("read_file"), tool("todo")],
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

    #[test]
    fn manifest_catalog_preserves_prompt_metadata() {
        let catalog = SessionToolCatalog::from_manifests(vec![CoreToolManifestMetadata {
            id: "search_codebase".to_string(),
            display_name: "Search Codebase".to_string(),
            description: "Search the indexed codebase".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            family: CoreToolFamily::KnowledgeAndGovernance,
            stability: omega_core::CoreToolStability::Preview,
            prompt: omega_core::CoreToolPromptProfile {
                summary: "Ranked search over project knowledge".to_string(),
                when_to_use: vec!["semantic retrieval matters".to_string()],
                when_not_to_use: vec!["exact file contents are needed".to_string()],
                prefer_over: vec!["grep_search".to_string()],
                fallback_to: vec!["read_file".to_string()],
                examples: vec!["search the codebase semantically".to_string()],
                anti_patterns: vec!["using search without reading the result".to_string()],
            },
            io: None,
            ui: None,
            context: None,
            permissions: None,
            storage: None,
            observability: None,
        }]);

        let resolved = catalog.resolve_for_step(&StepToolRequest::Inherit);
        assert_eq!(resolved.tool_names(), ["search_codebase"]);
        assert_eq!(resolved.tool_manifests()[0].family, CoreToolFamily::KnowledgeAndGovernance);
        assert_eq!(
            resolved.tool_manifests()[0].prompt.summary,
            "Ranked search over project knowledge"
        );
    }
}
