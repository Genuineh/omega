use std::path::PathBuf;

use omega_test_support::{test_root, TestRoot};

pub fn temp_root() -> TestRoot {
    test_root("tools-builtin")
}

pub fn root_path(dir: &TestRoot) -> PathBuf {
    dir.path_buf()
}
