//! Bounding recorded runs.
//!
//! A run is its report plus the logs that report points at, so both are sized
//! and removed together: pruning only reports would orphan the logs forever,
//! and pruning only logs would break the paths a retained report names.
//!
//! Age, per-project count and total size apply independently, and eviction is
//! always oldest first — the newest inspectable run must survive a full disk.

use crate::config::Retention;
use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const LOG_SUFFIXES: [&str; 4] = ["-matrix.log", "-windows.log", "-linux.log", "-macos.log"];

struct Run {
    name: String,
    directory: PathBuf,
    modified: SystemTime,
    logs: Vec<PathBuf>,
    size: u64,
    project: Option<String>,
    unfinished: bool,
}

/// Map each run id to its log files. A run id contains hyphens, so the suffix
/// set is matched explicitly rather than splitting on the separator.
fn log_index(state: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(state.join("logs")) else { return index };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        for suffix in LOG_SUFFIXES {
            if let Some(stem) = name.strip_suffix(suffix) {
                index.entry(stem.to_owned()).or_default().push(path);
                break;
            }
        }
    }
    index
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|value| if value.is_file() { value.len() } else { 0 }).unwrap_or(0)
}

fn tree_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.metadata().map(|value| value.len()).unwrap_or(0))
        .sum()
}

fn collect(state: &Path, current: &str, logs: &HashMap<String, Vec<PathBuf>>) -> Vec<Run> {
    let mut runs = Vec::new();
    let Ok(entries) = std::fs::read_dir(state.join("runs")) else { return runs };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == current || name.starts_with('.') || !entry.path().is_dir() {
            continue;
        }
        let directory = entry.path();
        let report_file = directory.join("report.json");
        // The report's own mtime is used: the directory's moves on every atomic
        // replace and would misreport the run's age.
        let dated = if report_file.is_file() { &report_file } else { &directory };
        let Ok(metadata) = std::fs::metadata(dated) else { continue };
        let Ok(modified) = metadata.modified() else { continue };
        let document: Option<serde_json::Value> =
            std::fs::read(&report_file).ok().and_then(|data| serde_json::from_slice(&data).ok());
        let project = document.as_ref().and_then(|value| {
            value
                .get("project")
                .and_then(|value| value.as_str())
                .or_else(|| value.get("source").and_then(|source| source.get("project")).and_then(|value| value.as_str()))
                .map(str::to_owned)
        });
        let unfinished = document
            .as_ref()
            .and_then(|value| value.get("status").and_then(|value| value.as_str()))
            .is_some_and(|status| status == "running" || status == "incomplete");
        let run_logs = logs.get(&name).cloned().unwrap_or_default();
        let size = tree_size(&directory) + run_logs.iter().map(|path| file_size(path)).sum::<u64>();
        runs.push(Run { name, directory, modified, logs: run_logs, size, project, unfinished });
    }
    runs.sort_by(|left, right| left.modified.cmp(&right.modified).then(left.name.cmp(&right.name)));
    runs
}

/// Remove expired logs no retained run points at. A run pruned by an earlier
/// policy can leave logs behind, and nothing reaches them from a report.
fn prune_orphan_logs(state: &Path, horizon: SystemTime, live: &BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(state.join("logs")) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("log") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if live.iter().any(|stamp| name.starts_with(stamp.as_str())) {
            continue;
        }
        if std::fs::metadata(&path).and_then(|value| value.modified()).is_ok_and(|value| value < horizon) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Apply the policy. Returns how many earlier runs were retained.
pub fn apply(state: &Path, current: &str, current_project: Option<&str>, policy: &Retention) -> Result<usize> {
    let horizon = SystemTime::now()
        .checked_sub(Duration::from_secs(policy.days.max(0) as u64 * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let logs = log_index(state);
    let runs = collect(state, current, &logs);
    let max_runs = policy.max_runs_per_project.max(1) as usize;
    let mut doomed: BTreeSet<String> = BTreeSet::new();

    // Age. An abandoned `running` report is reclaimed here and nowhere else:
    // the controller holds an exclusive lock, so no other run is still alive.
    for run in &runs {
        if run.modified < horizon {
            doomed.insert(run.name.clone());
        }
    }

    // Per project keep the newest runs; the current run occupies one slot.
    let mut kept: HashMap<Option<String>, usize> = HashMap::new();
    kept.insert(current_project.map(str::to_owned), 1);
    for run in runs.iter().rev() {
        if doomed.contains(&run.name) || run.unfinished {
            continue;
        }
        let count = kept.entry(run.project.clone()).or_insert(0);
        if *count >= max_runs {
            doomed.insert(run.name.clone());
        } else {
            *count += 1;
        }
    }

    // Total bytes, evicting the oldest first until the cap is satisfied.
    let mut total: u64 = tree_size(&state.join("runs").join(current))
        + logs.get(current).map(|paths| paths.iter().map(|path| file_size(path)).sum()).unwrap_or(0)
        + runs.iter().filter(|run| !doomed.contains(&run.name)).map(|run| run.size).sum::<u64>();
    for run in &runs {
        if total <= policy.max_bytes {
            break;
        }
        if doomed.contains(&run.name) || run.unfinished {
            continue;
        }
        doomed.insert(run.name.clone());
        total = total.saturating_sub(run.size);
    }

    let mut survivors = 0;
    let mut live: BTreeSet<String> = BTreeSet::new();
    live.insert(current.to_owned());
    for run in &runs {
        if !doomed.contains(&run.name) {
            survivors += 1;
            live.insert(run.name.clone());
            continue;
        }
        if std::fs::remove_dir_all(&run.directory).is_ok() {
            for log in &run.logs {
                let _ = std::fs::remove_file(log);
            }
        } else {
            survivors += 1;
            live.insert(run.name.clone());
        }
    }
    prune_orphan_logs(state, horizon, &live);
    Ok(survivors)
}
