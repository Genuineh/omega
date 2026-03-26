use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOOKS_DIR: &str = ".omega/hooks";
pub const DEFAULT_HOOK_MANIFEST_FILE: &str = "Hook.toml";
pub const DEFAULT_HOOK_API_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookManifest {
    pub id: String,
    pub package: String,
    pub artifact: PathBuf,
    pub api_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookManifestEntry {
    pub manifest_path: PathBuf,
    pub artifact_path: PathBuf,
    pub manifest: HookManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCatalog {
    root: PathBuf,
    manifests: BTreeMap<String, HookManifestEntry>,
}

impl HookCatalog {
    pub fn load(root: &Path) -> Result<Self> {
        let hooks_root = root.join(DEFAULT_HOOKS_DIR);
        if !hooks_root.exists() {
            return Ok(Self {
                root: root.to_path_buf(),
                manifests: BTreeMap::new(),
            });
        }

        let mut manifest_paths = fs::read_dir(&hooks_root)
            .with_context(|| format!("failed to read hook directory {}", hooks_root.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path().join(DEFAULT_HOOK_MANIFEST_FILE))
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        manifest_paths.sort();

        let mut manifests = BTreeMap::new();
        for manifest_path in manifest_paths {
            let entry = HookManifestEntry::load(&manifest_path)?;
            let hook_id = entry.manifest.id.clone();
            if manifests.insert(hook_id.clone(), entry).is_some() {
                bail!(
                    "hook manifest id '{}' is duplicated under {}",
                    hook_id,
                    hooks_root.display()
                );
            }
        }

        Ok(Self {
            root: root.to_path_buf(),
            manifests,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self, hook_id: &str) -> Option<&HookManifestEntry> {
        self.manifests.get(hook_id)
    }

    pub fn hook_ids(&self) -> Vec<&str> {
        self.manifests.keys().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }
}

impl HookManifestEntry {
    pub fn load(manifest_path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(manifest_path)
            .with_context(|| format!("failed to read hook manifest {}", manifest_path.display()))?;
        let manifest = toml::from_str::<HookManifest>(&raw).with_context(|| {
            format!("failed to parse hook manifest {}", manifest_path.display())
        })?;

        let id = manifest.id.trim().to_string();
        if id.is_empty() {
            bail!("hook manifest {} must declare a non-empty id", manifest_path.display());
        }

        let package = manifest.package.trim().to_string();
        if package.is_empty() {
            bail!(
                "hook manifest {} must declare a non-empty package",
                manifest_path.display()
            );
        }

        if manifest.artifact.as_os_str().is_empty() {
            bail!(
                "hook manifest {} must declare a non-empty artifact path",
                manifest_path.display()
            );
        }

        if manifest.api_version == 0 {
            bail!(
                "hook manifest {} must declare api_version >= 1",
                manifest_path.display()
            );
        }

        let artifact_path = resolve_artifact_path(manifest_path, &manifest.artifact);

        Ok(Self {
            manifest_path: manifest_path.to_path_buf(),
            artifact_path,
            manifest: HookManifest {
                id,
                package,
                artifact: manifest.artifact,
                api_version: manifest.api_version,
            },
        })
    }
}

fn resolve_artifact_path(manifest_path: &Path, artifact: &Path) -> PathBuf {
    if artifact.is_absolute() {
        artifact.to_path_buf()
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(artifact)
    }
}