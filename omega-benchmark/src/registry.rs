use std::collections::BTreeMap;
use std::path::Path;

use crate::manifest::{SuiteManifest, Track};
use crate::scoring::Scorer;

/// Registry of benchmark suites, organized by track.
pub struct SuiteRegistry {
    suites: BTreeMap<String, RegisteredSuite>,
}

pub struct RegisteredSuite {
    pub manifest: SuiteManifest,
    pub scorer: Box<dyn Scorer>,
}

impl SuiteRegistry {
    pub fn new() -> Self {
        Self {
            suites: BTreeMap::new(),
        }
    }

    /// Register a suite with its manifest and scorer.
    pub fn register(&mut self, manifest: SuiteManifest, scorer: Box<dyn Scorer>) {
        let id = manifest.id.clone();
        self.suites.insert(id, RegisteredSuite { manifest, scorer });
    }

    /// Get a registered suite by ID.
    pub fn get(&self, id: &str) -> Option<&RegisteredSuite> {
        self.suites.get(id)
    }

    /// List all registered suite IDs.
    pub fn suite_ids(&self) -> Vec<String> {
        self.suites.keys().cloned().collect()
    }

    /// List suites filtered by track.
    pub fn suites_for_track(&self, track: Track) -> Vec<&RegisteredSuite> {
        self.suites
            .values()
            .filter(|s| s.manifest.track == track)
            .collect()
    }

    /// Build a map of suite_id -> track string for summary generation.
    pub fn suite_track_map(&self) -> BTreeMap<String, String> {
        self.suites
            .iter()
            .map(|(id, s)| (id.clone(), s.manifest.track.to_string()))
            .collect()
    }

    /// Number of registered suites.
    pub fn len(&self) -> usize {
        self.suites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.suites.is_empty()
    }

    /// Auto-discover and register suites from a directory.
    ///
    /// Scans `suites_dir` for subdirectories containing `manifest.json`,
    /// loads each manifest, and pairs it with the appropriate default scorer.
    pub fn discover(suites_dir: &Path) -> anyhow::Result<Self> {
        use crate::scoring::{assistant, data_quality, tool_calling};

        let mut registry = Self::new();
        if !suites_dir.is_dir() {
            return Ok(registry);
        }

        for entry in std::fs::read_dir(suites_dir)? {
            let entry = entry?;
            let manifest_path = entry.path().join("manifest.json");
            if manifest_path.is_file() {
                let manifest = SuiteManifest::load(&manifest_path)?;
                let scorer: Box<dyn Scorer> = match manifest.track {
                    Track::ToolCalling => Box::new(tool_calling::ToolCallingScorer),
                    Track::Assistant => Box::new(assistant::AssistantScorer),
                    Track::DataQuality => Box::new(data_quality::DataQualityScorer),
                };
                registry.register(manifest, scorer);
            }
        }
        Ok(registry)
    }
}
