use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, time::UNIX_EPOCH};

/// A replay snapshot carries the whole parsed workflow and every step's shell
/// command. The dashboard needs the run's identity, not its recipe, so only the
/// fields it displays cross the bridge.
fn summarize_source(source: &Value) -> Value {
    if !source.is_object() { return source.clone(); }
    let mut summary = serde_json::Map::new();
    for key in ["revision", "dirty", "sourceMode", "fileCount", "workflowPath", "event", "requestedRef"] {
        if let Some(value) = source.get(key) { summary.insert(key.to_string(), value.clone()); }
    }
    if let Some(stages) = source.get("stages").and_then(Value::as_object) {
        summary.insert("stageCounts".into(), Value::Object(stages.iter()
            .map(|(name, steps)| (name.clone(), json!(steps.as_array().map(Vec::len).unwrap_or(0))))
            .collect()));
    }
    Value::Object(summary)
}

pub fn read(root: &Path, projects: &[String]) -> Result<Value, String> {
    let mut history = Vec::new();
    let mut unreadable = 0;
    let runs = root.join(".state/runs");
    if runs.is_dir() {
        for entry in fs::read_dir(&runs).map_err(|e| format!("빌드 기록을 읽지 못했어요: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') || !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) { continue; }
            let path = entry.path().join("report.json");
            let data = match fs::read(&path) {
                Ok(data) => data,
                // A run directory without a report is the current run before its
                // first write, or one being pruned. Neither is a broken record.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => { unreadable += 1; continue; }
            };
            let Some(report) = serde_json::from_slice::<Value>(&data).ok() else {
                unreadable += 1; continue;
            };
            // `ci` is a workflow replay of the same project and belongs here;
            // diagnosis, setup and launch records are not builds.
            let action = report["action"].as_str().unwrap_or_default();
            if !["build", "ci"].contains(&action) { continue; }
            let Some(project) = report["project"].as_str().or_else(|| report["source"]["project"].as_str()) else {
                unreadable += 1; continue;
            };
            if !projects.iter().any(|path| path == project) { continue; }
            let results = report["results"].as_object();
            let platforms: Vec<String> = match report["platforms"].as_array() {
                Some(values) => values.iter().filter_map(Value::as_str).map(String::from).collect(),
                None => results.map(|values| values.keys().cloned().collect()).unwrap_or_default(),
            };
            if results.is_none() || results.is_some_and(|values| values.values().any(|result| result["success"].as_bool().is_none()))
                || platforms.iter().any(|os| !["windows", "linux", "macos"].contains(&os.as_str())) { unreadable += 1; }
            let failed = report["error"].is_string() || report["status"] == "failure"
                || results.is_some_and(|values| values.values().any(|result| result["success"] == false));
            let complete = !platforms.is_empty() && platforms.iter().all(|os| {
                ["windows", "linux", "macos"].contains(&os.as_str()) && report["results"][os]["success"] == true
            });
            let status = if report["status"] == "running" { "incomplete" }
                else if failed { "failure" } else if report["status"] == "passed_with_limits" { "passed_with_limits" }
                else if complete { "success" } else { "incomplete" };
            let recorded_at = fs::metadata(&path).and_then(|m| m.modified()).ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|time| time.as_millis() as u64);
            if recorded_at.is_none() { unreadable += 1; }
            history.push(json!({"id":id,"project":project,"status":status,"action":action,
                "platforms":platforms,"results":report["results"],"recordedAt":recorded_at,
                "finishedAt":report["finishedAt"],"log":report["log"],"error":report["error"],
                "executionMode":report["executionMode"],"source":summarize_source(&report["source"])}));
        }
    }
    // The run id is the start stamp, so it orders chronologically. The report's
    // mtime is its last update, which would float a long run above later ones.
    history.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str())
        .then_with(|| b["recordedAt"].as_u64().cmp(&a["recordedAt"].as_u64())));
    let rows: Vec<Value> = projects.iter().map(|path| json!({"path":path,
        "latest":history.iter().find(|record| record["project"].as_str() == Some(path))})).collect();
    history.truncate(6);
    Ok(json!({"projects":rows,"history":history,"warning":if unreadable == 0 { None }
        else { Some(format!("빌드 기록 {unreadable}개를 완전히 읽지 못했어요. 표시된 결과가 최신이 아닐 수 있어요.")) }}))
}

pub fn log_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let logs = root.join(".state/logs").canonicalize().map_err(|_| "로그 폴더가 없어요.".to_string())?;
    let path = Path::new(value).canonicalize().map_err(|_| "해당 빌드 로그를 찾을 수 없어요.".to_string())?;
    if !path.starts_with(logs) || !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("log") {
        return Err("빌드 도구의 로그 파일만 열 수 있어요.".into());
    }
    Ok(path)
}
