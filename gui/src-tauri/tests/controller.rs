use build_machine_desktop::controller::{self, Action, JobRequest, OutputLine};
use std::{fs, sync::{Arc, Mutex}};

fn request(root: &std::path::Path) -> JobRequest {
    JobRequest { controller_path: root.to_string_lossy().into(), project_path: None,
        platforms: vec!["linux".into()], action: Action::Doctor, launch: false }
}

#[test]
fn child_failure_and_both_log_streams_reach_the_caller() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("machine.json"), "{}").unwrap();
    fs::write(directory.path().join("build.py"), r#"
import json, pathlib, sys
result = pathlib.Path(sys.argv[sys.argv.index('--result-file') + 1])
result.parent.mkdir(parents=True, exist_ok=True)
result.write_text(json.dumps({'results': {'linux': {'success': False, 'error': 'missing test tool'}}, 'argv': sys.argv}))
print('PLATFORM: linux', flush=True)
print('missing test tool', file=sys.stderr, flush=True)
sys.exit(7)
"#).unwrap();
    let lines = Arc::new(Mutex::new(Vec::<OutputLine>::new()));
    let captured = lines.clone();
    let result = controller::execute(request(directory.path()), Arc::new(move |line| captured.lock().unwrap().push(line))).unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(result.result.unwrap()["results"]["linux"]["success"], false);
    let output = lines.lock().unwrap();
    assert!(output.iter().any(|line| line.stream == "stdout" && line.line == "PLATFORM: linux"));
    assert!(output.iter().any(|line| line.stream == "stderr" && line.line == "missing test tool"));
}

#[test]
fn project_paths_are_passed_as_one_argument_without_shell_execution() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("machine.json"), "{}").unwrap();
    fs::write(directory.path().join("build.py"), r#"
import json, pathlib, sys
result = pathlib.Path(sys.argv[sys.argv.index('--result-file') + 1])
result.parent.mkdir(parents=True, exist_ok=True)
result.write_text(json.dumps({'argv': sys.argv, 'results': {}}))
"#).unwrap();
    let project = directory.path().join("project with ' spaces; touch unexpected");
    fs::create_dir(&project).unwrap();
    let mut input = request(directory.path());
    input.action = Action::Build;
    input.project_path = Some(project.to_string_lossy().into());
    input.platforms = vec!["linux".into(), "macos".into()];
    input.launch = true;
    let result = controller::execute(input, Arc::new(|_| {})).unwrap();
    let args = result.result.unwrap()["argv"].as_array().unwrap().clone();
    assert_eq!(args[2], project.to_string_lossy().as_ref());
    assert!(args.iter().any(|argument| argument == "--run"));
    assert!(!directory.path().join("unexpected").exists());
}

#[test]
fn tool_results_reload_from_disk_and_do_not_apply_to_changed_configuration() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".state")).unwrap();
    let config = serde_json::json!({"node":{"version":"22.test"}});
    let results = serde_json::json!({"linux":{"success":true,"action":"setup","finishedAt":"2026-09-09T11:00:00Z"}});
    fs::write(directory.path().join(".state/tool-status.json"), serde_json::to_vec(&serde_json::json!({"configuration":config,"results":results})).unwrap()).unwrap();
    assert_eq!(controller::tool_status(directory.path(), &config), results);
    assert_eq!(controller::tool_status(directory.path(), &serde_json::json!({"node":{"version":"24.test"}})), serde_json::json!({}));
}

#[test]
fn zero_exit_without_a_result_file_is_not_success() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("machine.json"), "{}").unwrap();
    fs::write(directory.path().join("build.py"), "print('worker ended without a report')").unwrap();
    let result = controller::execute(request(directory.path()), Arc::new(|_| {}));
    assert!(result.unwrap_err().contains("결과 파일"));
}

#[test]
fn invalid_platform_is_rejected_before_a_child_starts() {
    let mut input = request(std::path::Path::new("/unused"));
    input.platforms = vec!["linux; touch unexpected".into()];
    assert!(controller::arguments(&input, std::path::Path::new("/unused"), std::path::Path::new("/unused/result.json")).is_err());
}

#[test]
#[ignore = "Requires the configured running Ubuntu Parallels VM; performs read-only tool diagnosis"]
fn real_ubuntu_doctor() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let result = controller::execute(request(root), Arc::new(|line| println!("{}", line.line))).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.result.unwrap()["results"]["linux"]["success"], true);
    println!("GUI bridge result: {}", result.result_path);
}
