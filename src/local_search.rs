use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::SearchHit;
use crate::utils::{clean_text, normalize, token_overlap};

pub(crate) fn collect_local_project_files(root: &Path, limit: usize, files: &mut Vec<PathBuf>) -> Result<()> {
    if files.len() >= limit || !root.exists() {
        return Ok(());
    }
    if root.is_dir() {
        if should_skip_local_project_dir(root) {
            return Ok(());
        }
        for entry in std::fs::read_dir(root)
            .with_context(|| format!("failed to list local project path {}", root.display()))?
        {
            if files.len() >= limit {
                break;
            }
            let entry = entry?;
            collect_local_project_files(&entry.path(), limit, files)?;
        }
    } else if is_local_project_file(root) {
        files.push(root.to_path_buf());
    }
    Ok(())
}

pub(crate) fn should_skip_local_project_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|item| item.to_str())
        .map(|name| {
            matches!(
                name,
                ".git"
                    | "target"
                    | "node_modules"
                    | "dist"
                    | "build"
                    | ".next"
                    | ".turbo"
                    | ".idea"
                    | ".vscode"
                    | "vendor"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn is_local_project_file(path: &Path) -> bool {
    path.extension()
        .and_then(|item| item.to_str())
        .map(|ext| {
            matches!(
                ext,
                "rs" | "md" | "txt" | "toml" | "json" | "yaml" | "yml" | "ts" | "tsx"
                    | "js" | "jsx" | "py" | "go" | "java" | "kt" | "c" | "cc" | "cpp"
                    | "h" | "hpp" | "swift"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn local_project_hit(path: &Path, topic: &str, query: &str) -> Result<Option<SearchHit>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read local project file {}", path.display()))?;
    let normalized_query = normalize(&format!("{topic} {query}"));
    let normalized_content = normalize(&content);
    if token_overlap(&normalized_query, &normalized_content) < 0.08 {
        return Ok(None);
    }
    let line = best_matching_line(&content, query).unwrap_or(0);
    let excerpt = local_excerpt(&content, query);
    Ok(Some(SearchHit {
        provider: "local_project".to_string(),
        url: format!("file://{}", path.display()),
        title: path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("local project file")
            .to_string(),
        summary: format!("Matched local project file at line {}", line + 1),
        text: excerpt,
        published_date: None,
        author: None,
    }))
}

pub(crate) fn best_matching_line(content: &str, query: &str) -> Option<usize> {
    let normalized_query = normalize(query);
    content
        .lines()
        .enumerate()
        .map(|(index, line)| (index, token_overlap(&normalized_query, &normalize(line))))
        .filter(|(_, score)| *score > 0.0)
        .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(index, _)| index)
}

pub(crate) fn local_excerpt(content: &str, query: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let line = best_matching_line(content, query).unwrap_or(0);
    let start = line.saturating_sub(2);
    let end = (line + 3).min(lines.len());
    lines[start..end]
        .iter()
        .map(|item| clean_text(item))
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn normalize_url_key(url: &str) -> String {
    url.trim_end_matches('/').to_lowercase()
}
