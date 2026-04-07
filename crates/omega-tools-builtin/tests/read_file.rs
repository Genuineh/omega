mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::ReadHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn read_handler_reads_requested_line_range() {
    let root = temp_root();
    fs::write(root.path().join("lines.txt"), "a\nb\nc\nd\ne").expect("test file should be created");

    let handler = ReadHandler::new(root_path(&root));
    let result = handler
        .execute(json!({"path": "lines.txt", "start_line": 2, "end_line": 4}))
        .expect("tool execution should succeed");

    assert_eq!(result, "b\nc\nd");
}

#[test]
fn read_handler_reports_truncation_metadata() {
    let root = temp_root();
    fs::write(root.path().join("lines.txt"), "a\nb\nc\nd\ne").expect("test file should be created");

    let handler = ReadHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "lines.txt", "limit": 3}))
        .expect("tool execution should succeed");

    assert!(result.truncated);
    assert_eq!(result.metadata["path"], "lines.txt");
    assert_eq!(result.metadata["omitted_lines"], 2);
}

#[test]
fn read_handler_prefers_explicit_range_over_legacy_limit() {
    let root = temp_root();
    fs::write(root.path().join("lines.txt"), "a\nb\nc\nd\ne").expect("test file should be created");

    let handler = ReadHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "path": "lines.txt",
            "start_line": 2,
            "end_line": 4,
            "limit": 1
        }))
        .expect("tool execution should succeed");

    assert_eq!(result.error_kind, None);
    assert_eq!(result.output, "b\nc\nd");
    assert_eq!(result.metadata["start_line"], 2);
    assert_eq!(result.metadata["end_line"], 4);
}

#[test]
fn read_handler_still_rejects_inverted_range() {
    let root = temp_root();
    fs::write(root.path().join("lines.txt"), "a\nb\nc").expect("test file should be created");

    let handler = ReadHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "lines.txt", "start_line": 3, "end_line": 2}))
        .expect("tool execution should succeed with validation error");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Validation));
}
