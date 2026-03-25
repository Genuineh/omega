mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::CreateFileHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn create_file_handler_creates_file_and_reports_diff() {
    let root = temp_root();
    let handler = CreateFileHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "new.txt", "content": "hello\nworld\n"}))
        .expect("tool execution should succeed");

    assert_eq!(
        fs::read_to_string(root.path().join("new.txt")).unwrap(),
        "hello\nworld\n"
    );
    assert_eq!(result.metadata["created"], true);
    assert!(result.output.contains("Created new.txt"));
    assert!(result.output.contains("+++ b/new.txt"));
}

#[test]
fn create_file_handler_rejects_existing_path() {
    let root = temp_root();
    fs::write(root.path().join("existing.txt"), "hello").expect("test file should be created");

    let handler = CreateFileHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "existing.txt", "content": "next"}))
        .expect("tool execution should succeed with validation error");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Validation));
    assert_eq!(result.metadata["already_exists"], true);
}
