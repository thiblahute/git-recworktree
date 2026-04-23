//! `git-recworktree` — recursive git worktrees.
//!
//! When placed in $PATH this is invokable as `git recworktree`.

use clap::{Parser, Subcommand};
use git_recworktree::{delete_branch, remove_worktree, WorktreeBuilder, WorktreeInfo};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "git-recworktree")]
#[command(
    about = "Recursive git worktrees: creates sibling worktrees for nested repos (meson subprojects, vendored repos, etc.)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a worktree and sibling worktrees for every nested repo.
    Add {
        /// Branch name for the new worktree.
        branch: String,

        /// Path to the main repository (default: current directory).
        #[arg(long, default_value = ".")]
        repo: PathBuf,

        /// Base ref to branch from if the branch doesn't exist.
        #[arg(long, default_value = "origin/main")]
        base: String,

        /// Directory where the worktree will be created.
        /// Default: sibling of `repo` with the branch as basename.
        #[arg(long)]
        directory: Option<PathBuf>,

        /// Copy this file from the main repo into the new worktree.
        /// May be repeated.
        #[arg(long = "copy-file", value_name = "REL_PATH")]
        copy_files: Vec<String>,

        /// Copy this directory recursively from the main repo.
        /// May be repeated.
        #[arg(long = "copy-dir", value_name = "REL_PATH")]
        copy_dirs: Vec<String>,

        /// Additional directory name to skip during nested-repo discovery.
        /// May be repeated.
        #[arg(long = "skip-dir", value_name = "NAME")]
        skip_dirs: Vec<String>,

        /// Don't share LFS storage from the main repo's submodules.
        #[arg(long)]
        no_lfs_share: bool,

        /// Don't initialize submodules in the new worktree.
        #[arg(long)]
        no_submodules: bool,
    },

    /// Remove a worktree and all its nested worktrees.
    Remove {
        /// Path to the worktree to remove.
        path: PathBuf,

        /// Force removal even with uncommitted changes.
        #[arg(long, short)]
        force: bool,

        /// Also delete the branch after removing the worktree.
        #[arg(long)]
        delete_branch: bool,
    },
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add {
            branch,
            repo,
            base,
            directory,
            copy_files,
            copy_dirs,
            skip_dirs,
            no_lfs_share,
            no_submodules,
        } => {
            if !repo.exists() {
                return Err(format!("repo not found: {}", repo.display()).into());
            }
            let repo = repo.canonicalize()?;

            let worktree_dir = directory.unwrap_or_else(|| {
                repo.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let worktree_path = worktree_dir.join(&branch);

            let mut b = WorktreeBuilder::new(&repo, &worktree_path, &branch).base_branch(&base);
            for f in &copy_files {
                b = b.copy_file(f.clone());
            }
            for d in &copy_dirs {
                b = b.copy_dir(d.clone());
            }
            for s in &skip_dirs {
                b = b.skip_dir(s.clone());
            }
            if no_lfs_share {
                b = b.share_lfs(false);
            }
            if no_submodules {
                b = b.init_submodules(false);
            }

            b.create()?;
            eprintln!();
            eprintln!("Worktree created at: {}", worktree_path.display());
        }

        Commands::Remove {
            path,
            force,
            delete_branch: do_delete,
        } => {
            let branch_name = if do_delete {
                std::process::Command::new("git")
                    .args(["rev-parse", "--abbrev-ref", "HEAD"])
                    .current_dir(&path)
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                        } else {
                            None
                        }
                    })
            } else {
                None
            };

            let info = WorktreeInfo::from_path(&path);
            remove_worktree(&path, force)?;

            if let (Some(branch), Ok(wt_info)) = (&branch_name, info) {
                if branch != "HEAD" && !branch.is_empty() {
                    eprintln!("Deleting branch '{}'...", branch);
                    match delete_branch(&wt_info.main_repo_path, branch) {
                        Ok(true) => eprintln!("  Branch '{}' deleted", branch),
                        Ok(false) => eprintln!("  Branch '{}' not found", branch),
                        Err(e) => eprintln!("  Warning: {}", e),
                    }
                }
            }
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
