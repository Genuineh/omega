mod common;

use std::fs;

use omega_tools::ToolHandler;
use omega_tools_builtin::GlobSearchHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn glob_search_handler_returns_sorted_relative_matches() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("crates/demo/src")).expect("dir should be created");
    fs::write(
        root.path().join("crates/demo/src/lib.rs"),
        "pub fn demo() {}",
    )
    .expect("file should be created");
    fs::write(root.path().join("crates/demo/src/main.rs"), "fn main() {}")
        .expect("file should be created");

    let handler = GlobSearchHandler::new(root_path(&root));
    let result = handler
        .execute(json!({"pattern": "crates/**/*.rs"}))
        .expect("tool execution should succeed");

    assert_eq!(result, "crates/demo/src/lib.rs\ncrates/demo/src/main.rs");
}

#[test]
fn glob_search_handler_rejects_absolute_patterns() {
    let handler = GlobSearchHandler::new(std::env::current_dir().expect("cwd should exist"));
    let result = handler
        .execute(json!({"pattern": "/tmp/**/*.rs"}))
        .expect("tool execution should succeed with validation string");

    assert_eq!(
        result,
        "Error: Pattern must be relative to the workspace root"
    );
}
