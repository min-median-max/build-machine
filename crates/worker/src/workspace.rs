//! Bounding what a worker leaves on the machine it builds on.
//!
//! Each build extracts the source and compiles it, so one directory is a whole
//! dependency tree — gigabytes, not the kilobytes a run report costs. The
//! controller bounds its own records; without this the machine that does the
//! actual work fills its disk instead.

use anyhow::Result;
use build_machine_core::config::Retention;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

struct Workspace {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

fn tree_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.metadata().map(|value| value.len()).unwrap_or(0))
        .sum()
}

/// Keep the newest build directories that fit under the declared byte cap,
/// discarding the oldest first. The current build and whatever the latest
/// receipt points at are never removed: one is in use, the other is what `run`
/// would launch.
pub fn prune(root: &Path, current: &Path, policy: &Retention) -> Result<()> {
    let referenced = std::fs::read(root.join("latest.json"))
        .ok()
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
        .and_then(|value| value.get("executable").and_then(|value| value.as_str()).map(str::to_owned))
        .map(PathBuf::from);
    let protected = |path: &Path| {
        path == current
            || referenced.as_ref().is_some_and(|executable| executable.starts_with(path))
    };

    let mut workspaces = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return Ok(()) };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || protected(&path) {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|value| value.modified()) else { continue };
        workspaces.push(Workspace { size: tree_size(&path), path, modified });
    }
    workspaces.sort_by_key(|workspace| workspace.modified);

    let mut total: u64 = tree_size(current) + workspaces.iter().map(|value| value.size).sum::<u64>();
    for workspace in &workspaces {
        if total <= policy.max_bytes {
            break;
        }
        if std::fs::remove_dir_all(&workspace.path).is_ok() {
            println!("Removed an earlier build directory: {}", workspace.path.display());
            total = total.saturating_sub(workspace.size);
        }
    }
    Ok(())
}
