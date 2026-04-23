//! Recursive git worktrees.
//!
//! This crate creates a git worktree and, for every *nested* git repository
//! discovered under the source tree, creates a sibling detached-HEAD worktree
//! at the matching path inside the new worktree.
//!
//! It is aimed at layouts like GStreamer / meson subprojects, where
//! `subprojects/*` are independent git clones rather than submodules — and
//! built-in `git worktree add` only covers the top-level repo.
//!
//! Features beyond `git worktree add`:
//!
//! - Filesystem-based nested repo discovery (both `.git` directories and
//!   `.git` files / existing worktrees)
//! - Symlink-safe recursion with canonical-path dedup
//! - Submodule init with shared LFS storage from the source repo
//! - Recursive teardown that removes nested worktrees deepest-first and
//!   prunes stale refs in every involved git dir
//! - Pluggable file/directory copies via [`WorktreeBuilder`]
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::Path;
//! use git_recworktree::WorktreeBuilder;
//!
//! WorktreeBuilder::new(
//!     Path::new("/repo"),
//!     Path::new("/repo-worktrees/feature-x"),
//!     "feature-x",
//! )
//! .base_branch("origin/main")
//! .copy_file("NOTES.md")
//! .copy_dir(".vscode")
//! .create()
//! .unwrap();
//! ```

mod error;
mod info;
mod nested;
mod ops;

pub use error::{Error, Result};
pub use info::WorktreeInfo;
pub use nested::{find_nested_git_repos, find_nested_worktrees, NestedGitRepo};
pub use ops::{
    branch_exists, create_branch_if_needed, delete_branch, fetch_origin, remove_worktree,
    worktree_exists,
};

use std::path::{Path, PathBuf};

/// Directory names skipped by default during nested-repo discovery.
pub const DEFAULT_SKIP_DIRS: &[&str] =
    &["node_modules", "target", "_build", "build", "dist"];

/// Builder for creating a worktree and its nested worktrees.
///
/// See the crate-level docs for an overview. The builder collects
/// configuration and performs the whole operation when [`create`] is called.
///
/// [`create`]: Self::create
pub struct WorktreeBuilder<'a> {
    repo_path: &'a Path,
    worktree_path: &'a Path,
    branch_name: &'a str,
    base_branch: String,
    copy_files: Vec<String>,
    copy_dirs: Vec<String>,
    external_files: Vec<(PathBuf, PathBuf)>,
    skip_dirs: Vec<String>,
    share_lfs: bool,
    init_submodules: bool,
}

impl<'a> WorktreeBuilder<'a> {
    /// Start a new builder.
    ///
    /// * `repo_path` — the existing main repo (any worktree of it also works)
    /// * `worktree_path` — where the new worktree should be created
    /// * `branch_name` — branch checked out in the new worktree
    pub fn new(repo_path: &'a Path, worktree_path: &'a Path, branch_name: &'a str) -> Self {
        Self {
            repo_path,
            worktree_path,
            branch_name,
            base_branch: "origin/main".to_string(),
            copy_files: Vec::new(),
            copy_dirs: Vec::new(),
            external_files: Vec::new(),
            skip_dirs: DEFAULT_SKIP_DIRS.iter().map(|s| s.to_string()).collect(),
            share_lfs: true,
            init_submodules: true,
        }
    }

    /// Base ref to branch from if the target branch does not yet exist.
    /// Falls back to `origin/main`, `origin/master`, `HEAD` if invalid.
    /// Default: `origin/main`.
    pub fn base_branch(mut self, base: impl Into<String>) -> Self {
        self.base_branch = base.into();
        self
    }

    /// Copy a file from the main repo into the new worktree (relative path).
    pub fn copy_file(mut self, path: impl Into<String>) -> Self {
        self.copy_files.push(path.into());
        self
    }

    /// Copy a directory recursively from the main repo into the new worktree.
    pub fn copy_dir(mut self, path: impl Into<String>) -> Self {
        self.copy_dirs.push(path.into());
        self
    }

    /// Copy an external file (absolute path) into the worktree at a
    /// relative destination. Useful for shared dev-environment files
    /// living outside the repo.
    pub fn external_file(
        mut self,
        source: impl Into<PathBuf>,
        dest_in_worktree: impl Into<PathBuf>,
    ) -> Self {
        self.external_files
            .push((source.into(), dest_in_worktree.into()));
        self
    }

    /// Replace the skip list for nested-repo discovery. Default is
    /// [`DEFAULT_SKIP_DIRS`]. Hidden directories (starting with `.`) are
    /// always skipped regardless.
    pub fn skip_dirs(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.skip_dirs = names.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Add one more directory name to the skip list (does not replace).
    pub fn skip_dir(mut self, name: impl Into<String>) -> Self {
        self.skip_dirs.push(name.into());
        self
    }

    /// Whether to share LFS storage with the source repo's submodules.
    /// Default: true.
    pub fn share_lfs(mut self, yes: bool) -> Self {
        self.share_lfs = yes;
        self
    }

    /// Whether to run `git submodule update --init` in the new worktree.
    /// Default: true.
    pub fn init_submodules(mut self, yes: bool) -> Self {
        self.init_submodules = yes;
        self
    }

    /// Execute the worktree creation.
    pub fn create(self) -> Result<()> {
        ops::create_worktree(self)
    }

    // Accessors used by ops.rs.
    pub(crate) fn _repo_path(&self) -> &Path {
        self.repo_path
    }
    pub(crate) fn _worktree_path(&self) -> &Path {
        self.worktree_path
    }
    pub(crate) fn _branch_name(&self) -> &str {
        self.branch_name
    }
    pub(crate) fn _base_branch(&self) -> &str {
        &self.base_branch
    }
    pub(crate) fn _copy_files(&self) -> &[String] {
        &self.copy_files
    }
    pub(crate) fn _copy_dirs(&self) -> &[String] {
        &self.copy_dirs
    }
    pub(crate) fn _external_files(&self) -> &[(PathBuf, PathBuf)] {
        &self.external_files
    }
    pub(crate) fn _skip_dirs(&self) -> &[String] {
        &self.skip_dirs
    }
    pub(crate) fn _share_lfs(&self) -> bool {
        self.share_lfs
    }
    pub(crate) fn _init_submodules(&self) -> bool {
        self.init_submodules
    }
}
