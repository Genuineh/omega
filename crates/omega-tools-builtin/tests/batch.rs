mod common;

use std::fs;

use omega_tools::{ToolErrorKind, ToolHandler};
use omega_tools_builtin::BatchHandler;
use serde_json::json;

use common::{root_path, temp_root};

#[test]
fn batch_handler_runs_readonly_requests_and_preserves_order() {
    let root = temp_root();
    fs::create_dir_all(root.path().join("src")).expect("dir should be created");
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .expect("file should be created");
    fs::write(root.path().join("src/lib.rs"), "pub fn demo() {}\n")
        .expect("file should be created");

    let handler = BatchHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "requests": [
                {"tool": "list_dir", "input": {"path": "."}},
                {"tool": "read_file", "input": {"path": "Cargo.toml", "start_line": 1, "end_line": 1}}
            ]
        }))
        .expect("tool execution should succeed");

    assert_eq!(result.metadata["request_count"], 2);
    assert_eq!(result.metadata["success_count"], 2);
    assert_eq!(result.metadata["failure_count"], 0);
    assert_eq!(result.metadata["results"][0]["tool"], "list_dir");
    assert_eq!(result.metadata["results"][1]["tool"], "read_file");
}

#[test]
fn batch_handler_rejects_too_many_requests() {
    let handler = BatchHandler::new(std::env::current_dir().expect("cwd should exist"));
    let requests = (0..9)
        .map(|index| {
            json!({"tool": "list_dir", "input": {"path": "."}, "id": format!("req-{index}")})
        })
        .collect::<Vec<_>>();

    let result = handler
        .execute_v2(json!({"requests": requests}))
        .expect("tool execution should succeed with validation error");

    assert_eq!(result.error_kind, Some(ToolErrorKind::Validation));
    assert_eq!(result.metadata["request_count"], 9);
    assert_eq!(result.metadata["max_request_count"], 8);
}

#[test]
fn batch_handler_accepts_json_string_requests_payload() {
    let root = temp_root();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .expect("file should be created");

    let handler = BatchHandler::new(root_path(&root));
    let result = handler
        .execute_v2(json!({
            "requests": "[{\"id\":\"root\",\"tool\":\"read_file\",\"input\":{\"path\":\"Cargo.toml\",\"start_line\":1,\"end_line\":1}}]"
        }))
        .expect("tool execution should succeed");

    assert_eq!(result.error_kind, None);
    assert_eq!(result.metadata["request_count"], 1);
    assert_eq!(result.metadata["success_count"], 1);
    assert_eq!(result.metadata["results"][0]["id"], "root");
    assert_eq!(result.metadata["results"][0]["tool"], "read_file");
}
