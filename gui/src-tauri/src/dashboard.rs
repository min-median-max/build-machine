//! Reading recorded runs for the dashboard.
//!
//! Every run is recorded once, at `.state/runs/<run>/report.json`. The report
//! is deserialized into the shared type rather than indexed as a document, so
//! a field the controller renames cannot silently stop being displayed.

use build_machine_core::report::{Action, Outcome, RunReport, RunStatus};
use build_machine_core::source::Snapshot;
use build_machine_core::Platform;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// What the dashboard shows for a run, which is not the same as either the
/// overall status or a single platform's.
fn displayed_status(report: &RunReport) -> &'static str {
    if report.status == RunStatus::Running {
        return "incomplete";
    }
    let failed = report.error.is_some()
        || report.status == RunStatus::Failure
        || report.results.values().any(|result| !result.success);
    if failed {
        return "failure";
    }
    if report.status == RunStatus::PassedWithLimits {
        return "passed_with_limits";
    }
    let complete = !report.platforms.is_empty()
        && report.platforms.iter().all(|platform| {
            report.results.get(platform).map(|result| result.success).unwrap_or(false)
        });
    if complete {
        "success"
    } else {
        "incomplete"
    }
}

/// A replay snapshot carries the whole parsed workflow and every step's shell
/// command. The dashboard needs the run's identity, not its recipe.
fn summarize_source(source: &Snapshot) -> Value {
    json!({
        "revision": source.revision,
        "dirty": source.dirty,
        "sourceMode": source.source_mode,
        "fileCount": source.file_count,
        "workflowPath": source.workflow_path,
        "event": source.event,
        "requestedRef": source.requested_ref,
        "stageCounts": source.stage_counts,
    })
}

pub fn read(root: &Path, projects: &[String]) -> Result<Value, String> {
    let mut history = Vec::new();
    let mut unreadable = 0;
    let runs = root.join(".state/runs");
    if runs.is_dir() {
        for entry in fs::read_dir(&runs).map_err(|e| format!("빌드 기록을 읽지 못했어요: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') || !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path().join("report.json");
            let data = match fs::read(&path) {
                Ok(data) => data,
                // A run directory without a report is the current run before its
                // first write, or one being pruned. Neither is a broken record.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            let Ok(report) = serde_json::from_slice::<RunReport>(&data) else {
                unreadable += 1;
                continue;
            };
            // A replay is a build of the same project and belongs here;
            // diagnosis, setup and launch records are not builds.
            if !report.action.is_build_history() {
                continue;
            }
            let Some(project) =
                report.project.clone().or_else(|| report.source.as_ref().map(|source| source.project.clone()))
            else {
                unreadable += 1;
                continue;
            };
            if !projects.contains(&project) {
                continue;
            }
            let platforms: Vec<Platform> = if report.platforms.is_empty() {
                report.results.keys().copied().collect()
            } else {
                report.platforms.clone()
            };
            let recorded_at = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|time| time.as_millis() as u64);
            if recorded_at.is_none() {
                unreadable += 1;
            }
            history.push(json!({
                "id": id,
                "project": project,
                "status": displayed_status(&report),
                "action": if report.action == Action::Ci { "ci" } else { "build" },
                "platforms": platforms,
                "results": report.results,
                "recordedAt": recorded_at,
                "finishedAt": report.finished_at,
                "log": report.log,
                "error": report.error,
                "executionMode": report.execution_mode,
                "source": report.source.as_ref().map(summarize_source),
            }));
        }
    }
    // The run id is the start stamp, so it orders chronologically. The report's
    // mtime is its last update, which would float a long run above later ones.
    history.sort_by(|a, b| {
        b["id"].as_str().cmp(&a["id"].as_str()).then_with(|| b["recordedAt"].as_u64().cmp(&a["recordedAt"].as_u64()))
    });
    let rows: Vec<Value> = projects
        .iter()
        .map(|path| {
            json!({
                "path": path,
                "latest": history.iter().find(|record| record["project"].as_str() == Some(path))
            })
        })
        .collect();
    history.truncate(6);
    Ok(json!({
        "projects": rows,
        "history": history,
        "warning": if unreadable == 0 { None } else {
            Some(format!("빌드 기록 {unreadable}개를 완전히 읽지 못했어요. 표시된 결과가 최신이 아닐 수 있어요."))
        }
    }))
}

/// Only this controller's own log files can be opened.
pub fn log_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let logs = root.join(".state/logs").canonicalize().map_err(|_| "로그 폴더가 없어요.".to_string())?;
    let path = Path::new(value).canonicalize().map_err(|_| "해당 빌드 로그를 찾을 수 없어요.".to_string())?;
    if !path.starts_with(logs) || !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("log") {
        return Err("빌드 도구의 로그 파일만 열 수 있어요.".into());
    }
    Ok(path)
}

/// A platform result that cannot be described is not a success.
pub fn platform_succeeded(status: Outcome) -> bool {
    status.succeeded()
}
