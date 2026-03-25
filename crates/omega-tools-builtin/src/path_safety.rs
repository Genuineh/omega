use std::path::{Path, PathBuf};

pub(crate) fn resolve_file_root(root: PathBuf) -> PathBuf {
    let absolute = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(root)
    };
    std::fs::canonicalize(&absolute).unwrap_or_else(|_| normalize_file_path(&absolute))
}

pub(crate) fn normalize_file_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut components: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                }
            }
            Component::CurDir => {}
            _ => components.push(component),
        }
    }
    components.iter().collect()
}

pub(crate) fn safe_path_within_root(
    root: &Path,
    path_arg: &str,
) -> std::result::Result<PathBuf, String> {
    safe_path_within_root_from(root, root, path_arg)
}

pub(crate) fn safe_path_within_root_from(
    root: &Path,
    base_dir: &Path,
    path_arg: &str,
) -> std::result::Result<PathBuf, String> {
    if path_arg.is_empty() {
        return Err("Error: Path cannot be empty".to_string());
    }
    let candidate = Path::new(path_arg);
    let resolved = if candidate.is_absolute() {
        std::fs::canonicalize(candidate).unwrap_or_else(|_| normalize_file_path(candidate))
    } else {
        let joined = base_dir.join(candidate);
        std::fs::canonicalize(&joined).unwrap_or_else(|_| {
            if let Some(parent) = joined.parent() {
                if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
                    if let Some(name) = joined.file_name() {
                        return canonical_parent.join(name);
                    }
                }
            }
            normalize_file_path(&joined)
        })
    };
    if !resolved.starts_with(root) {
        return Err(format!(
            "Error: Path '{path_arg}' is outside the workspace root"
        ));
    }
    Ok(resolved)
}
