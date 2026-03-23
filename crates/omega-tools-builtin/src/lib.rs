use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::Result;
#[cfg(unix)]
use libc::{SIGKILL, kill};
use omega_tools::ToolHandler;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use tempfile::NamedTempFile;
use tracing::{debug, error, info, warn};
use wait_timeout::ChildExt;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_OUTPUT_CHARS: usize = 50_000;
const DEFAULT_ALLOWED_COMMANDS: &[&str] = &[
    "cat", "echo", "false", "find", "grep", "head", "ls", "printf", "pwd", "rg", "sleep", "tail",
    "touch", "tr", "true", "wait", "wc", "yes",
];
const DISALLOWED_SHELL_TOKENS: &[char] = &['$', '`', '\n', '\r'];
const FILE_PATH_COMMANDS: &[&str] = &[
    "cat", "find", "grep", "head", "ls", "rg", "tail", "touch", "wc",
];
const FIND_BLOCKED_ACTIONS: &[&str] = &[
    "-delete", "-exec", "-execdir", "-fprint", "-fprint0", "-fprintf", "-fls", "-ok", "-okdir",
];

pub fn default_bash_allowed_commands() -> Vec<String> {
    DEFAULT_ALLOWED_COMMANDS
        .iter()
        .map(|command| (*command).to_string())
        .collect()
}

#[derive(Debug, Clone)]
pub struct BashHandler {
    root: PathBuf,
    default_timeout_seconds: u64,
    allowed_commands: std::collections::BTreeSet<String>,
}

impl BashHandler {
    pub fn new(root: PathBuf) -> Self {
        Self::with_allowed_commands(root, default_bash_allowed_commands())
    }

    pub fn with_allowed_commands(root: PathBuf, allowed_commands: Vec<String>) -> Self {
        Self {
            root: Self::resolve_root(root),
            default_timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            allowed_commands: Self::normalize_allowed_commands(allowed_commands),
        }
    }

    pub fn with_timeout(root: PathBuf, default_timeout_seconds: u64) -> Self {
        Self::with_timeout_and_allowed_commands(
            root,
            default_timeout_seconds,
            default_bash_allowed_commands(),
        )
    }

    pub fn with_timeout_and_allowed_commands(
        root: PathBuf,
        default_timeout_seconds: u64,
        allowed_commands: Vec<String>,
    ) -> Self {
        Self {
            root: Self::resolve_root(root),
            default_timeout_seconds,
            allowed_commands: Self::normalize_allowed_commands(allowed_commands),
        }
    }

    fn normalize_allowed_commands(
        allowed_commands: Vec<String>,
    ) -> std::collections::BTreeSet<String> {
        allowed_commands
            .into_iter()
            .map(|command| command.trim().to_ascii_lowercase())
            .filter(|command| !command.is_empty())
            .collect()
    }

    fn resolve_root(root: PathBuf) -> PathBuf {
        let absolute = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(root)
        };

        std::fs::canonicalize(&absolute).unwrap_or_else(|_| Self::normalize_path(&absolute))
    }

    fn normalize_command_for_safety(command: &str) -> String {
        let mut normalized = String::with_capacity(command.len());
        let mut last_was_whitespace = false;

        for character in command.chars() {
            match character {
                '\'' | '"' => {}
                character if character.is_whitespace() => {
                    if !last_was_whitespace {
                        normalized.push(' ');
                        last_was_whitespace = true;
                    }
                }
                character => {
                    normalized.push(character.to_ascii_lowercase());
                    last_was_whitespace = false;
                }
            }
        }

        normalized.trim().to_string()
    }

    fn normalize_path(path: &Path) -> PathBuf {
        use std::path::Component;

        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    if let Some(Component::Normal(_)) = components.last() {
                        components.pop();
                    }
                }
                Component::CurDir => {}
                _ => components.push(component),
            }
        }

        components.iter().collect()
    }

    fn validate_path_within_root(&self, path_arg: &str) -> std::result::Result<(), String> {
        let candidate = Path::new(path_arg);
        let resolved = if candidate.is_absolute() {
            std::fs::canonicalize(candidate).unwrap_or_else(|_| Self::normalize_path(candidate))
        } else {
            let joined = self.root.join(candidate);
            std::fs::canonicalize(&joined).unwrap_or_else(|_| {
                if let Some(parent) = joined.parent() {
                    if let Ok(parent_canonical) = std::fs::canonicalize(parent) {
                        if let Some(name) = joined.file_name() {
                            return parent_canonical.join(name);
                        }
                        return parent_canonical;
                    }
                }

                Self::normalize_path(joined.as_path())
            })
        };
        let root = Self::resolve_root(self.root.clone());

        if !resolved.starts_with(&root) {
            return Err(format!(
                "Error: Path '{path_arg}' is outside the workspace root"
            ));
        }

        Ok(())
    }

    fn validate_segment(&self, segment: &str) -> std::result::Result<(), String> {
        let argv =
            shlex::split(segment).ok_or_else(|| "Error: Command parsing failed".to_string())?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "Error: Command cannot be empty".to_string())?;

        if !self.allowed_commands.contains(program.as_str()) {
            return Err(format!(
                "Error: Command '{program}' is blocked by safety policy"
            ));
        }

        if program == "find" {
            self.validate_find_args(args)?;
        }

        if FILE_PATH_COMMANDS.contains(&program.as_str()) {
            for arg in args {
                if arg.starts_with('-') {
                    continue;
                }
                self.validate_path_within_root(arg)?;
            }
        }

        Ok(())
    }

    fn validate_find_args(&self, args: &[String]) -> std::result::Result<(), String> {
        if args
            .iter()
            .any(|arg| FIND_BLOCKED_ACTIONS.contains(&arg.as_str()))
        {
            return Err("Error: Dangerous find action blocked".to_string());
        }

        Ok(())
    }

    fn validate_command(&self, input: &Value) -> std::result::Result<String, String> {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or_else(|| "Error: Missing required field 'command'".to_string())?;

        if command.is_empty() {
            return Err("Error: Command cannot be empty".to_string());
        }

        if command
            .chars()
            .any(|character| DISALLOWED_SHELL_TOKENS.contains(&character))
        {
            return Err("Error: Shell expansion is not allowed".to_string());
        }

        if command.contains('>') || command.contains('<') {
            return Err("Error: Shell redirection is not allowed".to_string());
        }

        let normalized_command = Self::normalize_command_for_safety(command);
        if normalized_command.contains("rm -rf") || normalized_command.contains("rm -fr") {
            return Err("Error: Dangerous command blocked".to_string());
        }

        for segment in normalized_command
            .split(['|', '&', ';', '(', ')'])
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
        {
            self.validate_segment(segment)?;
        }

        Ok(command.to_string())
    }

    fn timeout_seconds(&self, input: &Value) -> std::result::Result<u64, String> {
        match input.get("timeout") {
            None => Ok(self.default_timeout_seconds),
            Some(Value::Number(number)) => number
                .as_u64()
                .filter(|timeout| *timeout > 0)
                .ok_or_else(|| "Error: Timeout must be a positive integer".to_string()),
            Some(_) => Err("Error: Timeout must be a positive integer".to_string()),
        }
    }

    fn truncate_output(output: String) -> String {
        if output.chars().count() <= MAX_OUTPUT_CHARS {
            return output;
        }

        output.chars().take(MAX_OUTPUT_CHARS).collect()
    }

    fn read_output_file(path: &Path) -> Vec<u8> {
        fs::read(path).unwrap_or_default()
    }

    fn format_output(stdout: &[u8], stderr: &[u8]) -> String {
        let mut output = String::from_utf8_lossy(stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(stderr));
        let output = output.trim().to_string();

        if output.is_empty() {
            return "(no output)".to_string();
        }

        if output.chars().count() > MAX_OUTPUT_CHARS {
            return Self::truncate_output(output);
        }

        output
    }

    fn build_command(
        &self,
        command: &str,
        stdout_handle: std::fs::File,
        stderr_handle: std::fs::File,
    ) -> Command {
        let mut process = Command::new("sh");
        process
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .stdout(Stdio::from(stdout_handle))
            .stderr(Stdio::from(stderr_handle));

        #[cfg(unix)]
        process.process_group(0);

        process
    }

    fn run_command(&self, command: &str, timeout_seconds: u64) -> String {
        let stdout_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => return format!("Error: Failed to create stdout capture file: {error}"),
        };
        let stderr_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => return format!("Error: Failed to create stderr capture file: {error}"),
        };

        let stdout_path = stdout_file.path().to_path_buf();
        let stderr_path = stderr_file.path().to_path_buf();

        let stdout_handle = match stdout_file.reopen() {
            Ok(file) => file,
            Err(error) => return format!("Error: Failed to reopen stdout capture file: {error}"),
        };
        let stderr_handle = match stderr_file.reopen() {
            Ok(file) => file,
            Err(error) => return format!("Error: Failed to reopen stderr capture file: {error}"),
        };

        let mut child = match self
            .build_command(command, stdout_handle, stderr_handle)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return format!("Error: Failed to start command: {error}"),
        };

        let timeout = Duration::from_secs(timeout_seconds);
        match child.wait_timeout(timeout) {
            Ok(Some(_status)) => match child.wait() {
                Ok(_) => {
                    let stdout = Self::read_output_file(stdout_path.as_path());
                    let stderr = Self::read_output_file(stderr_path.as_path());
                    Self::format_output(&stdout, &stderr)
                }
                Err(error) => format!("Error: Failed to collect command output: {error}"),
            },
            Ok(None) => {
                self.terminate_child(&mut child);
                let _ = child.wait();
                format!("Error: Timeout ({timeout_seconds}s)")
            }
            Err(error) => {
                self.terminate_child(&mut child);
                let _ = child.wait();
                format!("Error: Failed to wait for command: {error}")
            }
        }
    }

    #[cfg(unix)]
    fn terminate_child(&self, child: &mut Child) {
        let process_group_id = -(child.id() as i32);
        unsafe {
            let _ = kill(process_group_id, SIGKILL);
        }
        let _ = child.kill();
    }

    #[cfg(not(unix))]
    fn terminate_child(&self, child: &mut Child) {
        let _ = child.kill();
    }
}

impl ToolHandler for BashHandler {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute from the workspace root. Pipes and compound shell forms are supported for an allowlisted command set, while shell expansion, redirection, non-allowlisted commands, and paths outside the workspace root are blocked for safety."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in seconds",
                    "minimum": 1
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        // Log command execution at info level
        let command_for_log = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("(missing)");
        info!(bash.command = %command_for_log);

        let command = match self.validate_command(&input) {
            Ok(command) => command,
            Err(message) => {
                warn!(bash.blocked_reason = %message, bash.command = %command_for_log);
                return Ok(message);
            }
        };

        let timeout_seconds = match self.timeout_seconds(&input) {
            Ok(timeout_seconds) => timeout_seconds,
            Err(message) => {
                warn!(bash.blocked_reason = %message);
                return Ok(message);
            }
        };

        let result = self.run_command(&command, timeout_seconds);

        // Log timeout as error
        if result.contains("Timeout") {
            let timeout_val = result
                .strip_prefix("Error: Timeout (")
                .and_then(|s| s.strip_suffix("s)"))
                .unwrap_or("unknown");
            error!(bash.timeout_seconds = %timeout_val, bash.command = %command);
        }

        // Log output truncation at debug level
        if result.chars().count() >= MAX_OUTPUT_CHARS {
            debug!(
                bash.output_truncated = true,
                bash.max_chars = MAX_OUTPUT_CHARS
            );
        }

        Ok(result)
    }
}

// ── Shared path safety ─────────────────────────────────────────────────────

fn resolve_file_root(root: PathBuf) -> PathBuf {
    let absolute = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(root)
    };
    std::fs::canonicalize(&absolute).unwrap_or_else(|_| normalize_file_path(&absolute))
}

fn normalize_file_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut components: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                }
            }
            Component::CurDir => {}
            _ => components.push(component),
        }
    }
    components.iter().collect()
}

/// Resolve `path_arg` within `root` and verify it does not escape.
///
/// Works for both existing files and paths not yet created (e.g. write targets).
fn safe_path_within_root(root: &Path, path_arg: &str) -> std::result::Result<PathBuf, String> {
    if path_arg.is_empty() {
        return Err("Error: Path cannot be empty".to_string());
    }
    let candidate = Path::new(path_arg);
    let resolved = if candidate.is_absolute() {
        std::fs::canonicalize(candidate).unwrap_or_else(|_| normalize_file_path(candidate))
    } else {
        let joined = root.join(candidate);
        std::fs::canonicalize(&joined).unwrap_or_else(|_| {
            // File does not yet exist — canonicalize parent and re-attach filename.
            if let Some(parent) = joined.parent() {
                if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
                    if let Some(name) = joined.file_name() {
                        return canonical_parent.join(name);
                    }
                }
            }
            normalize_file_path(&joined)
        })
    };
    if !resolved.starts_with(root) {
        return Err(format!(
            "Error: Path '{path_arg}' is outside the workspace root"
        ));
    }
    Ok(resolved)
}

// ── ReadHandler ────────────────────────────────────────────────────────────

const MAX_READ_CHARS: usize = 50_000;

/// Reads file contents within the workspace root.
///
/// Mirrors `run_read` from `learn-claude-code/agents/s02_tool_use.py`.
#[derive(Debug, Clone)]
pub struct ReadHandler {
    root: PathBuf,
}

impl ReadHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }
}

impl ToolHandler for ReadHandler {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file contents."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace root."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum number of lines to return.",
                    "minimum": 1
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        let path_arg = match input.get("path").and_then(Value::as_str).map(str::trim) {
            Some(p) if !p.is_empty() => p,
            _ => return Ok("Error: Missing required field 'path'".to_string()),
        };
        info!(read_file.path = %path_arg);

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(p) => p,
            Err(msg) => {
                warn!(read_file.blocked_reason = %msg, read_file.path = %path_arg);
                return Ok(msg);
            }
        };

        let text = match fs::read_to_string(&resolved) {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("Error: {e}");
                warn!(read_file.error = %e, read_file.path = %path_arg);
                return Ok(msg);
            }
        };

        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let result = if let Some(limit) = limit {
            let lines: Vec<&str> = text.lines().collect();
            if limit < lines.len() {
                let extra = lines.len() - limit;
                let truncated = lines[..limit].join("\n");
                debug!(read_file.limited = true, read_file.omitted_lines = extra);
                format!("{truncated}\n... ({extra} more lines)")
            } else {
                text
            }
        } else {
            text
        };

        let result = if result.chars().count() > MAX_READ_CHARS {
            debug!(
                read_file.truncated = true,
                read_file.max_chars = MAX_READ_CHARS
            );
            result.chars().take(MAX_READ_CHARS).collect()
        } else {
            result
        };

        info!(read_file.bytes = result.len());
        Ok(result)
    }
}

// ── WriteHandler ───────────────────────────────────────────────────────────

/// Writes (or overwrites) a file within the workspace root, creating parent
/// directories as needed.
///
/// Mirrors `run_write` from `learn-claude-code/agents/s02_tool_use.py`.
#[derive(Debug, Clone)]
pub struct WriteHandler {
    root: PathBuf,
}

impl WriteHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }
}

impl ToolHandler for WriteHandler {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to write. Parent directories are created if they do not exist."
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        let path_arg = match input.get("path").and_then(Value::as_str).map(str::trim) {
            Some(p) if !p.is_empty() => p,
            _ => return Ok("Error: Missing required field 'path'".to_string()),
        };
        let content = match input.get("content").and_then(Value::as_str) {
            Some(c) => c,
            None => return Ok("Error: Missing required field 'content'".to_string()),
        };
        info!(write_file.path = %path_arg, write_file.bytes = content.len());

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(p) => p,
            Err(msg) => {
                warn!(write_file.blocked_reason = %msg, write_file.path = %path_arg);
                return Ok(msg);
            }
        };

        if let Some(parent) = resolved.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                let msg = format!("Error: {e}");
                warn!(write_file.error = %e, write_file.path = %path_arg);
                return Ok(msg);
            }
        }

        if let Err(e) = fs::write(&resolved, content) {
            let msg = format!("Error: {e}");
            warn!(write_file.error = %e, write_file.path = %path_arg);
            return Ok(msg);
        }

        Ok(format!("Wrote {} bytes to {path_arg}", content.len()))
    }
}

// ── EditHandler ────────────────────────────────────────────────────────────

/// Replaces the first occurrence of `old_text` with `new_text` in a file.
///
/// Mirrors `run_edit` from `learn-claude-code/agents/s02_tool_use.py`.
#[derive(Debug, Clone)]
pub struct EditHandler {
    root: PathBuf,
}

impl EditHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }
}

impl ToolHandler for EditHandler {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace exact text in file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to edit."
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find and replace. Must appear in the file."
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text."
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        let path_arg = match input.get("path").and_then(Value::as_str).map(str::trim) {
            Some(p) if !p.is_empty() => p,
            _ => return Ok("Error: Missing required field 'path'".to_string()),
        };
        let old_text = match input.get("old_text").and_then(Value::as_str) {
            Some(t) => t,
            None => return Ok("Error: Missing required field 'old_text'".to_string()),
        };
        let new_text = match input.get("new_text").and_then(Value::as_str) {
            Some(t) => t,
            None => return Ok("Error: Missing required field 'new_text'".to_string()),
        };
        info!(edit_file.path = %path_arg);

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(p) => p,
            Err(msg) => {
                warn!(edit_file.blocked_reason = %msg, edit_file.path = %path_arg);
                return Ok(msg);
            }
        };

        let content = match fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("Error: {e}");
                warn!(edit_file.error = %e, edit_file.path = %path_arg);
                return Ok(msg);
            }
        };

        if !content.contains(old_text) {
            warn!(edit_file.not_found = true, edit_file.path = %path_arg);
            return Ok(format!("Error: Text not found in {path_arg}"));
        }

        let new_content = content.replacen(old_text, new_text, 1);
        if let Err(e) = fs::write(&resolved, &new_content) {
            let msg = format!("Error: {e}");
            warn!(edit_file.error = %e, edit_file.path = %path_arg);
            return Ok(msg);
        }

        info!(edit_file.completed = true);
        Ok(format!("Edited {path_arg}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("omega-bash-handler-{nanos}"));
        fs::create_dir_all(&dir).expect("test temp dir should be created");
        dir
    }

    fn remove_dir_if_exists(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp dir should be removed");
        }
    }

    #[test]
    fn bash_handler_exposes_expected_metadata() {
        let handler = BashHandler::new(PathBuf::from("."));
        assert_eq!(handler.name(), "bash");
        assert_eq!(handler.description(), "Run a shell command.");

        let schema = handler.input_schema();
        assert_eq!(schema["required"][0], "command");
        assert_eq!(schema["properties"]["command"]["type"], "string");
        assert_eq!(schema["properties"]["timeout"]["minimum"], 1);
    }

    #[test]
    fn bash_handler_runs_command_in_root_directory() {
        let root = unique_test_dir();
        fs::write(root.join("hello.txt"), "hello world").expect("test file should be created");

        let handler = BashHandler::new(root.clone());
        let result = handler
            .execute(json!({"command": "cat hello.txt"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "hello world");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn bash_handler_allows_grep_and_find_by_default() {
        let root = unique_test_dir();
        fs::create_dir_all(root.join("nested")).expect("nested dir should be created");
        fs::write(root.join("nested/hello.txt"), "hello world")
            .expect("test file should be created");

        let handler = BashHandler::new(root.clone());
        let grep_result = handler
            .execute(json!({"command": "grep hello nested/hello.txt"}))
            .expect("tool execution should succeed");
        let find_result = handler
            .execute(json!({"command": "find nested -name hello.txt"}))
            .expect("tool execution should succeed");

        assert_eq!(grep_result, "hello world");
        assert_eq!(find_result, "nested/hello.txt");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn bash_handler_blocks_dangerous_command() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "rm -rf /tmp/foo"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Dangerous command blocked");
    }

    #[test]
    fn bash_handler_blocks_quoted_dangerous_command_bypass() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "r''m -r''f /tmp/foo"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Dangerous command blocked");
    }

    #[test]
    fn bash_handler_blocks_shell_expansion_bypass() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "r$(printf m) -rf /tmp/foo"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Shell expansion is not allowed");
    }

    #[test]
    fn bash_handler_blocks_redirection() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "printf hi > out.txt"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Shell redirection is not allowed");
    }

    #[test]
    fn bash_handler_blocks_dangerous_find_actions() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "find . -exec pwd {} +"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Dangerous find action blocked");
    }

    #[test]
    fn bash_handler_blocks_paths_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "cat /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(
            result,
            "Error: Path '/etc/passwd' is outside the workspace root"
        );
    }

    #[test]
    fn bash_handler_blocks_wc_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "wc /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(
            result,
            "Error: Path '/etc/passwd' is outside the workspace root"
        );
    }

    #[test]
    fn bash_handler_blocks_rg_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "rg root /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(
            result,
            "Error: Path '/etc/passwd' is outside the workspace root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bash_handler_blocks_symlink_escape() {
        let root = unique_test_dir();
        let outside_dir = unique_test_dir();
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret").expect("outside file should be created");

        let link_path = root.join("escape.txt");
        std::os::unix::fs::symlink(&outside_file, &link_path).expect("symlink should be created");

        let handler = BashHandler::new(root.clone());
        let result = handler
            .execute(json!({"command": "cat escape.txt"}))
            .expect("tool execution should succeed with error string");

        assert!(result.contains("outside the workspace root"));

        remove_dir_if_exists(&root);
        remove_dir_if_exists(&outside_dir);
    }

    #[test]
    fn bash_handler_times_out_long_running_command() {
        let handler = BashHandler::with_timeout(PathBuf::from("."), 1);
        let result = handler
            .execute(json!({"command": "sleep 2"}))
            .expect("tool execution should succeed with timeout string");

        assert_eq!(result, "Error: Timeout (1s)");
    }

    #[cfg(unix)]
    #[test]
    fn bash_handler_timeout_kills_descendants() {
        let root = unique_test_dir();
        let marker = root.join("marker.txt");
        let handler = BashHandler::with_timeout(root.clone(), 1);
        let command = format!("(sleep 3; touch {}) & wait", marker.display());

        let result = handler
            .execute(json!({"command": command}))
            .expect("tool execution should succeed with timeout string");

        assert_eq!(result, "Error: Timeout (1s)");
        std::thread::sleep(Duration::from_secs(3));
        assert!(!marker.exists(), "background descendant survived timeout");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn bash_handler_returns_no_output_marker() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "true"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "(no output)");
    }

    #[test]
    fn bash_handler_truncates_large_output() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "yes a | head -n 60000 | tr -d '\\n'"}))
            .expect("tool execution should succeed");

        assert_eq!(result.chars().count(), MAX_OUTPUT_CHARS);
        assert!(result.chars().all(|character| character == 'a'));
    }

    #[test]
    fn bash_handler_truncates_utf8_output_without_panicking() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "yes 你 | head -n 60000 | tr -d '\\n'"}))
            .expect("tool execution should succeed");

        assert_eq!(result.chars().count(), MAX_OUTPUT_CHARS);
        assert!(result.chars().all(|character| character == '你'));
    }

    #[test]
    fn bash_handler_reports_missing_command() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({}))
            .expect("tool execution should succeed");

        assert_eq!(result, "Error: Missing required field 'command'");
    }

    #[test]
    fn bash_handler_rejects_invalid_timeout_values() {
        let handler = BashHandler::new(PathBuf::from("."));

        let zero_timeout = handler
            .execute(json!({"command": "echo ok", "timeout": 0}))
            .expect("tool execution should succeed with validation string");
        assert_eq!(zero_timeout, "Error: Timeout must be a positive integer");

        let string_timeout = handler
            .execute(json!({"command": "echo ok", "timeout": "fast"}))
            .expect("tool execution should succeed with validation string");
        assert_eq!(string_timeout, "Error: Timeout must be a positive integer");
    }

    #[test]
    fn bash_handler_respects_explicit_timeout_argument() {
        let handler = BashHandler::with_timeout(PathBuf::from("."), 30);
        let result = handler
            .execute(json!({"command": "sleep 2", "timeout": 1}))
            .expect("tool execution should succeed with timeout string");

        assert_eq!(result, "Error: Timeout (1s)");
    }

    #[test]
    fn bash_handler_keeps_stderr_output() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "cat does-not-exist.txt"}))
            .expect("tool execution should succeed");

        assert!(result.contains("does-not-exist.txt"));
    }

    #[test]
    fn bash_handler_supports_pipes() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "printf hello | wc -c"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "5");
    }

    #[test]
    fn bash_handler_respects_custom_allowlist() {
        let handler = BashHandler::with_allowed_commands(
            PathBuf::from("."),
            vec!["printf".to_string(), "wc".to_string()],
        );

        let allowed = handler
            .execute(json!({"command": "printf hello | wc -c"}))
            .expect("tool execution should succeed");
        let blocked = handler
            .execute(json!({"command": "ls"}))
            .expect("tool execution should succeed with validation string");

        assert_eq!(allowed, "5");
        assert_eq!(blocked, "Error: Command 'ls' is blocked by safety policy");
    }

    #[test]
    fn bash_handler_trims_surrounding_whitespace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "printf '  hello  '"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "hello");
    }

    // ── ReadHandler tests ──────────────────────────────────────────────────

    #[test]
    fn read_handler_exposes_expected_metadata() {
        let handler = ReadHandler::new(PathBuf::from("."));
        assert_eq!(handler.name(), "read_file");
        assert_eq!(handler.description(), "Read file contents.");

        let schema = handler.input_schema();
        assert_eq!(schema["required"][0], "path");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert_eq!(schema["properties"]["limit"]["minimum"], 1);
    }

    #[test]
    fn read_handler_reads_file_contents() {
        let root = unique_test_dir();
        fs::write(root.join("hello.txt"), "hello world").expect("test file should be created");

        let handler = ReadHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "hello.txt"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "hello world");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn read_handler_respects_line_limit() {
        let root = unique_test_dir();
        fs::write(root.join("lines.txt"), "a\nb\nc\nd\ne").expect("test file should be created");

        let handler = ReadHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "lines.txt", "limit": 3}))
            .expect("tool execution should succeed");

        assert!(result.contains("a\nb\nc"));
        assert!(result.contains("(2 more lines)"));
        remove_dir_if_exists(&root);
    }

    #[test]
    fn read_handler_returns_full_content_when_limit_exceeds_lines() {
        let root = unique_test_dir();
        fs::write(root.join("short.txt"), "line1\nline2").expect("test file should be created");

        let handler = ReadHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "short.txt", "limit": 100}))
            .expect("tool execution should succeed");

        assert_eq!(result, "line1\nline2");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn read_handler_reports_missing_path_field() {
        let handler = ReadHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'path'");
    }

    #[test]
    fn read_handler_blocks_paths_outside_workspace() {
        let handler = ReadHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "/etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert!(result.contains("outside the workspace root"));
    }

    #[test]
    fn read_handler_reports_missing_file() {
        let root = unique_test_dir();
        let handler = ReadHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "does-not-exist.txt"}))
            .expect("tool execution should succeed with error string");

        assert!(result.starts_with("Error:"));
        remove_dir_if_exists(&root);
    }

    // ── WriteHandler tests ─────────────────────────────────────────────────

    #[test]
    fn write_handler_exposes_expected_metadata() {
        let handler = WriteHandler::new(PathBuf::from("."));
        assert_eq!(handler.name(), "write_file");
        assert_eq!(handler.description(), "Write content to file.");

        let schema = handler.input_schema();
        assert_eq!(schema["required"][0], "path");
        assert_eq!(schema["required"][1], "content");
        assert_eq!(schema["properties"]["content"]["type"], "string");
    }

    #[test]
    fn write_handler_creates_new_file() {
        let root = unique_test_dir();
        let handler = WriteHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "new.txt", "content": "hello"}))
            .expect("tool execution should succeed");

        assert!(result.contains("Wrote"));
        assert_eq!(fs::read_to_string(root.join("new.txt")).unwrap(), "hello");
        remove_dir_if_exists(&root);
    }

    #[test]
    fn write_handler_overwrites_existing_file() {
        let root = unique_test_dir();
        fs::write(root.join("existing.txt"), "old content").expect("test file should be created");

        let handler = WriteHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "existing.txt", "content": "new content"}))
            .expect("tool execution should succeed");

        assert!(result.contains("Wrote"));
        assert_eq!(
            fs::read_to_string(root.join("existing.txt")).unwrap(),
            "new content"
        );
        remove_dir_if_exists(&root);
    }

    #[test]
    fn write_handler_creates_parent_directories() {
        let root = unique_test_dir();
        let handler = WriteHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "sub/dir/file.txt", "content": "nested"}))
            .expect("tool execution should succeed");

        assert!(result.contains("Wrote"));
        assert_eq!(
            fs::read_to_string(root.join("sub/dir/file.txt")).unwrap(),
            "nested"
        );
        remove_dir_if_exists(&root);
    }

    #[test]
    fn write_handler_reports_missing_path_field() {
        let handler = WriteHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"content": "hello"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'path'");
    }

    #[test]
    fn write_handler_reports_missing_content_field() {
        let handler = WriteHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "file.txt"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'content'");
    }

    #[test]
    fn write_handler_blocks_paths_outside_workspace() {
        let handler = WriteHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "/tmp/evil.txt", "content": "bad"}))
            .expect("tool execution should succeed with error string");

        assert!(result.contains("outside the workspace root"));
    }

    #[test]
    fn write_handler_reports_byte_count() {
        let root = unique_test_dir();
        let handler = WriteHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "bytes.txt", "content": "12345"}))
            .expect("tool execution should succeed");

        assert!(result.contains("5 bytes"));
        remove_dir_if_exists(&root);
    }

    // ── EditHandler tests ──────────────────────────────────────────────────

    #[test]
    fn edit_handler_exposes_expected_metadata() {
        let handler = EditHandler::new(PathBuf::from("."));
        assert_eq!(handler.name(), "edit_file");
        assert_eq!(handler.description(), "Replace exact text in file.");

        let schema = handler.input_schema();
        assert_eq!(schema["required"][0], "path");
        assert_eq!(schema["required"][1], "old_text");
        assert_eq!(schema["required"][2], "new_text");
    }

    #[test]
    fn edit_handler_replaces_text_in_file() {
        let root = unique_test_dir();
        fs::write(root.join("edit.txt"), "hello world").expect("test file should be created");

        let handler = EditHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "edit.txt", "old_text": "world", "new_text": "rust"}))
            .expect("tool execution should succeed");

        assert!(result.contains("Edited"));
        assert_eq!(
            fs::read_to_string(root.join("edit.txt")).unwrap(),
            "hello rust"
        );
        remove_dir_if_exists(&root);
    }

    #[test]
    fn edit_handler_replaces_only_first_occurrence() {
        let root = unique_test_dir();
        fs::write(root.join("multi.txt"), "foo foo foo").expect("test file should be created");

        let handler = EditHandler::new(root.clone());
        handler
            .execute(json!({"path": "multi.txt", "old_text": "foo", "new_text": "bar"}))
            .expect("tool execution should succeed");

        assert_eq!(
            fs::read_to_string(root.join("multi.txt")).unwrap(),
            "bar foo foo"
        );
        remove_dir_if_exists(&root);
    }

    #[test]
    fn edit_handler_reports_text_not_found() {
        let root = unique_test_dir();
        fs::write(root.join("file.txt"), "hello world").expect("test file should be created");

        let handler = EditHandler::new(root.clone());
        let result = handler
            .execute(json!({"path": "file.txt", "old_text": "missing", "new_text": "x"}))
            .expect("tool execution should succeed with error string");

        assert!(result.contains("Text not found"));
        // File should be unchanged.
        assert_eq!(
            fs::read_to_string(root.join("file.txt")).unwrap(),
            "hello world"
        );
        remove_dir_if_exists(&root);
    }

    #[test]
    fn edit_handler_reports_missing_path_field() {
        let handler = EditHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"old_text": "a", "new_text": "b"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'path'");
    }

    #[test]
    fn edit_handler_reports_missing_old_text_field() {
        let handler = EditHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "f.txt", "new_text": "b"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'old_text'");
    }

    #[test]
    fn edit_handler_reports_missing_new_text_field() {
        let handler = EditHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "f.txt", "old_text": "a"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Missing required field 'new_text'");
    }

    #[test]
    fn edit_handler_blocks_paths_outside_workspace() {
        let handler = EditHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"path": "/etc/passwd", "old_text": "root", "new_text": "evil"}))
            .expect("tool execution should succeed with error string");

        assert!(result.contains("outside the workspace root"));
    }
}
