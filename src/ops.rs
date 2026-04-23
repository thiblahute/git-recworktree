use crate::error::{Error, Result};
use crate::info::WorktreeInfo;
use crate::nested::{find_nested_git_repos_with, find_nested_worktrees};
use crate::WorktreeBuilder;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Fetch from origin.
pub fn fetch_origin(repo_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(repo_path)
        .status()?;

    if !status.success() {
        return Err(Error::GitCommand("git fetch origin failed".into()));
    }
    Ok(())
}

fn ref_exists(repo_path: &Path, refname: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", refname])
        .current_dir(repo_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn local_branch_exists(repo_path: &Path, branch_name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["branch", "--list", branch_name])
        .current_dir(repo_path)
        .output()?;
    Ok(!output.stdout.is_empty() && !String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn remote_branch_exists(repo_path: &Path, branch_name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["branch", "-r", "--list", &format!("origin/{}", branch_name)])
        .current_dir(repo_path)
        .output()?;
    Ok(!output.stdout.is_empty() && !String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// Whether a branch exists locally or on origin.
pub fn branch_exists(repo_path: &Path, branch_name: &str) -> Result<bool> {
    Ok(local_branch_exists(repo_path, branch_name)? || remote_branch_exists(repo_path, branch_name)?)
}

/// Create a local branch from `base_branch` if it doesn't already exist.
///
/// If the branch already exists locally, nothing happens. If it exists on
/// origin, a tracking branch is created. Otherwise the branch is created
/// from `base_branch`, falling back through `origin/main`, `origin/master`,
/// `HEAD`.
pub fn create_branch_if_needed(
    repo_path: &Path,
    branch_name: &str,
    base_branch: &str,
) -> Result<()> {
    if local_branch_exists(repo_path, branch_name)? {
        eprintln!("  Branch '{}' already exists locally", branch_name);
        return Ok(());
    }

    if remote_branch_exists(repo_path, branch_name)? {
        eprintln!(
            "  Creating local branch '{}' tracking 'origin/{}'",
            branch_name, branch_name
        );
        let status = Command::new("git")
            .args(["branch", branch_name, &format!("origin/{}", branch_name)])
            .current_dir(repo_path)
            .status()?;
        if !status.success() {
            return Err(Error::GitCommand(format!(
                "Failed to create tracking branch {} from origin/{}",
                branch_name, branch_name
            )));
        }
        return Ok(());
    }

    let bases = [base_branch, "origin/main", "origin/master", "HEAD"];
    for base in &bases {
        if !ref_exists(repo_path, base) {
            continue;
        }
        eprintln!("  Creating branch '{}' from '{}'", branch_name, base);
        let status = Command::new("git")
            .args(["branch", branch_name, base])
            .current_dir(repo_path)
            .status()?;
        if status.success() {
            return Ok(());
        }
    }

    Err(Error::GitCommand(format!(
        "Failed to create branch {} — no valid base ref found",
        branch_name,
    )))
}

/// Whether a worktree at `worktree_path` is registered for `repo_path`.
pub fn worktree_exists(repo_path: &Path, worktree_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let worktree_str = worktree_path.to_string_lossy();
    Ok(output_str.contains(&*worktree_str))
}

pub(crate) fn create_worktree(b: WorktreeBuilder<'_>) -> Result<()> {
    let repo_path = b._repo_path();
    let worktree_path = b._worktree_path();
    let branch_name = b._branch_name();

    eprintln!("Setting up worktree at: {}", worktree_path.display());
    eprintln!("  Fetching origin...");
    fetch_origin(repo_path)?;

    create_branch_if_needed(repo_path, branch_name, b._base_branch())?;

    if worktree_exists(repo_path, worktree_path)? {
        eprintln!("  Worktree already exists at {}", worktree_path.display());
        return Ok(());
    }

    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    eprintln!("  Creating worktree...");
    let status = Command::new("git")
        .args([
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            branch_name,
        ])
        .current_dir(repo_path)
        .status()?;

    if !status.success() {
        return Err(Error::GitCommand(format!(
            "Failed to create worktree at {}",
            worktree_path.display()
        )));
    }

    let worktree_path = worktree_path.canonicalize().map_err(|e| Error::GitFileRead {
        path: worktree_path.to_path_buf(),
        source: e,
    })?;

    // Create detached-HEAD worktrees for all nested git repos. This
    // includes repos where `.git` is a file (existing worktrees): `git
    // worktree add` works from a worktree and creates a sibling worktree
    // backed by the same underlying repo.
    let nested_repos = find_nested_git_repos_with(repo_path, b._skip_dirs());
    for nested in &nested_repos {
        let nested_worktree = worktree_path.join(&nested.relative_path);
        eprintln!(
            "  Setting up nested worktree for {}...",
            nested.relative_path.display()
        );

        if worktree_exists(&nested.path, &nested_worktree).unwrap_or(false) {
            eprintln!(
                "  {} worktree already exists",
                nested.relative_path.display()
            );
            continue;
        }

        // Remove leftover directory if it's not a git repo (left behind
        // by the parent checkout).
        if nested_worktree.exists() && !nested_worktree.join(".git").exists() {
            eprintln!(
                "  Removing leftover directory at {}...",
                nested.relative_path.display()
            );
            if let Err(e) = std::fs::remove_dir_all(&nested_worktree) {
                eprintln!(
                    "  Warning: Failed to remove {}: {}",
                    nested.relative_path.display(),
                    e
                );
                continue;
            }
        }

        if let Some(parent) = nested_worktree.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let wt_path_str = nested_worktree.to_string_lossy().to_string();
        let output = Command::new("git")
            .args(["worktree", "add", "--detach", &wt_path_str])
            .current_dir(&nested.path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                eprintln!("  {} worktree created", nested.relative_path.display());
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!(
                    "  Warning: Failed to create worktree for {}: {}",
                    nested.relative_path.display(),
                    stderr.trim()
                );
            }
            Err(e) => {
                eprintln!(
                    "  Warning: Failed to run git for {}: {}",
                    nested.relative_path.display(),
                    e
                );
            }
        }
    }

    if b._init_submodules() {
        setup_submodules(repo_path, &worktree_path, branch_name, b._share_lfs())?;
    }

    // Copy configured files / dirs from the main repo.
    for rel in b._copy_files() {
        let src = repo_path.join(rel);
        if src.exists() {
            let dst = worktree_path.join(rel);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            eprintln!("  Copying {} to worktree...", rel);
            if let Err(e) = std::fs::copy(&src, &dst) {
                eprintln!("  Warning: Failed to copy {}: {}", rel, e);
            }
        }
    }

    for rel in b._copy_dirs() {
        let src = repo_path.join(rel);
        if src.is_dir() {
            eprintln!("  Copying {}/ to worktree...", rel);
            if let Err(e) = copy_dir_recursive(&src, &worktree_path.join(rel)) {
                eprintln!("  Warning: Failed to copy {}/: {}", rel, e);
            }
        }
    }

    for (src, rel_dst) in b._external_files() {
        if src.exists() {
            let dst = worktree_path.join(rel_dst);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            eprintln!(
                "  Copying external file {} -> {}...",
                src.display(),
                rel_dst.display()
            );
            if let Err(e) = std::fs::copy(src, &dst) {
                eprintln!("  Warning: Failed to copy {}: {}", src.display(), e);
            }
        }
    }

    eprintln!("  Worktree created successfully");
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Delete a local branch. `Ok(true)` if deleted, `Ok(false)` if missing.
pub fn delete_branch(repo_path: &Path, branch_name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["branch", "-d", branch_name])
        .current_dir(repo_path)
        .output()?;

    if output.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") {
            Ok(false)
        } else {
            Err(Error::GitCommand(format!(
                "Failed to delete branch '{}': {}",
                branch_name,
                stderr.trim()
            )))
        }
    }
}

fn git_with_dir(git_dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--git-dir");
    cmd.arg(git_dir);
    cmd
}

/// Remove a worktree and all its nested worktrees (deepest-first).
pub fn remove_worktree(worktree_path: &Path, force: bool) -> Result<()> {
    let worktree_path = worktree_path.canonicalize().map_err(|e| Error::GitFileRead {
        path: worktree_path.to_path_buf(),
        source: e,
    })?;

    let main_info = WorktreeInfo::from_path(&worktree_path)?;
    eprintln!("Removing worktree at: {}", worktree_path.display());
    eprintln!("  Main git dir: {}", main_info.main_git_dir.display());

    let mut nested = find_nested_worktrees(&worktree_path);

    // Deepest first so children go before parents.
    nested.sort_by(|a, b| {
        b.worktree_path
            .components()
            .count()
            .cmp(&a.worktree_path.components().count())
    });

    let mut git_dirs_to_prune: Vec<PathBuf> =
        nested.iter().map(|n| n.main_git_dir.clone()).collect();
    git_dirs_to_prune.push(main_info.main_git_dir.clone());
    git_dirs_to_prune.sort();
    git_dirs_to_prune.dedup();

    for nested_wt in &nested {
        eprintln!(
            "  Removing nested worktree: {}",
            nested_wt.worktree_path.display()
        );

        if nested_wt.worktree_path.join(".gitmodules").exists() {
            let _ = Command::new("git")
                .args(["submodule", "deinit", "--all", "--force"])
                .current_dir(&nested_wt.worktree_path)
                .status();
        }

        let wt_str = nested_wt.worktree_path.to_string_lossy().to_string();
        let mut cmd = git_with_dir(&nested_wt.main_git_dir);
        cmd.args(["worktree", "remove"]);
        if force {
            cmd.arg("--force");
        }
        cmd.arg(&wt_str);

        match cmd.output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("  Warning: {}", stderr.trim());
            }
            Err(e) => {
                eprintln!("  Warning: Failed to run git: {}", e);
            }
        }
    }

    // `git worktree remove` refuses to remove a worktree that has submodule
    // metadata in its admin dir, so deinit submodules first and clean up.
    if worktree_path.join(".gitmodules").exists() {
        eprintln!("  Deinitializing submodules...");
        let _ = Command::new("git")
            .args(["submodule", "deinit", "--all", "--force"])
            .current_dir(&worktree_path)
            .status();

        let modules_dir = main_info.worktree_git_dir.join("modules");
        if modules_dir.exists() {
            eprintln!("  Removing submodule metadata...");
            if let Err(e) = std::fs::remove_dir_all(&modules_dir) {
                eprintln!("  Warning: Failed to remove submodule metadata: {}", e);
            }
        }
    }

    eprintln!("  Removing main worktree...");
    let wt_str = worktree_path.to_string_lossy().to_string();
    let mut cmd = git_with_dir(&main_info.main_git_dir);
    cmd.args(["worktree", "remove"]);
    if force {
        cmd.arg("--force");
    }
    cmd.arg(&wt_str);

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitCommand(format!(
            "Failed to remove worktree: {}",
            stderr.trim()
        )));
    }

    for git_dir in &git_dirs_to_prune {
        let _ = git_with_dir(git_dir)
            .args(["worktree", "prune"])
            .status();
    }

    eprintln!("Worktree removed successfully");
    Ok(())
}

fn get_shared_lfs_storage(submodule_path: &Path) -> Option<PathBuf> {
    if !submodule_path.exists() {
        return None;
    }

    let output = Command::new("git")
        .args(["config", "lfs.storage"])
        .current_dir(submodule_path)
        .output()
        .ok()?;

    if output.status.success() {
        let storage = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !storage.is_empty() {
            return Some(PathBuf::from(storage));
        }
    }

    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(submodule_path)
        .output()
        .ok()?;

    if output.status.success() {
        let git_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let git_dir = PathBuf::from(&git_dir_str);
        let git_dir = if git_dir.is_absolute() {
            git_dir
        } else {
            submodule_path.join(&git_dir)
        };
        if let Ok(resolved) = git_dir.canonicalize() {
            return Some(resolved.join("lfs"));
        }
    }

    None
}

fn setup_submodules(
    repo_path: &Path,
    worktree_path: &Path,
    branch_name: &str,
    share_lfs: bool,
) -> Result<()> {
    let gitmodules = worktree_path.join(".gitmodules");
    if !gitmodules.exists() {
        return Ok(());
    }

    eprintln!("  Initializing submodules (skipping LFS smudge)...");

    let status = Command::new("git")
        .args(["submodule", "update", "--init"])
        .current_dir(worktree_path)
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .status()?;

    if !status.success() {
        eprintln!("  Warning: Failed to initialize submodules");
        return Ok(());
    }

    let output = Command::new("git")
        .args(["submodule", "foreach", "--quiet", "echo $sm_path"])
        .current_dir(worktree_path)
        .output()?;

    let submodule_paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();

    for sm_path in &submodule_paths {
        let worktree_sm = worktree_path.join(sm_path);
        let main_sm = repo_path.join(sm_path);

        if !worktree_sm.exists() {
            continue;
        }

        if share_lfs {
            if let Some(shared_lfs) = get_shared_lfs_storage(&main_sm) {
                if shared_lfs.exists() {
                    eprintln!(
                        "  Sharing LFS storage for {} from: {}",
                        sm_path,
                        shared_lfs.display()
                    );
                    let _ = Command::new("git")
                        .args(["config", "lfs.storage", &shared_lfs.to_string_lossy()])
                        .current_dir(&worktree_sm)
                        .status();

                    eprintln!("  Running LFS checkout for {}...", sm_path);
                    let _ = Command::new("git")
                        .args(["lfs", "checkout"])
                        .current_dir(&worktree_sm)
                        .status();
                }
            }
        }

        if !branch_exists(&worktree_sm, branch_name).unwrap_or(false) {
            let base = if remote_branch_exists(&worktree_sm, "main").unwrap_or(false) {
                "origin/main"
            } else {
                "HEAD"
            };
            eprintln!(
                "  Creating branch '{}' from '{}' in {}",
                branch_name, base, sm_path
            );
            let _ = Command::new("git")
                .args(["checkout", "-b", branch_name])
                .current_dir(&worktree_sm)
                .status();
        } else {
            eprintln!("  Checking out branch '{}' in {}", branch_name, sm_path);
            let _ = Command::new("git")
                .args(["checkout", branch_name])
                .current_dir(&worktree_sm)
                .status();
        }
    }

    eprintln!("  Submodules ready");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nested::{find_nested_git_repos, find_nested_worktrees};
    use std::fs;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
            .env("GIT_CONFIG_VALUE_0", "always")
            .output()
            .unwrap_or_else(|e| panic!("Failed to run git {:?}: {}", args, e));
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("git {:?} failed in {}: {}", args, dir.display(), stderr);
        }
    }

    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init"]);
        git(path, &["config", "user.email", "test@test.com"]);
        git(path, &["config", "user.name", "Test"]);
        git(path, &["config", "protocol.file.allow", "always"]);
        fs::write(path.join("README"), "hello").unwrap();
        git(path, &["add", "README"]);
        git(path, &["commit", "-m", "initial"]);
    }

    fn create_worktree_raw(repo: &Path, wt_path: &Path, branch: &str) {
        git(repo, &["branch", branch]);
        fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
        git(repo, &["worktree", "add", &wt_path.to_string_lossy(), branch]);
    }

    fn create_nested_worktrees_for_tests(main_repo: &Path, wt_path: &Path) {
        let nested = find_nested_git_repos(main_repo);
        for n in &nested {
            if !n.path.join(".git").is_dir() {
                continue;
            }
            let nested_wt = wt_path.join(&n.relative_path);
            if nested_wt.exists() && !nested_wt.join(".git").exists() {
                fs::remove_dir_all(&nested_wt).unwrap();
            }
            if let Some(parent) = nested_wt.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            git(
                &n.path,
                &["worktree", "add", "--detach", &nested_wt.to_string_lossy()],
            );
        }
    }

    /// Repo structure mimicking a GStreamer-style layout with a separated
    /// git dir and nested subproject repos.
    fn setup_separated_repo_with_nested(tmp: &Path) -> (PathBuf, PathBuf) {
        let root = tmp.to_path_buf();
        let main_repo = root.join("main");
        let git_dir = root.join("main.git");

        init_repo(&main_repo);

        fs::rename(main_repo.join(".git"), &git_dir).unwrap();
        fs::write(
            main_repo.join(".git"),
            format!("gitdir: {}", git_dir.display()),
        )
        .unwrap();

        let sub_a = main_repo.join("subprojects/sub-a");
        init_repo(&sub_a);

        let sub_b = main_repo.join("subprojects/sub-b");
        init_repo(&sub_b);

        git(&main_repo, &["add", "subprojects"]);
        git(&main_repo, &["commit", "-m", "add subprojects"]);

        (root, main_repo)
    }

    #[test]
    fn test_worktree_info_separated_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, main_repo) = setup_separated_repo_with_nested(tmp.path());
        let git_dir = root.join("main.git");

        let wt_path = root.join("worktrees/test-branch");
        create_worktree_raw(&main_repo, &wt_path, "test-branch");

        let info = WorktreeInfo::from_path(&wt_path).unwrap();
        assert_eq!(info.worktree_path, wt_path.canonicalize().unwrap());
        assert_eq!(info.main_git_dir, git_dir.canonicalize().unwrap());
        assert!(!info.is_main_worktree);
    }

    #[test]
    fn test_create_and_remove_worktree_with_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, main_repo) = setup_separated_repo_with_nested(tmp.path());

        let wt_path = root.join("worktrees/feature-x");
        create_worktree_raw(&main_repo, &wt_path, "feature-x");
        create_nested_worktrees_for_tests(&main_repo, &wt_path);

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());

        let nested_a = wt_path.join("subprojects/sub-a");
        let nested_b = wt_path.join("subprojects/sub-b");
        assert!(nested_a.join(".git").exists(), "sub-a worktree not created");
        assert!(nested_b.join(".git").exists(), "sub-b worktree not created");

        let nested = find_nested_worktrees(&wt_path.canonicalize().unwrap());
        assert!(
            nested.len() >= 2,
            "Expected at least 2 nested worktrees, found {}",
            nested.len()
        );

        remove_worktree(&wt_path, true).unwrap();

        assert!(!wt_path.exists(), "Worktree directory should be removed");

        let output = Command::new("git")
            .args([
                "--git-dir",
                &root.join("main.git").to_string_lossy(),
                "worktree",
                "list",
            ])
            .output()
            .unwrap();
        let list = String::from_utf8_lossy(&output.stdout);
        assert!(
            !list.contains("feature-x"),
            "Stale worktree reference found in main repo: {}",
            list
        );
    }

    #[test]
    fn test_remove_worktree_force_with_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, main_repo) = setup_separated_repo_with_nested(tmp.path());
        let wt_path = root.join("worktrees/dirty");

        create_worktree_raw(&main_repo, &wt_path, "dirty");
        create_nested_worktrees_for_tests(&main_repo, &wt_path);

        fs::write(wt_path.join("dirty-file.txt"), "uncommitted").unwrap();
        git(&wt_path, &["add", "dirty-file.txt"]);

        let result = remove_worktree(&wt_path, false);
        assert!(
            result.is_err(),
            "Should fail without --force on dirty worktree"
        );
        assert!(
            wt_path.exists(),
            "Worktree should still exist after failed remove"
        );

        remove_worktree(&wt_path, true).unwrap();
        assert!(!wt_path.exists(), "Worktree should be removed with --force");
    }

    #[test]
    fn test_remove_worktree_with_submodules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let sub_remote = root.join("sub-remote");
        init_repo(&sub_remote);

        let main_repo = root.join("main");
        init_repo(&main_repo);

        git(
            &main_repo,
            &[
                "submodule",
                "add",
                &sub_remote.to_string_lossy(),
                "my-submodule",
            ],
        );
        git(&main_repo, &["commit", "-m", "add submodule"]);

        let wt_path = root.join("worktrees/with-sub");
        create_worktree_raw(&main_repo, &wt_path, "with-sub");

        git(&wt_path, &["submodule", "update", "--init"]);
        assert!(wt_path.join("my-submodule/.git").exists());

        remove_worktree(&wt_path, false).unwrap();
        assert!(
            !wt_path.exists(),
            "Worktree with submodule should be removed"
        );
    }

    #[test]
    fn test_delete_branch_after_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let main_repo = root.join("main");
        init_repo(&main_repo);

        let wt_path = root.join("worktrees/to-delete");
        create_worktree_raw(&main_repo, &wt_path, "to-delete");

        remove_worktree(&wt_path, false).unwrap();

        assert!(branch_exists(&main_repo, "to-delete").unwrap());

        let deleted = delete_branch(&main_repo, "to-delete").unwrap();
        assert!(deleted);
        assert!(!branch_exists(&main_repo, "to-delete").unwrap());
    }
}
