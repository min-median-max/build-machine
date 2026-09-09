use build_machine_desktop::dashboard;
use serde_json::{json, Value};
use std::{fs, path::Path};

fn report(root: &Path, name: &str, value: Value) {
    fs::create_dir_all(root.join(".state")).unwrap();
    fs::write(root.join(".state").join(format!("{name}-result.json")), value.to_string()).unwrap();
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
    fs::create_dir_all(root.path().join(".state")).unwrap();
    fs::write(root.path().join(".state/broken-result.json"), "{").unwrap();
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert!(value["warning"].as_str().unwrap().contains("최신"));
    assert!(value["projects"][0]["latest"].is_null());
    report(root.path(), "invalid-fields", json!({"action":"build","project":"/app","results":{"linux":{}}}));
    let value = dashboard::read(root.path(), &["/app".into()]).unwrap();
    assert_eq!(value["projects"][0]["latest"]["status"], "incomplete");
    assert!(value["warning"].as_str().unwrap().contains("2개"));
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
