use std::path::PathBuf;

use tempfile::TempDir;

pub fn temp_root() -> TempDir {
    tempfile::tempdir().expect("temp dir should be created")
}

pub fn root_path(dir: &TempDir) -> PathBuf {
    dir.path().to_path_buf()
}
