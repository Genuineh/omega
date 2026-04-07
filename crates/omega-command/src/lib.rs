use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmegaCommandSource {
    Builtin,
    ToolExtension,
}

impl OmegaCommandSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::ToolExtension => "tool-extension",
        }
    }
}

#[derive(Clone)]
pub struct OmegaCommandDescriptor {
    pub name: String,
    pub aliases: Vec<String>,
    pub argument_hint: Option<String>,
    pub subcommands: Vec<OmegaCommandSubcommand>,
    pub description: String,
    pub source: OmegaCommandSource,
    pub is_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmegaCommandSubcommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

impl OmegaCommandSubcommand {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        argument_hint: Option<&str>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            argument_hint: argument_hint.map(ToOwned::to_owned),
        }
    }
}

impl fmt::Debug for OmegaCommandDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OmegaCommandDescriptor")
            .field("name", &self.name)
            .field("aliases", &self.aliases)
            .field("argument_hint", &self.argument_hint)
            .field("subcommands", &self.subcommands)
            .field("description", &self.description)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl OmegaCommandDescriptor {
    pub fn new(
        name: impl Into<String>,
        aliases: Vec<String>,
        argument_hint: Option<&str>,
        subcommands: Vec<OmegaCommandSubcommand>,
        description: impl Into<String>,
        source: OmegaCommandSource,
        is_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            name: name.into(),
            aliases,
            argument_hint: argument_hint.map(ToOwned::to_owned),
            subcommands,
            description: description.into(),
            source,
            is_enabled,
        }
    }

    fn matches_name(&self, candidate: &str) -> bool {
        self.name == candidate || self.aliases.iter().any(|alias| alias == candidate)
    }

    fn resolve_subcommand(&self, candidate: &str) -> Option<OmegaCommandSubcommand> {
        self.subcommands
            .iter()
            .find(|name| name.name.as_str() == candidate)
            .cloned()
    }

    fn top_level_hint(&self) -> CommandHint {
        CommandHint {
            name: self.name.clone(),
            source: self.source,
            description: self.description.clone(),
            argument_hint: self.argument_hint.clone(),
            enabled: (self.is_enabled)(),
        }
    }

    fn subcommand_hint(&self, subcommand: &OmegaCommandSubcommand) -> CommandHint {
        CommandHint {
            name: subcommand.name.clone(),
            source: self.source,
            description: subcommand.description.clone(),
            argument_hint: subcommand.argument_hint.clone(),
            enabled: (self.is_enabled)(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmegaCommandInvocation {
    pub raw: String,
    pub name: String,
    pub subcommand: Option<String>,
    pub args: Vec<String>,
    pub source: OmegaCommandSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandHint {
    pub name: String,
    pub source: OmegaCommandSource,
    pub description: String,
    pub argument_hint: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandHintResolution {
    TopLevel(Vec<CommandHint>),
    Command {
        command: CommandHint,
        subcommands: Vec<CommandHint>,
    },
    Ready {
        command: CommandHint,
        subcommand: Option<CommandHint>,
        args: Vec<String>,
    },
    Disabled {
        command: CommandHint,
        subcommand: Option<CommandHint>,
    },
    NoMatch {
        input: String,
    },
}

pub trait CommandHintProvider: Send + Sync {
    fn resolve_hint(&self, input: &str) -> CommandHintResolution;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandParseError {
    MissingSlash,
    EmptyCommand,
    UnknownCommand { name: String },
    InvalidSubcommand {
        command: String,
        subcommand: String,
        expected: Vec<String>,
    },
}

impl fmt::Display for CommandParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSlash => write!(f, "command input must start with '/'"),
            Self::EmptyCommand => write!(f, "missing command name after '/'"),
            Self::UnknownCommand { name } => write!(f, "unknown command '/{name}'"),
            Self::InvalidSubcommand {
                command,
                subcommand,
                expected,
            } => write!(
                f,
                "invalid subcommand '{subcommand}' for '/{command}'; expected one of: {}",
                expected.join(", ")
            ),
        }
    }
}

impl std::error::Error for CommandParseError {}

#[derive(Debug, Clone, Default)]
pub struct OmegaCommandRegistry {
    descriptors: Vec<OmegaCommandDescriptor>,
}

impl OmegaCommandRegistry {
    pub fn new(descriptors: Vec<OmegaCommandDescriptor>) -> Self {
        Self { descriptors }
    }

    pub fn visible_commands(&self) -> Vec<&OmegaCommandDescriptor> {
        self.descriptors
            .iter()
            .filter(|descriptor| (descriptor.is_enabled)())
            .collect()
    }

    pub fn parse(&self, input: &str) -> Result<OmegaCommandInvocation, CommandParseError> {
        let trimmed = input.trim();
        let Some(stripped) = trimmed.strip_prefix('/') else {
            return Err(CommandParseError::MissingSlash);
        };
        let mut parts = stripped.split_whitespace();
        let Some(name) = parts.next() else {
            return Err(CommandParseError::EmptyCommand);
        };
        let descriptor = self
            .descriptors
            .iter()
            .find(|descriptor| descriptor.matches_name(name))
            .ok_or_else(|| CommandParseError::UnknownCommand {
                name: name.to_string(),
            })?;

        let mut remaining = parts.map(ToOwned::to_owned).collect::<Vec<_>>();
        let subcommand = if descriptor.subcommands.is_empty() || remaining.is_empty() {
            None
        } else {
            let candidate = remaining.remove(0);
            match descriptor.resolve_subcommand(&candidate) {
                Some(resolved) => Some(resolved),
                None => {
                    return Err(CommandParseError::InvalidSubcommand {
                        command: descriptor.name.clone(),
                        subcommand: candidate,
                        expected: descriptor
                            .subcommands
                            .iter()
                            .map(|subcommand| subcommand.name.clone())
                            .collect(),
                    });
                }
            }
        };

        Ok(OmegaCommandInvocation {
            raw: trimmed.to_string(),
            name: descriptor.name.clone(),
            subcommand: subcommand.map(|resolved| resolved.name),
            args: remaining,
            source: descriptor.source,
        })
    }

    pub fn all_commands(&self) -> Vec<CommandHint> {
        self.descriptors
            .iter()
            .map(OmegaCommandDescriptor::top_level_hint)
            .collect()
    }

    fn matching_commands(&self, candidate: &str) -> Vec<&OmegaCommandDescriptor> {
        self.descriptors
            .iter()
            .filter(|descriptor| {
                descriptor.name.starts_with(candidate)
                    || descriptor.aliases.iter().any(|alias| alias.starts_with(candidate))
            })
            .collect()
    }

    fn matching_subcommands(
        descriptor: &OmegaCommandDescriptor,
        candidate: &str,
    ) -> Vec<OmegaCommandSubcommand> {
        descriptor
            .subcommands
            .iter()
            .filter(|subcommand| subcommand.name.starts_with(candidate))
            .cloned()
            .collect()
    }
}

impl CommandHintProvider for OmegaCommandRegistry {
    fn resolve_hint(&self, input: &str) -> CommandHintResolution {
        let trimmed = input.trim();
        let Some(stripped) = trimmed.strip_prefix('/') else {
            return CommandHintResolution::NoMatch {
                input: trimmed.to_string(),
            };
        };

        if stripped.is_empty() {
            return CommandHintResolution::TopLevel(self.all_commands());
        }

        let tokens = stripped.split_whitespace().collect::<Vec<_>>();
        let command_token = tokens.first().copied().unwrap_or_default();
        let matching_commands = self.matching_commands(command_token);

        if matching_commands.is_empty() {
            return CommandHintResolution::NoMatch {
                input: stripped.to_string(),
            };
        }

        if matching_commands.len() > 1 {
            return CommandHintResolution::TopLevel(
                matching_commands
                    .into_iter()
                    .map(OmegaCommandDescriptor::top_level_hint)
                    .collect(),
            );
        }

        let descriptor = matching_commands[0];
        let command = descriptor.top_level_hint();
        if !command.enabled {
            return CommandHintResolution::Disabled {
                command,
                subcommand: None,
            };
        }

        if tokens.len() == 1 && !trimmed.ends_with(' ') {
            if descriptor.name == command_token || descriptor.aliases.iter().any(|alias| alias == command_token) {
                return CommandHintResolution::Command {
                    command,
                    subcommands: descriptor
                        .subcommands
                        .iter()
                        .map(|subcommand| descriptor.subcommand_hint(subcommand))
                        .collect(),
                };
            }

            return CommandHintResolution::TopLevel(vec![command]);
        }

        if descriptor.subcommands.is_empty() {
            return CommandHintResolution::Ready {
                command,
                subcommand: None,
                args: tokens.into_iter().skip(1).map(ToOwned::to_owned).collect(),
            };
        }

        let subcommand_token = tokens.get(1).copied().unwrap_or_default();
        if subcommand_token.is_empty() {
            return CommandHintResolution::Command {
                command,
                subcommands: descriptor
                    .subcommands
                    .iter()
                    .map(|subcommand| descriptor.subcommand_hint(subcommand))
                    .collect(),
            };
        }

        let matching_subcommands = Self::matching_subcommands(descriptor, subcommand_token);
        if matching_subcommands.is_empty() {
            return CommandHintResolution::NoMatch {
                input: stripped.to_string(),
            };
        }

        if matching_subcommands.len() > 1 {
            return CommandHintResolution::Command {
                command,
                subcommands: matching_subcommands
                    .iter()
                    .map(|subcommand| descriptor.subcommand_hint(subcommand))
                    .collect(),
            };
        }

        let subcommand = descriptor.subcommand_hint(&matching_subcommands[0]);
        if !subcommand.enabled {
            return CommandHintResolution::Disabled {
                command,
                subcommand: Some(subcommand),
            };
        }

        CommandHintResolution::Ready {
            command,
            subcommand: Some(subcommand),
            args: tokens.into_iter().skip(2).map(ToOwned::to_owned).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> OmegaCommandRegistry {
        OmegaCommandRegistry::new(vec![OmegaCommandDescriptor::new(
            "document",
            vec!["doc".to_string()],
            None,
            vec![
                OmegaCommandSubcommand::new("init", "Initialize document indexes", None),
                OmegaCommandSubcommand::new(
                    "query",
                    "Search indexed project documents",
                    Some("<text>"),
                ),
                OmegaCommandSubcommand::new("health", "Check repository document health", None),
                OmegaCommandSubcommand::new("sync", "Refresh document indexes", None),
                OmegaCommandSubcommand::new(
                    "create",
                    "Create a managed document from a template",
                    Some("<path> <doc_type> <title...>"),
                ),
                OmegaCommandSubcommand::new(
                    "archive",
                    "Archive an existing document",
                    Some("<path> [reason] [replaced_by]"),
                ),
                OmegaCommandSubcommand::new(
                    "list",
                    "List tracked documents",
                    Some("[doc_type] [status]"),
                ),
            ],
            "Manage indexed project documents",
            OmegaCommandSource::Builtin,
            Arc::new(|| true),
        )])
    }

    #[test]
    fn parses_document_subcommand() {
        let invocation = registry().parse("/document health").unwrap();
        assert_eq!(invocation.name, "document");
        assert_eq!(invocation.subcommand.as_deref(), Some("health"));
        assert!(invocation.args.is_empty());
        assert_eq!(invocation.source, OmegaCommandSource::Builtin);
    }

    #[test]
    fn resolves_aliases_to_canonical_name() {
        let invocation = registry().parse("/doc query parser contract").unwrap();
        assert_eq!(invocation.name, "document");
        assert_eq!(invocation.subcommand.as_deref(), Some("query"));
        assert_eq!(invocation.args, vec!["parser", "contract"]);
    }

    #[test]
    fn rejects_unknown_subcommand() {
        let error = registry().parse("/document nope").unwrap_err();
        assert!(matches!(
            error,
            CommandParseError::InvalidSubcommand { .. }
        ));
    }

    #[test]
    fn resolves_top_level_hint_for_slash_only() {
        let hint = registry().resolve_hint("/");
        match hint {
            CommandHintResolution::TopLevel(commands) => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].name, "document");
            }
            other => panic!("expected top-level hint, got {other:?}"),
        }
    }

    #[test]
    fn resolves_ready_hint_for_specific_subcommand() {
        let hint = registry().resolve_hint("/document query parser");
        match hint {
            CommandHintResolution::Ready {
                command,
                subcommand,
                args,
            } => {
                assert_eq!(command.name, "document");
                assert_eq!(subcommand.as_ref().map(|hint| hint.name.as_str()), Some("query"));
                assert_eq!(args, vec!["parser"]);
            }
            other => panic!("expected ready hint, got {other:?}"),
        }
    }
}