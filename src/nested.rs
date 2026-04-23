use crate::info::WorktreeInfo;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A nested git repository found inside a parent tree.
#[derive(Debug, Clone)]
pub struct NestedGitRepo {
    /// Absolute path to the nested repo.
    pub path: PathBuf,
    /// Path relative to the root that was scanned.
    pub relative_path: PathBuf,
}

/// Find nested worktrees (`.git` is a file) inside `worktree_path`.
///
/// Uses the default skip list. For a custom skip list use
/// [`find_nested_git_repos_with`] and check the `.git` kind yourself.
pub fn find_nested_worktrees(worktree_path: &Path) -> Vec<WorktreeInfo> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    if let Ok(canonical) = worktree_path.canonicalize() {
        visited.insert(canonical);
    }
    let skips = crate::DEFAULT_SKIP_DIRS
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    walk(
        worktree_path,
        &skips,
        &mut visited,
        &mut result,
        |path, _| {
            let git_path = path.join(".git");
            if git_path.is_file() {
                WorktreeInfo::from_path(path).ok()
            } else {
                None
            }
        },
    );
    result
}

/// Find all nested git repositories (any `.git` — file or dir) inside `root`.
/// Uses the default skip list.
pub fn find_nested_git_repos(root: &Path) -> Vec<NestedGitRepo> {
    let skips = crate::DEFAULT_SKIP_DIRS
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    find_nested_git_repos_with(root, &skips)
}

/// Like [`find_nested_git_repos`] but with an explicit skip list (in
/// addition to hidden directories, which are always skipped).
pub fn find_nested_git_repos_with(root: &Path, skip_dirs: &[String]) -> Vec<NestedGitRepo> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    if let Ok(canonical) = root.canonicalize() {
        visited.insert(canonical);
    }
    walk(root, skip_dirs, &mut visited, &mut result, |path, root| {
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        Some(NestedGitRepo {
            path: path.to_path_buf(),
            relative_path: relative,
        })
    });
    result
}

fn walk<T, F>(
    dir: &Path,
    skip_dirs: &[String],
    visited: &mut HashSet<PathBuf>,
    result: &mut Vec<T>,
    collector: F,
) where
    F: Fn(&Path, &Path) -> Option<T> + Copy,
{
    walk_inner(dir, dir, skip_dirs, visited, result, collector);
}

fn walk_inner<T, F>(
    root: &Path,
    dir: &Path,
    skip_dirs: &[String],
    visited: &mut HashSet<PathBuf>,
    result: &mut Vec<T>,
    collector: F,
) where
    F: Fn(&Path, &Path) -> Option<T> + Copy,
{
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }
        if skip_dirs.iter().any(|s| s == name_str.as_ref()) {
            continue;
        }

        // Follow symlinks (`path.is_dir()` resolves them) so symlinked
        // subprojects are found. Dedup by canonical path so a symlink
        // pointing back up doesn't cause infinite recursion.
        if path.is_dir() {
            let canonical = match path.canonicalize() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !visited.insert(canonical) {
                continue;
            }

            let git_path = path.join(".git");
            if git_path.exists() {
                if let Some(item) = collector(&path, root) {
                    result.push(item);
                }
            }

            // Always recurse — even into nested git repos — so we can find
            // deeply nested layouts like `foo/bar/.git` + `foo/bar/subprojects/baz/.git`.
            walk_inner(root, &path, skip_dirs, visited, result, collector);
        }
    }
}
