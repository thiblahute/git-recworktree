use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Information about a git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Path to the worktree working directory.
    pub worktree_path: PathBuf,
    /// Path to the worktree admin dir (`main_repo/.git/worktrees/{name}/`).
    pub worktree_git_dir: PathBuf,
    /// Path to the main `.git` directory.
    pub main_git_dir: PathBuf,
    /// Path to the main repository working directory (for display / protection).
    ///
    /// Note: for separated git dirs (e.g. `repo.git/` alongside `repo/`),
    /// this may not be the actual working tree. Use `main_git_dir` with
    /// `--git-dir` when running git commands against the main repo.
    pub main_repo_path: PathBuf,
    /// Whether this is the main worktree (not a linked worktree).
    pub is_main_worktree: bool,
}

impl WorktreeInfo {
    /// Detect worktree info from a given path.
    pub fn from_path(path: &Path) -> Result<Self> {
        let path = path.canonicalize().map_err(|e| Error::GitFileRead {
            path: path.to_path_buf(),
            source: e,
        })?;

        let git_path = path.join(".git");

        if !git_path.exists() {
            return Err(Error::NotWorktree(path));
        }

        if git_path.is_dir() {
            // This is a main repository, not a worktree
            return Err(Error::IsMainRepo(path));
        }

        // .git is a file - this is a worktree
        // Format: "gitdir: /path/to/main/.git/worktrees/{name}"
        let git_content = std::fs::read_to_string(&git_path).map_err(|e| Error::GitFileRead {
            path: git_path.clone(),
            source: e,
        })?;

        let worktree_git_dir = parse_gitdir(&git_content, &git_path)?;
        let worktree_git_dir = worktree_git_dir
            .canonicalize()
            .map_err(|e| Error::GitFileRead {
                path: worktree_git_dir.clone(),
                source: e,
            })?;

        // Read commondir to find main .git directory. If commondir doesn't
        // exist, the gitdir itself is the main git dir (e.g. when .git is a
        // file pointing directly to a relocated git dir).
        let commondir_path = worktree_git_dir.join("commondir");
        let (main_git_dir, is_main_worktree) = if commondir_path.exists() {
            let commondir_content =
                std::fs::read_to_string(&commondir_path).map_err(|e| Error::GitFileRead {
                    path: commondir_path,
                    source: e,
                })?;

            let dir = worktree_git_dir
                .join(commondir_content.trim())
                .canonicalize()
                .map_err(|e| Error::GitFileRead {
                    path: worktree_git_dir.join(commondir_content.trim()),
                    source: e,
                })?;

            (dir, false)
        } else {
            (worktree_git_dir.clone(), true)
        };

        let main_repo_path = main_git_dir
            .parent()
            .ok_or_else(|| Error::InvalidPath("Main .git has no parent".into()))?
            .to_path_buf();

        Ok(WorktreeInfo {
            worktree_path: path,
            worktree_git_dir,
            main_git_dir,
            main_repo_path,
            is_main_worktree,
        })
    }
}

fn parse_gitdir(content: &str, git_path: &Path) -> Result<PathBuf> {
    let content = content.trim();
    if !content.starts_with("gitdir: ") {
        return Err(Error::InvalidGitFile(git_path.to_path_buf()));
    }
    Ok(PathBuf::from(&content[8..]))
}
