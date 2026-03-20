use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use omega_tools::ToolHandler;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub source_path: PathBuf,
}

impl Skill {
    pub fn rendered_content(&self) -> String {
        format!("<skill name=\"{}\">\n{}\n</skill>", self.name, self.body)
    }

    fn match_score(&self, task_tokens: &BTreeSet<String>, normalized_task: &str) -> usize {
        if task_tokens.is_empty() {
            return 0;
        }

        let terms = collect_terms(&self.name, &self.description);
        let overlap = terms.intersection(task_tokens).count();
        let exact_name_bonus =
            usize::from(normalized_task.contains(&normalize_text(&self.name))) * 4;
        let exact_desc_bonus = usize::from(
            !self.description.is_empty()
                && normalized_task.contains(&normalize_text(&self.description)),
        ) * 2;

        overlap + exact_name_bonus + exact_desc_bonus
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillLoader {
    skills: BTreeMap<String, Skill>,
}

impl SkillLoader {
    pub fn new(skills_dir: &Path) -> Result<Self> {
        let mut skills = BTreeMap::new();
        load_skills_recursive(skills_dir, &mut skills)?;
        Ok(Self { skills })
    }

    pub fn from_repo_root(root: &Path) -> Result<Self> {
        let mut skills = BTreeMap::new();
        for candidate in [root.join(".claude/skills"), root.join("skills")] {
            load_skills_recursive(&candidate, &mut skills)?;
        }
        Ok(Self { skills })
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn descriptions(&self) -> Vec<String> {
        self.skills
            .values()
            .map(|skill| {
                if skill.description.is_empty() {
                    format!("  - {}", skill.name)
                } else {
                    format!("  - {}: {}", skill.name, skill.description)
                }
            })
            .collect()
    }

    pub fn load(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(skill) => skill.rendered_content(),
            None => format!("Error: Unknown skill '{}'", name),
        }
    }

    pub fn match_for_task(&self, task: &str, limit: usize) -> Vec<Skill> {
        let normalized_task = normalize_text(task);
        let task_tokens = tokenize(&normalized_task);
        if task_tokens.is_empty() {
            return Vec::new();
        }

        let mut ranked: Vec<(usize, &Skill)> = self
            .skills
            .values()
            .map(|skill| (skill.match_score(&task_tokens, &normalized_task), skill))
            .filter(|(score, _)| *score > 0)
            .collect();

        ranked.sort_by(|(score_a, skill_a), (score_b, skill_b)| {
            score_b
                .cmp(score_a)
                .then_with(|| skill_a.name.cmp(&skill_b.name))
        });

        ranked
            .into_iter()
            .take(limit)
            .map(|(_, skill)| skill.clone())
            .collect()
    }

    pub fn build_system_prompt(&self, base_prompt: &str, task: &str) -> String {
        if self.skills.is_empty() {
            return base_prompt.to_string();
        }

        let mut sections = vec![base_prompt.trim_end().to_string()];

        let descriptions = self.descriptions();
        if !descriptions.is_empty() {
            sections.push(format!("Skills available:\n{}", descriptions.join("\n")));
        }

        let matched = self.match_for_task(task, 3);
        if !matched.is_empty() {
            let preloaded = matched
                .iter()
                .map(Skill::rendered_content)
                .collect::<Vec<_>>()
                .join("\n\n");
            sections.push(format!("Preloaded skills for this task:\n{}", preloaded));
        }

        sections.join("\n\n")
    }
}

pub struct LoadSkillHandler {
    loader: SkillLoader,
}

impl LoadSkillHandler {
    pub fn new(loader: SkillLoader) -> Self {
        Self { loader }
    }

    pub fn from_repo_root(root: &Path) -> Result<Self> {
        Ok(Self::new(SkillLoader::from_repo_root(root)?))
    }
}

impl ToolHandler for LoadSkillHandler {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load the full content of a repository skill by name."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name to load."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        let name = input
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Ok(self.loader.load(name))
    }
}

fn load_skills_recursive(path: &Path, skills: &mut BTreeMap<String, Skill>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            let skill_md = entry_path.join("SKILL.md");
            if skill_md.is_file() {
                let skill = parse_skill_file(&skill_md)?;
                skills.insert(skill.name.clone(), skill);
            }
            load_skills_recursive(&entry_path, skills)?;
        }
    }

    Ok(())
}

fn parse_skill_file(path: &Path) -> Result<Skill> {
    let text = fs::read_to_string(path)?;
    let (frontmatter, body) = split_frontmatter(&text);
    let fallback_name = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();

    let name = frontmatter
        .get("name")
        .cloned()
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_name);
    let description = frontmatter.get("description").cloned().unwrap_or_default();

    Ok(Skill {
        name,
        description,
        body,
        source_path: path.to_path_buf(),
    })
}

fn split_frontmatter(text: &str) -> (BTreeMap<String, String>, String) {
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some((frontmatter, body)) = rest.split_once("\n---\n") {
            return (parse_frontmatter(frontmatter), body.trim().to_string());
        }
    }

    (BTreeMap::new(), text.trim().to_string())
}

fn parse_frontmatter(frontmatter: &str) -> BTreeMap<String, String> {
    frontmatter
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
                return None;
            }

            let (key, value) = trimmed.split_once(':')?;
            Some((
                key.trim().to_string(),
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            ))
        })
        .collect()
}

fn collect_terms(name: &str, description: &str) -> BTreeSet<String> {
    let mut terms = tokenize(&normalize_text(name));
    terms.extend(tokenize(&normalize_text(description)));
    terms
}

fn normalize_text(text: &str) -> String {
    text.to_ascii_lowercase()
}

fn tokenize(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .map(|token| token.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("omega-skills-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn loader_scans_recursive_skill_directories() {
        let root = temp_dir("recursive");
        let nested = root.join("group/review");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
			nested.join("SKILL.md"),
			"---\nname: code-review\ndescription: Review code carefully\n---\nCheck for regressions.",
		)
		.unwrap();

        let loader = SkillLoader::new(&root).unwrap();

        assert_eq!(loader.len(), 1);
        assert_eq!(
            loader.get("code-review").unwrap().description,
            "Review code carefully"
        );
    }

    #[test]
    fn build_system_prompt_lists_and_preloads_matching_skills() {
        let root = temp_dir("prompt");
        let skills_dir = root.join(".claude/skills/review");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code changes\n---\nFind regressions first.",
        )
        .unwrap();

        let loader = SkillLoader::from_repo_root(&root).unwrap();
        let prompt = loader.build_system_prompt("Base prompt", "Please review this patch");

        assert!(prompt.contains("Skills available:"));
        assert!(prompt.contains("review: Review code changes"));
        assert!(prompt.contains("Preloaded skills for this task:"));
        assert!(prompt.contains("<skill name=\"review\">"));
    }

    #[test]
    fn load_returns_error_for_unknown_skill() {
        let loader = SkillLoader::default();
        assert_eq!(loader.load("missing"), "Error: Unknown skill 'missing'");
    }

    #[test]
    fn load_skill_handler_reads_name_argument() {
        let root = temp_dir("handler");
        let skills_dir = root.join("skills/git");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: git\ndescription: Git workflows\n---\nUse topic branches.",
        )
        .unwrap();

        let handler = LoadSkillHandler::from_repo_root(&root).unwrap();
        let output = handler.execute(json!({"name": "git"})).unwrap();

        assert!(output.contains("<skill name=\"git\">"));
        assert!(output.contains("Use topic branches."));
    }
}
