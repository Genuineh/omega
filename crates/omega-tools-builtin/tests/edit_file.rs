mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::EditHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn edit_handler_reports_diff_and_match_count() {
    let root = temp_root();
    fs::write(root.path().join("multi.txt"), "foo\nfoo\n").expect("test file should be created");

    let handler = EditHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "multi.txt", "old_text": "foo", "new_text": "bar"}))
        .expect("tool execution should succeed");

    assert_eq!(result.metadata["match_count"], 2);
    assert_eq!(result.metadata["ambiguous_match"], true);
    assert!(result
        .output
        .contains("Edited multi.txt (replaced first match of 2)"));
}

#[test]
fn edit_handler_classifies_text_not_found_as_validation() {
    let root = temp_root();
    fs::write(root.path().join("file.txt"), "hello world").expect("test file should be created");

    let handler = EditHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "file.txt", "old_text": "missing", "new_text": "x"}))
        .expect("tool execution should succeed with error result");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Validation));
    assert_eq!(result.metadata["match_count"], 0);
}
