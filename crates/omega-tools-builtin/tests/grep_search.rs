mod common;

use std::fs;

use omega_tools::ToolHandler;
use omega_tools_builtin::GrepSearchHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn grep_search_handler_finds_matching_lines() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("src")).expect("dir should be created");
    fs::write(
        root.path().join("src/lib.rs"),
        "fn main() {}\nlet value = 1;",
    )
    .expect("file should be created");
    fs::write(root.path().join("src/other.rs"), "fn helper() {}").expect("file should be created");

    let handler = GrepSearchHandler::new(root_path(&root));
    let result = handler
        .execute(json!({"query": "fn", "path": "src"}))
        .expect("tool execution should succeed");

    assert_eq!(
        result,
        "src/lib.rs:1:fn main() {}\nsrc/other.rs:1:fn helper() {}"
    );
}

#[test]
fn grep_search_handler_supports_regex_and_include_pattern() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("src")).expect("dir should be created");
    fs::write(
        root.path().join("src/lib.rs"),
        "fn main() {}\nfn helper() {}",
    )
    .expect("file should be created");
    fs::write(root.path().join("src/lib.txt"), "fn text() {}").expect("file should be created");

    let handler = GrepSearchHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "query": "fn\\s+[a-z_]+",
            "is_regex": true,
            "include_pattern": "src/**/*.rs"
        }))
        .expect("tool execution should succeed");

    assert_eq!(result.metadata["returned_match_count"], 2);
    assert_eq!(result.metadata["include_pattern"], "src/**/*.rs");
    assert!(result.output.contains("src/lib.rs:1:fn main() {}"));
    assert!(!result.output.contains("lib.txt"));
}
