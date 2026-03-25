mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::BashHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn bash_handler_runs_command_in_custom_workdir() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("nested")).expect("nested dir should be created");
    fs::write(root.path().join("nested/hello.txt"), "hello from nested")
        .expect("test file should be created");

    let handler = BashHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "command": "cat hello.txt",
            "workdir": "nested",
            "description": "Read nested greeting"
        }))
        .expect("tool execution should succeed");

    assert_eq!(result.output, "hello from nested");
    assert_eq!(result.metadata["workdir"], "nested");
    assert_eq!(result.metadata["description"], "Read nested greeting");
}

#[test]
fn bash_handler_blocks_dangerous_command_with_policy_error() {
    let handler = BashHandler::new(std::env::current_dir().expect("cwd should exist"));
    let result = handler
        .execute_v2(json!({"command": "rm -rf ."}))
        .expect("tool execution should succeed with structured error result");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Policy));
    assert_eq!(result.metadata["error_code"], "dangerous_command_blocked");
    assert!(result.is_error());
}
