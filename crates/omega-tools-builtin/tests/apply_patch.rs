mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::ApplyPatchHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn apply_patch_handler_applies_multiple_edits_atomically() {
    let root = temp_root();
    fs::write(root.path().join("file.txt"), "alpha\nbeta\ngamma\n")
        .expect("test file should be created");

    let handler = ApplyPatchHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "path": "file.txt",
            "edits": [
                {"old_text": "alpha", "new_text": "alpha!"},
                {"old_text": "gamma", "new_text": "delta"}
            ]
        }))
        .expect("tool execution should succeed");

    assert_eq!(
        fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "alpha!\nbeta\ndelta\n"
    );
    assert_eq!(result.metadata["edit_count"], 2);
    assert_eq!(result.metadata["replacements"], 2);
}

#[test]
fn apply_patch_handler_rejects_ambiguous_match_without_writing() {
    let root = temp_root();
    fs::write(root.path().join("file.txt"), "foo\nfoo\n").expect("test file should be created");

    let handler = ApplyPatchHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "path": "file.txt",
            "edits": [
                {"old_text": "foo", "new_text": "bar"}
            ]
        }))
        .expect("tool execution should succeed with validation error");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Validation));
    assert_eq!(result.metadata["match_count"], 2);
    assert_eq!(
        fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "foo\nfoo\n"
    );
}
