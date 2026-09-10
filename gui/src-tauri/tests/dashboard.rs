use build_machine_core::report::{
    Action, ExecutionMode, Outcome, PlatformResult, RunReport, RunStatus,
};
use build_machine_core::source::{Snapshot, SourceMode};
use build_machine_core::Platform;
use build_machine_desktop::dashboard;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn snapshot(project: &str) -> Snapshot {
    Snapshot {
        revision: "abc123def456789".to_owned(),
        dirty: false,
        source_hash: "hash".to_owned(),
        file_count: 3,
        source_mode: SourceMode::Local,
        project_key: "app-0123456789".to_owned(),
        project: project.to_owned(),
        archive: "/archive.zip".to_owned(),
        workflow_path: None,
        event: None,
        requested_ref: None,
        stage_counts: BTreeMap::new(),
        framework: None,
        command: None,
        artifact: None,
    }
}

struct Builder {
    report: RunReport,
}

fn report(id: &str, project: &str) -> Builder {
    Builder {
        report: RunReport {
            run_id: id.to_owned(),
            action: Action::Build,
            project: Some(project.to_owned()),
            platforms: vec![Platform::Linux],
            execution_mode: ExecutionMode::Sequential,
            status: RunStatus::Success,
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            finished_at: Some("2026-01-01T00:01:00Z".to_owned()),
            source: Some(snapshot(project)),
            results: BTreeMap::new(),
            log: "/logs/run.log".to_owned(),
            error: None,
        },
    }
}

impl Builder {
    fn platforms(mut self, platforms: &[Platform]) -> Self {
        self.report.platforms = platforms.to_vec();
        self
    }

    fn result(mut self, platform: Platform, success: bool) -> Self {
        let mut result = PlatformResult::passed("2026-01-01T00:01:00Z".to_owned(), "/logs/p.log".to_owned());
        result.success = success;
        result.status = if success { Outcome::Passed } else { Outcome::Failed };
        self.report.results.insert(platform, result);
        self
    }

    fn status(mut self, status: RunStatus) -> Self {
        self.report.status = status;
        self
    }

    fn action(mut self, action: Action) -> Self {
        self.report.action = action;
        self
    }

    fn error(mut self, error: &str) -> Self {
        self.report.error = Some(error.to_owned());
        self
    }

    fn source(mut self, source: Snapshot) -> Self {
        self.report.source = Some(source);
        self
    }

    fn write(self, root: &Path) {
        let directory = root.join(".state/runs").join(&self.report.run_id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("report.json"), serde_json::to_string(&self.report).unwrap()).unwrap();
    }
}

#[test]
fn only_registered_projects_and_real_build_reports_enter_the_dashboard() {
    let root = tempfile::tempdir().unwrap();
    report("01", "/first/app").result(Platform::Linux, true).write(root.path());
    report("02", "/first/app").action(Action::Doctor).result(Platform::Linux, true).write(root.path());
    report("03", "/unregistered/app").result(Platform::Linux, false).write(root.path());
    let value = dashboard::read(root.path(), &["/first/app".into(), "/second/app".into()]).unwrap();
    assert_eq!(value["history"].as_array().unwrap().len(), 1);
    assert_eq!(value["projects"][0]["latest"]["status"], "success");
    assert_eq!(value["projects"][0]["latest"]["platforms"], json!(["linux"]));
    assert!(value["projects"][1]["latest"].is_null());
}

#[test]
fn later_failure_and_unfinished_reports_replace_old_success_without_inventing_success() {
    let root = tempfile::tempdir().unwrap();
    report("01", "/app").result(Platform::Linux, true).write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "success");

    report("02", "/app")
        .platforms(&[Platform::Linux, Platform::Macos])
        .status(RunStatus::Failure)
        .error("source preparation failed")
        .write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "failure");
    assert_eq!(value["projects"][0]["latest"]["error"], "source preparation failed");

    report("03", "/app")
        .platforms(&[Platform::Linux, Platform::Macos])
        .status(RunStatus::Running)
        .result(Platform::Linux, true)
        .write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "incomplete");
}

#[test]
fn a_platform_without_a_result_is_incomplete_not_successful() {
    let root = tempfile::tempdir().unwrap();
    report("01", "/app")
        .platforms(&[Platform::Linux, Platform::Macos])
        .result(Platform::Linux, true)
        .write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "incomplete");
}

#[test]
fn missing_and_corrupt_history_are_not_reported_as_success() {
    let root = tempfile::tempdir().unwrap();
    assert!(dashboard::read(root.path(), &["/app".into()]).unwrap()["projects"][0]["latest"].is_null());
    fs::create_dir_all(root.path().join(".state/runs/broken")).unwrap();
    fs::write(root.path().join(".state/runs/broken/report.json"), "{").unwrap();
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert!(value["warning"].as_str().unwrap().contains("최신"));
    assert!(value["projects"][0]["latest"].is_null());
}

/// A run directory without a report is the current run before its first write,
/// or one being pruned. Neither is a broken record.
#[test]
fn a_run_directory_without_a_report_is_not_a_broken_record() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".state/runs/20260101-000000-000000")).unwrap();
    report("20260101-120000-000000", "/app").result(Platform::Linux, true).write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert!(value["warning"].is_null());
    assert_eq!(value["history"].as_array().unwrap().len(), 1);
    assert_eq!(value["projects"][0]["latest"]["status"], "success");
}

/// The run id is the start stamp. A long run's report is rewritten last, so
/// ordering by file time would float it above later runs.
#[test]
fn history_is_ordered_by_run_start_not_by_last_write() {
    let root = tempfile::tempdir().unwrap();
    report("20260101-100000-000000", "/app").result(Platform::Linux, true).write(root.path());
    report("20260101-102000-000000", "/app").result(Platform::Linux, true).write(root.path());
    report("20260101-100000-000000", "/app").result(Platform::Linux, true).write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["history"][0]["id"], "20260101-102000-000000");
    assert_eq!(value["projects"][0]["latest"]["id"], "20260101-102000-000000");
}

/// A replay belongs in the build history, labelled, and without the parsed
/// workflow it was produced from.
#[test]
fn workflow_replays_are_listed_without_carrying_the_whole_recipe() {
    let root = tempfile::tempdir().unwrap();
    let mut source = snapshot("/app");
    source.workflow_path = Some(".github/workflows/release.yml".to_owned());
    source.event = Some("workflow_dispatch".to_owned());
    source.stage_counts = BTreeMap::from([("build".to_owned(), 1), ("test".to_owned(), 2)]);
    report("01", "/app")
        .action(Action::Ci)
        .status(RunStatus::PassedWithLimits)
        .result(Platform::Linux, true)
        .source(source)
        .write(root.path());
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    let latest = &value["projects"][0]["latest"];
    assert_eq!(latest["action"], "ci");
    assert_eq!(latest["status"], "passed_with_limits");
    assert_eq!(latest["source"]["revision"], "abc123def456789");
    assert_eq!(latest["source"]["workflowPath"], ".github/workflows/release.yml");
    assert_eq!(latest["source"]["stageCounts"]["test"], 2);
    assert!(latest["source"]["archive"].is_null(), "the archive path is not the dashboard's business");
    assert!(value["warning"].is_null());
}

#[test]
fn log_access_stays_inside_the_controller_log_directory() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".state/logs")).unwrap();
    let inside = root.path().join(".state/logs/build.log");
    let outside = root.path().join("outside.log");
    fs::write(&inside, "build output").unwrap();
    fs::write(&outside, "other output").unwrap();
    assert_eq!(dashboard::log_path(root.path(), inside.to_str().unwrap()).unwrap(), inside.canonicalize().unwrap());
    assert!(dashboard::log_path(root.path(), outside.to_str().unwrap()).is_err());
    assert!(dashboard::log_path(root.path(), "missing.log").is_err());
}
