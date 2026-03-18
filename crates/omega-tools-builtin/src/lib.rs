use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::Result;
#[cfg(unix)]
use libc::{kill, SIGKILL};
use omega_tools::ToolHandler;
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use tempfile::NamedTempFile;
use tracing::{debug, error, info, warn};
use wait_timeout::ChildExt;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_OUTPUT_CHARS: usize = 50_000;
const ALLOWED_COMMANDS: &[&str] = &[
    "cat",
    "echo",
    "false",
    "head",
    "ls",
    "printf",
    "pwd",
    "rg",
    "sleep",
    "tail",
    "touch",
    "tr",
    "true",
    "wait",
    "wc",
    "yes",
];
const DISALLOWED_SHELL_TOKENS: &[char] = &['$', '`', '\n', '\r'];
const FILE_PATH_COMMANDS: &[&str] = &["cat", "head", "ls", "rg", "tail", "touch", "wc"];

#[derive(Debug, Clone)]
pub struct BashHandler {
    root: PathBuf,
    default_timeout_seconds: u64,
}

impl BashHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Self::resolve_root(root),
            default_timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }

    pub fn with_timeout(root: PathBuf, default_timeout_seconds: u64) -> Self {
        Self {
            root: Self::resolve_root(root),
            default_timeout_seconds,
        }
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
            return Err(format!("Error: Path '{path_arg}' is outside the workspace root"));
        }

        Ok(())
    }

    fn validate_segment(&self, segment: &str) -> std::result::Result<(), String> {
        let argv = shlex::split(segment).ok_or_else(|| "Error: Command parsing failed".to_string())?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "Error: Command cannot be empty".to_string())?;

        if !ALLOWED_COMMANDS.contains(&program.as_str()) {
            return Err(format!("Error: Command '{program}' is blocked by safety policy"));
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

    fn validate_command(&self, input: &Value) -> std::result::Result<String, String> {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or_else(|| "Error: Missing required field 'command'".to_string())?;

        if command.is_empty() {
            return Err("Error: Command cannot be empty".to_string());
        }

        if command.chars().any(|character| DISALLOWED_SHELL_TOKENS.contains(&character)) {
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

        let mut child = match self.build_command(command, stdout_handle, stderr_handle).spawn() {
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
            debug!(bash.output_truncated = true, bash.max_chars = MAX_OUTPUT_CHARS);
        }

        Ok(result)
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
    fn bash_handler_blocks_paths_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "cat /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Path '/etc/passwd' is outside the workspace root");
    }

    #[test]
    fn bash_handler_blocks_wc_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "wc /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Path '/etc/passwd' is outside the workspace root");
    }

    #[test]
    fn bash_handler_blocks_rg_outside_workspace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "rg root /etc/passwd"}))
            .expect("tool execution should succeed with error string");

        assert_eq!(result, "Error: Path '/etc/passwd' is outside the workspace root");
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
        let result = handler.execute(json!({})).expect("tool execution should succeed");

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
    fn bash_handler_trims_surrounding_whitespace() {
        let handler = BashHandler::new(PathBuf::from("."));
        let result = handler
            .execute(json!({"command": "printf '  hello  '"}))
            .expect("tool execution should succeed");

        assert_eq!(result, "hello");
    }
}
