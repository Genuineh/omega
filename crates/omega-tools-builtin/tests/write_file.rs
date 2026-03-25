mod common;

use std::fs;

use omega_tools::ToolHandler;
use omega_tools_builtin::WriteHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn write_handler_overwrite_reports_previous_size_and_diff() {
    let root = temp_root();
    fs::write(root.path().join("existing.txt"), "old\nvalue\n")
        .expect("test file should be created");

    let handler = WriteHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "existing.txt", "content": "new\nvalue\n"}))
        .expect("tool execution should succeed");

    assert_eq!(result.metadata["previously_existed"], true);
    assert_eq!(result.metadata["bytes_before"], 10);
    assert_eq!(result.metadata["bytes_after"], 10);
    assert!(result.output.contains("--- a/existing.txt"));
}

#[test]
fn write_handler_creates_parent_directories() {
    let root = temp_root();
    let handler = WriteHandler::new(root_path(&root));
    handler
        .execute(json!({"path": "sub/dir/file.txt", "content": "nested"}))
        .expect("tool execution should succeed");

    assert_eq!(
        fs::read_to_string(root.path().join("sub/dir/file.txt")).unwrap(),
        "nested"
    );
}
