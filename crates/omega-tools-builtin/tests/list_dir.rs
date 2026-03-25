mod common;

use std::fs;

use omega_tools::ToolHandler;
use omega_tools_builtin::ListDirHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn list_dir_handler_lists_sorted_entries_and_marks_directories() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("b-dir")).expect("dir should be created");
    fs::write(root.path().join("a.txt"), "a").expect("file should be created");
    fs::write(root.path().join("c.txt"), "c").expect("file should be created");

    let handler = ListDirHandler::new(root_path(&root));
    let result = handler
        .execute(json!({"path": "."}))
        .expect("tool execution should succeed");

    assert_eq!(result, "a.txt\nb-dir/\nc.txt");
}

#[test]
fn list_dir_handler_reports_entry_counts() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("sub")).expect("dir should be created");
    fs::write(root.path().join("a.txt"), "a").expect("file should be created");

    let handler = ListDirHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({"path": "."}))
        .expect("tool execution should succeed");

    assert_eq!(result.metadata["entry_count"], 2);
    assert_eq!(result.metadata["directory_count"], 1);
    assert_eq!(result.metadata["file_count"], 1);
}
