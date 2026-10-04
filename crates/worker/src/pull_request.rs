//! `peter-evans/create-pull-request`.
//!
//! The replay pushes nothing and opens no pull request. It records what the
//! action would propose: the files that changed in the repository of `path`
//! against its checked-out commit, limited to `add-paths`, each with its size
//! and SHA-256; a deleted file has the SHA-256 `deleted`.

use anyhow::{bail, Context, Result};
use build_machine_core::report::Artifact;
use std::path::Path;
use std::process::Command;

/// The files the action would commit in the repository at `root`, sorted by
/// path. `add_paths` holds the paths of the action's `add-paths`, one per
/// line or comma-separated.
pub fn proposed(root: &Path, add_paths: Option<&str>) -> Result<Vec<Artifact>> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--"]);
    if let Some(paths) = add_paths {
        command.args(paths.split([',', '\n']).map(str::trim).filter(|path| !path.is_empty()));
    }
    let output = command.output().context("git status를 실행하지 못했어요.")?;
    if !output.status.success() {
        bail!("git status가 실패했어요: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let text = String::from_utf8(output.stdout).context("git status 출력이 UTF-8이 아니에요.")?;
    let mut files = Vec::new();
    let mut entries = text.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (status, path) = entry.split_at(3.min(entry.len()));
        // A rename lists the new path, then the old path as its own entry.
        if status.starts_with('R') || status.starts_with('C') {
            entries.next();
        }
        let file = root.join(path);
        files.push(if file.is_file() {
            Artifact {
                path: path.to_owned(),
                sha256: build_machine_core::source::sha256_file(&file)?,
                size: std::fs::metadata(&file)?.len(),
            }
        } else {
            Artifact { path: path.to_owned(), sha256: "deleted".to_owned(), size: 0 }
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}
