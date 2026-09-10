use build_machine_desktop::dashboard;
use serde_json::{json, Value};
use std::{fs, path::Path};

fn report(root: &Path, name: &str, value: Value) {
    let directory = root.join(".state/runs").join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("report.json"), value.to_string()).unwrap();
}

#[test]
fn only_registered_projects_and_real_build_reports_enter_the_dashboard() {
    let root = tempfile::tempdir().unwrap();
    report(root.path(), "01", json!({"action":"build","source":{"project":"/first/app"},"results":{"linux":{"success":true}}}));
    report(root.path(), "02", json!({"action":"doctor","project":"/first/app","results":{"windows":{"success":false}}}));
    report(root.path(), "03", json!({"action":"build","project":"/unregistered/app","results":{"linux":{"success":false}}}));
    fs::create_dir_all(root.path().join(".state/projects/request")).unwrap();
    fs::write(root.path().join(".state/projects/request/latest.json"), json!({"project":"/second/app","sourceHash":"request-only"}).to_string()).unwrap();
    let value = dashboard::read(root.path(), &["/first/app".into(), "/second/app".into()]).unwrap();
    assert_eq!(value["history"].as_array().unwrap().len(), 1);
    assert_eq!(value["projects"][0]["latest"]["status"], "success");
    assert_eq!(value["projects"][0]["latest"]["platforms"], json!(["linux"]));
    assert!(value["projects"][1]["latest"].is_null());
}

#[test]
fn later_failure_and_unfinished_reports_replace_old_success_without_inventing_success() {
    let root = tempfile::tempdir().unwrap();
    report(root.path(), "01", json!({"action":"build","project":"/app","results":{"linux":{"success":true}}}));
    report(root.path(), "02", json!({"action":"build","project":"/app","platforms":["linux","macos"],"status":"failure","error":"source preparation failed","results":{}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "failure");
    assert_eq!(value["projects"][0]["latest"]["error"], "source preparation failed");
    report(root.path(), "03", json!({"action":"build","project":"/app","platforms":["linux","macos"],"status":"running","results":{"linux":{"success":true}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "incomplete");
    report(root.path(), "04", json!({"action":"build","project":"/app","platforms":["linux","macos"],"status":"success","results":{"linux":{"success":true}}}));
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
    report(root.path(), "invalid-fields", json!({"action":"build","project":"/app","results":{"linux":{}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "incomplete");
    assert!(value["warning"].as_str().unwrap().contains("2개"));
}

#[test]
fn workflow_replays_are_listed_without_carrying_the_whole_recipe() {
    let root = tempfile::tempdir().unwrap();
    report(root.path(), "01", json!({"action":"ci","project":"/app","platforms":["linux"],
        "status":"passed_with_limits","results":{"linux":{"success":true,"status":"passed_with_limits"}},
        "source":{"revision":"abc123","dirty":false,"event":"workflow_dispatch",
                  "workflowPath":".github/workflows/release.yml",
                  "workflow":{"jobs":[{"steps":[{"run":"echo secret-looking command"}]}]},
                  "stages":{"build":[{"index":1}],"test":[{"index":2},{"index":3}]}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    let latest = &value["projects"][0]["latest"];
    assert_eq!(latest["status"], "passed_with_limits");
    assert_eq!(latest["action"], "ci");
    assert_eq!(latest["source"]["revision"], "abc123");
    assert_eq!(latest["source"]["workflowPath"], ".github/workflows/release.yml");
    assert_eq!(latest["source"]["stageCounts"]["test"], 2);
    assert!(latest["source"]["workflow"].is_null());
    assert!(latest["source"]["stages"].is_null());
    assert!(value["warning"].is_null());
}

#[test]
fn a_run_directory_without_a_report_is_not_a_broken_record() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".state/runs/20260101-000000-000000")).unwrap();
    report(root.path(), "20260101-120000-000000", json!({"action":"build","project":"/app",
        "platforms":["linux"],"status":"success","results":{"linux":{"success":true}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert!(value["warning"].is_null());
    assert_eq!(value["history"].as_array().unwrap().len(), 1);
    assert_eq!(value["projects"][0]["latest"]["status"], "success");
}

#[test]
fn history_is_ordered_by_run_start_not_by_last_write() {
    let root = tempfile::tempdir().unwrap();
    // The earlier run finishes later, so its report is rewritten most recently.
    report(root.path(), "20260101-100000-000000", json!({"action":"build","project":"/app",
        "platforms":["linux"],"status":"success","results":{"linux":{"success":true}}}));
    report(root.path(), "20260101-102000-000000", json!({"action":"build","project":"/app",
        "platforms":["linux"],"status":"success","results":{"linux":{"success":true}}}));
    report(root.path(), "20260101-100000-000000", json!({"action":"build","project":"/app",
        "platforms":["linux"],"status":"success","results":{"linux":{"success":true}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["history"][0]["id"], "20260101-102000-000000");
    assert_eq!(value["projects"][0]["latest"]["id"], "20260101-102000-000000");
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
