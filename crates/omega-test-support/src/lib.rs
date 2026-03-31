use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};

#[derive(Debug)]
pub struct TestRoot {
    temp_dir: TempDir,
}

impl TestRoot {
    pub fn new(prefix: &str) -> Self {
        let sanitized = prefix
            .chars()
            .map(|character| match character {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
                _ => '-',
            })
            .collect::<String>();
        let temp_dir = Builder::new()
            .prefix(&format!("omega-{sanitized}-"))
            .tempdir()
            .expect("test temp root should be created");
        Self { temp_dir }
    }

    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    pub fn path_buf(&self) -> PathBuf {
        self.path().to_path_buf()
    }

    pub fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path().join(path)
    }
}

pub fn test_root(prefix: &str) -> TestRoot {
    TestRoot::new(prefix)
}

pub fn persistent_test_root(prefix: &str) -> PathBuf {
    test_root(prefix).temp_dir.keep()
}
