use std::collections::BTreeSet;

use omega_skills::{Skill, SkillLoader};
use omega_workflow::StepSkillRequest;

const DEFAULT_MATCH_LIMIT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillSet {
    descriptions: Vec<String>,
    preloaded_skills: Vec<Skill>,
}

impl ResolvedSkillSet {
    pub fn new(descriptions: Vec<String>, preloaded_skills: Vec<Skill>) -> Self {
        Self {
            descriptions,
            preloaded_skills,
        }
    }

    pub fn descriptions(&self) -> &[String] {
        &self.descriptions
    }

    pub fn preloaded_skills(&self) -> &[Skill] {
        &self.preloaded_skills
    }

    pub fn build_system_prompt(&self, base_prompt: &str) -> String {
        let mut sections = vec![base_prompt.trim_end().to_string()];

        if !self.descriptions.is_empty() {
            sections.push(format!(
                "Skills available:\n{}",
                self.descriptions.join("\n")
            ));
        }

        if !self.preloaded_skills.is_empty() {
            let preloaded = self
                .preloaded_skills
                .iter()
                .map(Skill::rendered_content)
                .collect::<Vec<_>>()
                .join("\n\n");
            sections.push(format!("Preloaded skills for this task:\n{preloaded}"));
        }

        sections.join("\n\n")
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionSkillCatalog {
    loader: SkillLoader,
}

impl SessionSkillCatalog {
    pub fn new(loader: SkillLoader) -> Self {
        Self { loader }
    }

    pub fn resolve_for_step(&self, task: &str, request: &StepSkillRequest) -> ResolvedSkillSet {
        let descriptions = self.loader.descriptions();

        match request {
            StepSkillRequest::Disable => ResolvedSkillSet::new(descriptions, Vec::new()),
            StepSkillRequest::MatchTask => ResolvedSkillSet::new(
                descriptions,
                self.loader.match_for_task(task, DEFAULT_MATCH_LIMIT),
            ),
            StepSkillRequest::Append(names) => {
                let mut preloaded = self.loader.match_for_task(task, DEFAULT_MATCH_LIMIT);
                let mut seen = preloaded
                    .iter()
                    .map(|skill| skill.name.clone())
                    .collect::<BTreeSet<_>>();

                for name in names {
                    if let Some(skill) = self.loader.get(name) {
                        if seen.insert(skill.name.clone()) {
                            preloaded.push(skill.clone());
                        }
                    }
                }

                ResolvedSkillSet::new(descriptions, preloaded)
            }
        }
    }

    pub fn build_system_prompt(
        &self,
        base_prompt: &str,
        task: &str,
        request: &StepSkillRequest,
    ) -> String {
        self.resolve_for_step(task, request)
            .build_system_prompt(base_prompt)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SessionSkillCatalog;
    use omega_skills::SkillLoader;
    use omega_workflow::StepSkillRequest;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("omega-session-skill-catalog-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn catalog() -> SessionSkillCatalog {
        let root = temp_dir("catalog");
        let review = root.join(".claude/skills/review");
        let docs = root.join(".claude/skills/docs");
        fs::create_dir_all(&review).unwrap();
        fs::create_dir_all(&docs).unwrap();
        fs::write(
            review.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes\n---\nFind regressions first.",
        )
        .unwrap();
        fs::write(
            docs.join("SKILL.md"),
            "---\nname: docs-specs\ndescription: Write technical specs\n---\nBe precise.",
        )
        .unwrap();

        SessionSkillCatalog::new(SkillLoader::from_repo_root(&root).unwrap())
    }

    #[test]
    fn match_task_preserves_existing_skill_prompt_shape() {
        let catalog = catalog();
        let prompt = catalog.build_system_prompt(
            "Base prompt",
            "Please review this patch",
            &StepSkillRequest::MatchTask,
        );

        assert!(prompt.contains("Skills available:"));
        assert!(prompt.contains("review: Review code changes"));
        assert!(prompt.contains("Preloaded skills for this task:"));
        assert!(prompt.contains("<skill name=\"review\">"));
    }

    #[test]
    fn append_adds_explicit_skills_without_duplicates() {
        let catalog = catalog();
        let resolved = catalog.resolve_for_step(
            "Please review this patch",
            &StepSkillRequest::Append(vec!["review".to_string(), "docs-specs".to_string()]),
        );

        let names = resolved
            .preloaded_skills()
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["review", "docs-specs"]);
    }

    #[test]
    fn disable_keeps_descriptions_but_skips_preloaded_skills() {
        let catalog = catalog();
        let resolved =
            catalog.resolve_for_step("Please review this patch", &StepSkillRequest::Disable);

        assert!(!resolved.descriptions().is_empty());
        assert!(resolved.preloaded_skills().is_empty());
    }
}
