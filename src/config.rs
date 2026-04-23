//! Per-repo / per-user configuration read from `git config`.
//!
//! `git-recworktree` uses git's own config mechanism rather than a
//! bespoke file. That means values can live anywhere git already looks
//! (`.git/config`, `~/.gitconfig`, system config, includes) and be
//! managed with the usual `git config` commands.
//!
//! # Keys
//!
//! | Key | Cardinality | Format | Meaning |
//! |-----|-------------|--------|---------|
//! | `recworktree.copy` | multi | relative path | File or directory under the repo root to copy into each new worktree. Auto-detected at apply time. |
//! | `recworktree.external` | multi | `SRC:DST` | Absolute-path source file to copy to the relative destination inside the worktree. |
//! | `recworktree.skipDir` | multi | name | Extra directory name to exclude during nested-repo discovery. |
//!
//! # Setting values
//!
//! ```sh
//! # Per-repo (writes to .git/config)
//! git config --add recworktree.copy NOTES.md
//! git config --add recworktree.copy .envrc
//! git config --add recworktree.copy .vscode
//! git config --add recworktree.skipDir yolotarget
//!
//! # Per-user (writes to ~/.gitconfig) — good for machine-specific externals
//! git config --global --add recworktree.external \
//!     "$HOME/.local/share/meson/native/gst.native:gst.native"
//! ```
//!
//! Or edit the config file directly:
//!
//! ```text
//! [recworktree]
//!     copy = NOTES.md
//!     copy = .envrc
//!     copy = .vscode
//!     external = /home/me/.local/share/meson/native/gst.native:gst.native
//!     skipDir = yolotarget
//! ```

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Config values read from git.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepoConfig {
    /// Values of `recworktree.copy` — paths relative to the repo root.
    /// File vs. directory is decided at apply time.
    pub copies: Vec<PathBuf>,
    /// Values of `recworktree.external`, parsed as `(SRC, DST)`.
    pub externals: Vec<(PathBuf, PathBuf)>,
    /// Values of `recworktree.skipDir`.
    pub skip_dirs: Vec<String>,
}

impl RepoConfig {
    /// Load config by running `git config --get-all` in `repo_path`.
    ///
    /// Reads from all levels (system, global, local) the same way any
    /// git command would. Returns an empty config if no values are set.
    pub fn load(repo_path: &Path) -> Result<Self> {
        let copies_raw = git_get_all(repo_path, "recworktree.copy")?;
        let externals_raw = git_get_all(repo_path, "recworktree.external")?;
        let skip_dirs = git_get_all(repo_path, "recworktree.skipdir")?;

        let copies = copies_raw.into_iter().map(PathBuf::from).collect();

        let mut externals = Vec::with_capacity(externals_raw.len());
        for entry in externals_raw {
            externals.push(parse_external(&entry)?);
        }

        Ok(Self {
            copies,
            externals,
            skip_dirs,
        })
    }
}

fn git_get_all(repo_path: &Path, key: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["config", "--get-all", key])
        .output()?;

    // Exit 1 with no stderr = key not set. That's normal, not an error.
    match output.status.code() {
        Some(0) => {}
        Some(1) if output.stderr.is_empty() => return Ok(Vec::new()),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::GitCommand(format!(
                "git config --get-all {} failed: {}",
                key,
                stderr.trim()
            )));
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

fn parse_external(value: &str) -> Result<(PathBuf, PathBuf)> {
    let (src, dst) = value.split_once(':').ok_or_else(|| {
        Error::InvalidPath(format!(
            "recworktree.external value '{}' must be 'SRC:DST'",
            value
        ))
    })?;
    if src.is_empty() || dst.is_empty() {
        return Err(Error::InvalidPath(format!(
            "recworktree.external value '{}' has empty SRC or DST",
            value
        )));
    }
    Ok((PathBuf::from(src), PathBuf::from(dst)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("git {:?} failed to spawn: {}", args, e));
        if !output.status.success() {
            panic!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init"]);
        git(path, &["config", "user.email", "t@t"]);
        git(path, &["config", "user.name", "t"]);
    }

    #[test]
    fn empty_repo_gives_empty_config() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        let cfg = RepoConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg, RepoConfig::default());
    }

    #[test]
    fn reads_copy_and_skipdir() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["config", "--add", "recworktree.copy", "NOTES.md"]);
        git(tmp.path(), &["config", "--add", "recworktree.copy", ".envrc"]);
        git(tmp.path(), &["config", "--add", "recworktree.copy", ".vscode"]);
        git(tmp.path(), &["config", "--add", "recworktree.skipDir", "yolotarget"]);

        let cfg = RepoConfig::load(tmp.path()).unwrap();
        assert_eq!(
            cfg.copies,
            vec![
                PathBuf::from("NOTES.md"),
                PathBuf::from(".envrc"),
                PathBuf::from(".vscode"),
            ]
        );
        assert_eq!(cfg.skip_dirs, vec!["yolotarget".to_string()]);
    }

    #[test]
    fn reads_external_pairs() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(
            tmp.path(),
            &[
                "config",
                "--add",
                "recworktree.external",
                "/abs/src.ini:dest/out.ini",
            ],
        );

        let cfg = RepoConfig::load(tmp.path()).unwrap();
        assert_eq!(
            cfg.externals,
            vec![(
                PathBuf::from("/abs/src.ini"),
                PathBuf::from("dest/out.ini")
            )]
        );
    }

    #[test]
    fn external_without_colon_errors() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(
            tmp.path(),
            &["config", "--add", "recworktree.external", "/no-colon"],
        );
        let err = RepoConfig::load(tmp.path()).unwrap_err();
        assert!(format!("{}", err).contains("must be 'SRC:DST'"));
    }

    #[test]
    fn casing_is_ignored_for_keys() {
        // git config treats variable names (the part after the section) as
        // case-insensitive. Using `skipdir` on write, `skipDir` on read.
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["config", "--add", "recworktree.skipdir", "foo"]);
        let cfg = RepoConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.skip_dirs, vec!["foo"]);
    }
}
