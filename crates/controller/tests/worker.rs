//! What the controller accepts from a worker.
//!
//! The macOS lane once ran a worker built weeks before the controller: it read
//! the request as its own older shape, ran no workflow step and reported a
//! success. A fake worker stands in for that binary here.

use build_machine_controller::{matrix, Operation};
use build_machine_core::config::Machine;
use build_machine_core::report::{Action, ExecutionMode};
use build_machine_core::Platform;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
}

const WORKFLOW: &str = r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: push
jobs:
  test:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - run: make check
"#;

/// A controller root with `machine.json`, a project and a fake macOS worker
/// that runs `script` for every invocation.
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
}

fn fixture(script: &str) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let root = base.join("controller");
    std::fs::create_dir_all(root.join("workers")).unwrap();
    std::fs::write(root.join("machine.json"), "{}").unwrap();
    let worker = root.join("workers/build-machine-worker-macos");
    std::fs::write(&worker, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = base.join("project");
    std::fs::create_dir_all(project.join(".github/workflows")).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["config", "user.email", "test@example.invalid"]);
    git(&project, &["config", "user.name", "test"]);
    std::fs::write(project.join(".github/workflows/ci.yml"), WORKFLOW).unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "ci: Add the workflow"]);
    Fixture { _directory: directory, root, project }
}

fn machine() -> Machine {
    let pinned = serde_json::json!({ "version": "1", "url": "https://example.invalid", "sha256": "0" });
    serde_json::from_value(serde_json::json!({
        "vm": "vm", "share": "share", "windowsRoot": "C:\\BuildMachine", "architecture": "arm64",
        "node": pinned, "pnpm": { "version": "1" },
        "rust": { "version": "1", "host": "h", "installerUrl": "https://example.invalid", "installerSha256": "0" },
        "go": pinned, "git": pinned,
        "msvc": { "installPath": "C:\\BuildTools", "installerUrl": "https://example.invalid", "components": [] },
        "platforms": { "macos": { "runner": "macos-15", "target": "universal-apple-darwin" } },
    }))
    .unwrap()
}

fn replay(fixture: &Fixture) -> build_machine_core::report::RunReport {
    matrix::execute(&Operation {
        root: fixture.root.clone(),
        machine: machine(),
        action: Action::Ci,
        platforms: vec![Platform::Macos],
        execution: ExecutionMode::Sequential,
        project: Some(fixture.project.clone()),
        framework: None,
        command: None,
        artifact: None,
        launch: false,
        workflow: Some(".github/workflows/ci.yml".to_owned()),
        event: "push".to_owned(),
        reference: None,
        result_file: None,
        observer: None,
    })
    .unwrap()
}

/// A successful replay report with no step in it, as the stale worker wrote.
const EMPTY_SUCCESS: &str = r#"echo BUILD_MACHINE_REPORT_BEGIN
echo '{"success":true,"status":"passed_with_limits","finishedAt":"now","attempts":1,"log":""}'
echo BUILD_MACHINE_REPORT_END"#;

/// A worker that does not report the controller's protocol is never given the
/// request: the platform fails, naming both and the rebuild.
#[test]
fn a_worker_of_another_protocol_fails_the_platform() {
    let fixture = fixture(EMPTY_SUCCESS);
    let report = replay(&fixture);
    let result = &report.results[&Platform::Macos];
    let error = result.error.clone().unwrap_or_default();
    assert!(!result.success && !report.succeeded(), "{report:#?}");
    assert!(error.contains("protocol") && error.contains("cargo xtask worker"), "{error}");
}

/// A worker of this protocol whose replay reached no workflow step has not
/// replayed the workflow, so its success is not one.
#[test]
fn a_replay_that_ran_no_step_fails_the_platform() {
    let protocol = build_machine_core::request::PROTOCOL;
    let fixture = fixture(&format!("if [ \"$1\" = protocol ]; then echo {protocol}; exit 0; fi\n{EMPTY_SUCCESS}"));
    let report = replay(&fixture);
    let result = &report.results[&Platform::Macos];
    let error = result.error.clone().unwrap_or_default();
    assert!(!result.success && !report.succeeded(), "{report:#?}");
    assert!(error.contains("실행된 workflow 단계가 없어요"), "{error}");
}

/// A worker built before `protocol` existed rejects the subcommand; only that
/// is reported as an old worker.
#[test]
fn a_worker_without_the_protocol_command_is_reported_as_old() {
    let fixture = fixture("echo \"error: unrecognized subcommand 'protocol'\" >&2; exit 2");
    let report = replay(&fixture);
    let error = report.results[&Platform::Macos].error.clone().unwrap_or_default();
    assert!(error.contains("오래된 워커") && error.contains("cargo xtask worker"), "{error}");
}

/// When the question never reached the worker — Parallels did not start the
/// command — the error names that, not an old worker. Run 20261004-171833-591581
/// reported "older worker" for a worker of the controller's own protocol.
#[test]
fn a_transport_failure_is_not_reported_as_an_old_worker() {
    let fixture = fixture("echo 'PrlJob_GetRetCode: Invalid argument. An invalid argument was passed.' >&2; exit 255");
    let report = replay(&fixture);
    let error = report.results[&Platform::Macos].error.clone().unwrap_or_default();
    assert!(!error.contains("오래된 워커"), "{error}");
    assert!(error.contains("PrlJob_GetRetCode"), "{error}");
}
