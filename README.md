# git-recworktree

Recursive `git worktree` for repos that contain nested *independent* git
repositories — the typical meson-subprojects layout used by GStreamer and
others, where `subprojects/foo` is its own full clone rather than a
submodule.

`git worktree add` only creates a worktree for the top-level repo; the
nested clones are left as unrelated files in the new tree. `git-recworktree`
discovers them by walking the filesystem and creates a detached-HEAD sibling
worktree for each, at the same relative path inside the new worktree.

## Installation

```
cargo install --path .
```

Once the `git-recworktree` binary is in `$PATH`, git invokes it as
`git recworktree …`.

## Usage

```
git recworktree add feature-x              # sibling of CWD
git recworktree add feature-x --repo /path/to/repo
git recworktree remove ../worktrees/feature-x --force
git recworktree remove ../worktrees/feature-x --delete-branch
```

Optional flags:

- `--copy-file REL_PATH` — copy a file from the main repo into the worktree
  (e.g. `NOTES.md`, `.envrc`). Repeatable.
- `--copy-dir REL_PATH` — same but a directory, copied recursively.
- `--skip-dir NAME` — extra directory name to skip during nested-repo
  discovery (hidden dirs and `node_modules`, `target`, `_build`, `build`,
  `dist` are skipped by default).
- `--no-lfs-share` — don't point submodule `lfs.storage` at the main repo's
  LFS cache.
- `--no-submodules` — skip `git submodule update --init`.

## Library

```toml
[dependencies]
git-recworktree = { path = "../git-recworktree" }  # or a git URL
```

```rust
use git_recworktree::WorktreeBuilder;

WorktreeBuilder::new(&repo, &worktree_path, "feature-x")
    .base_branch("origin/main")
    .copy_file("NOTES.md")
    .copy_dir(".vscode")
    .create()?;
```

## What it does beyond `git worktree add`

- Filesystem-based nested-repo discovery (both `.git` dirs and `.git` files)
- Symlink-safe recursion with canonical-path dedup
- `git submodule update --init` with `GIT_LFS_SKIP_SMUDGE=1`, then
  point submodule `lfs.storage` at the main repo's LFS cache
- Recursive teardown: deepest-first, deinit submodules, prune refs in every
  involved git dir
- Works when the main repo uses a separated git dir (`repo.git/` alongside
  `repo/`)

## License

MIT OR Apache-2.0
