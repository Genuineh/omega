use std::collections::BTreeSet;

use omega_workflow::StepToolRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedToolSet {
    tool_names: Vec<String>,
}

impl ResolvedToolSet {
    pub fn new(tool_names: Vec<String>) -> Self {
        Self { tool_names }
    }

    pub fn tool_names(&self) -> &[String] {
        &self.tool_names
    }

    pub fn tool_name_refs(&self) -> Vec<&str> {
        self.tool_names.iter().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tool_names.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToolCatalog {
    default_tool_names: BTreeSet<String>,
    available_tool_names: BTreeSet<String>,
}

impl SessionToolCatalog {
    pub fn new(default_tool_names: Vec<String>) -> Self {
        let default_tool_names = default_tool_names.into_iter().collect::<BTreeSet<_>>();
        Self {
            available_tool_names: default_tool_names.clone(),
            default_tool_names,
        }
    }

    pub fn with_available_tools(
        default_tool_names: Vec<String>,
        available_tool_names: Vec<String>,
    ) -> Self {
        let default_tool_names = default_tool_names.into_iter().collect::<BTreeSet<_>>();
        let mut all_available = available_tool_names.into_iter().collect::<BTreeSet<_>>();
        all_available.extend(default_tool_names.iter().cloned());
        Self {
            default_tool_names,
            available_tool_names: all_available,
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
                        .filter(|name| self.available_tool_names.contains(name.as_str()))
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

        ResolvedToolSet::new(tool_names.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::SessionToolCatalog;
    use omega_workflow::StepToolRequest;

    fn catalog() -> SessionToolCatalog {
        SessionToolCatalog::with_available_tools(
            vec!["bash".to_string(), "read_file".to_string()],
            vec![
                "bash".to_string(),
                "read_file".to_string(),
                "todo".to_string(),
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
