use serde_json::Value;

use crate::output::parse_json_value;

pub(crate) fn parse_structured_id(text: &str, field_names: &[&str]) -> Option<String> {
    let value = parse_json_value(text)?;
    parse_structured_id_from_value(Some(&value), field_names)
}

pub(crate) fn parse_structured_id_from_value(
    value: Option<&Value>,
    field_names: &[&str],
) -> Option<String> {
    let object = value?.as_object()?;
    field_names.iter().find_map(|field_name| {
        object
            .get(*field_name)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub(crate) fn find_catalog_match<'a>(
    text: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let normalized = text.to_ascii_lowercase();
    normalized
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '-' && character != '_'
        })
        .find_map(|token| {
            if token.is_empty() {
                return None;
            }
            candidates.iter().find_map(|candidate| {
                token
                    .eq_ignore_ascii_case(candidate)
                    .then(|| (*candidate).to_string())
            })
        })
}

pub(crate) fn latest_user_turn_requires_feature_scene(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    const ASCII_HINTS: &[&str] = &[
        "fix",
        "implement",
        "update",
        "edit",
        "change",
        "modify",
        "add",
        "create",
        "write",
        "refactor",
        "rename",
        "remove",
        "delete",
        "split",
        "move",
        "replace",
        "wire",
        "expose",
        "support",
        "patch",
        "document",
        "docs",
        "test",
    ];
    const CJK_HINTS: &[&str] = &[
        "修复",
        "实现",
        "更新",
        "编辑",
        "修改",
        "调整",
        "新增",
        "添加",
        "创建",
        "编写",
        "重构",
        "重命名",
        "删除",
        "拆",
        "迁移",
        "补",
        "改",
        "文档",
        "测试",
        "暴露",
    ];

    let normalized = trimmed.to_ascii_lowercase();
    let ascii_tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if ascii_tokens
        .iter()
        .any(|token| ASCII_HINTS.iter().any(|hint| token == hint))
    {
        return true;
    }

    CJK_HINTS.iter().any(|hint| trimmed.contains(hint))
}

pub(crate) fn latest_user_turn_prefers_research_scene(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    const ASCII_EXPLICIT_HINTS: &[&str] = &[
        "research",
        "investigate",
        "investigation",
        "explore",
        "exploration",
        "discovery",
        "deepdive",
    ];
    const ASCII_ANALYSIS_HINTS: &[&str] = &[
        "analyze",
        "analysis",
        "review",
        "evaluate",
        "comparison",
        "compare",
        "architecture",
        "tradeoff",
        "tradeoffs",
        "survey",
    ];
    const CJK_EXPLICIT_HINTS: &[&str] = &["研究", "调研", "探索", "调查", "排查"];
    const CJK_ANALYSIS_HINTS: &[&str] = &["分析", "评审", "架构", "对比", "比较", "梳理"];

    let normalized = trimmed.to_ascii_lowercase();
    let ascii_tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    if ascii_tokens
        .iter()
        .any(|token| ASCII_EXPLICIT_HINTS.iter().any(|hint| token == hint))
    {
        return true;
    }

    if CJK_EXPLICIT_HINTS.iter().any(|hint| trimmed.contains(hint)) {
        return true;
    }

    ASCII_ANALYSIS_HINTS
        .iter()
        .any(|hint| ascii_tokens.iter().any(|token| token == hint))
        || CJK_ANALYSIS_HINTS.iter().any(|hint| trimmed.contains(hint))
}

pub(crate) fn latest_user_turn_prefers_deep_research_scene(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    const ASCII_EXPLICIT_HINTS: &[&str] = &[
        "deep-research",
        "deepresearch",
        "deep-dive",
        "deepdive",
        "systematic",
        "holistic",
        "global",
        "comprehensive",
        "systemwide",
    ];
    const ASCII_ANALYSIS_HINTS: &[&str] = &[
        "analyze",
        "analysis",
        "investigate",
        "investigation",
        "explore",
        "exploration",
        "research",
        "architecture",
        "tradeoff",
        "tradeoffs",
        "survey",
        "review",
    ];
    const ASCII_INTENSIFIERS: &[&str] = &[
        "deep",
        "complex",
        "comprehensive",
        "systematic",
        "thorough",
        "holistic",
        "detailed",
        "global",
        "broad",
        "repo",
        "wide",
        "endtoend",
    ];
    const CJK_EXPLICIT_HINTS: &[&str] = &[
        "深度研究",
        "深度调研",
        "深入研究",
        "系统性",
        "全局性",
        "全局",
        "全面",
        "综合性",
    ];
    const CJK_ANALYSIS_HINTS: &[&str] = &["分析", "研究", "调研", "探索", "调查", "梳理", "架构"];
    const CJK_INTENSIFIERS: &[&str] = &[
        "深度",
        "深入",
        "系统性",
        "全局性",
        "全局",
        "全面",
        "综合",
        "整体",
    ];

    let normalized = trimmed.to_ascii_lowercase();
    let ascii_tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    if ascii_tokens
        .iter()
        .any(|token| ASCII_EXPLICIT_HINTS.iter().any(|hint| token == hint))
    {
        return true;
    }

    if ascii_tokens
        .iter()
        .any(|token| ASCII_ANALYSIS_HINTS.iter().any(|hint| token == hint))
        && ascii_tokens
            .iter()
            .any(|token| ASCII_INTENSIFIERS.iter().any(|hint| token == hint))
    {
        return true;
    }

    if CJK_EXPLICIT_HINTS.iter().any(|hint| trimmed.contains(hint)) {
        return true;
    }

    CJK_ANALYSIS_HINTS.iter().any(|hint| trimmed.contains(hint))
        && CJK_INTENSIFIERS.iter().any(|hint| trimmed.contains(hint))
}
