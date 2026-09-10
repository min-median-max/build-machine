use build_machine_desktop::controller::{self, JobRequest};
use build_machine_core::report::Action;
use build_machine_core::Platform;
use std::path::Path;
use std::sync::Arc;

/// A machine definition with only what the controller reads.
fn machine_json(node_version: &str) -> String {
    format!(
        r#"{{
  "vm": "Windows 11", "share": "WindowsBuildMachine", "windowsRoot": "C:\\BuildMachine",
  "architecture": "arm64",
  "node": {{"version": "{node_version}", "url": "https://example.invalid/node.zip", "sha256": "00"}},
  "pnpm": {{"version": "11.24.0"}},
  "rust": {{"version": "1.98.1", "host": "aarch64-pc-windows-msvc",
            "installerUrl": "https://example.invalid/rustup", "installerSha256": "00"}},
  "go": {{"version": "1.27.1", "url": "https://example.invalid/go.zip", "sha256": "00"}},
  "git": {{"version": "2.55.0", "url": "https://example.invalid/git.exe", "sha256": "00"}},
  "msvc": {{"installPath": "C:\\BuildTools", "installerUrl": "https://example.invalid/vs.exe", "components": []}},
  "platforms": {{
    "windows": {{"vm": "Windows 11", "runner": "windows-11-arm", "target": "aarch64-pc-windows-msvc", "bundle": "nsis"}},
    "linux": {{"vm": "Ubuntu", "runner": "ubuntu-26.04-arm", "target": "aarch64-unknown-linux-gnu", "bundle": "deb"}},
    "macos": {{"runner": "macos-14", "target": "universal-apple-darwin", "bundle": "dmg"}}
  }}
}}"#
    )
}

fn controller_directory(node_version: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("machine.json"), machine_json(node_version)).unwrap();
    directory
}

fn request(root: &Path, project: Option<&Path>) -> JobRequest {
    JobRequest {
        controller_path: root.to_string_lossy().into_owned(),
        project_path: project.map(|path| path.to_string_lossy().into_owned()),
        platforms: vec!["linux".to_owned()],
        action: "build".to_owned(),
        launch: true,
        workflow: None,
        event: "workflow_dispatch".to_owned(),
        ref_name: None,
        execution: "sequential".to_owned(),
    }
}

fn observer() -> build_machine_controller::oplog::Observer {
    Arc::new(|_, _| {})
}

#[test]
fn a_folder_without_a_machine_definition_is_not_a_controller() {
    let directory = tempfile::tempdir().unwrap();
    let error = controller::controller_root(directory.path().to_str().unwrap()).unwrap_err();
    assert!(error.contains("machine.json"), "{error}");
}

#[test]
fn an_empty_or_unknown_environment_selection_is_rejected_before_work_starts() {
    let directory = controller_directory("22.23.2");
    let project = tempfile::tempdir().unwrap();

    let mut input = request(directory.path(), Some(project.path()));
    input.platforms = vec![];
    assert!(controller::build_operation(&input, observer()).is_err());

    let mut input = request(directory.path(), Some(project.path()));
    input.platforms = vec!["solaris".to_owned()];
    assert!(controller::build_operation(&input, observer()).is_err());
}

#[test]
fn a_build_needs_a_project_folder_but_an_environment_action_does_not() {
    let directory = controller_directory("22.23.2");

    let mut input = request(directory.path(), None);
    assert!(controller::build_operation(&input, observer()).is_err());

    input.action = "doctor".to_owned();
    let operation = controller::build_operation(&input, observer()).unwrap();
    assert_eq!(operation.action, Action::Doctor);
    assert!(operation.project.is_none());
}

/// A workflow path turns the build button into a workflow replay, and a replay
/// has no launch step.
#[test]
fn a_workflow_path_makes_the_build_a_replay_without_a_launch() {
    let directory = controller_directory("22.23.2");
    let project = tempfile::tempdir().unwrap();

    let plain = controller::build_operation(&request(directory.path(), Some(project.path())), observer()).unwrap();
    assert_eq!(plain.action, Action::Build);
    assert!(plain.launch);

    let mut input = request(directory.path(), Some(project.path()));
    input.workflow = Some(".github/workflows/release.yml".to_owned());
    input.ref_name = Some("v0.1.0".to_owned());
    let replay = controller::build_operation(&input, observer()).unwrap();
    assert_eq!(replay.action, Action::Ci);
    assert!(!replay.launch, "a replay must not carry the launch option");
    assert_eq!(replay.reference.as_deref(), Some("v0.1.0"));
}

/// A project path is carried as one value, never through a shell, so a folder
/// name that looks like a command cannot become one.
#[test]
fn a_project_path_is_never_interpreted_as_a_command() {
    let directory = controller_directory("22.23.2");
    let parent = tempfile::tempdir().unwrap();
    let project = parent.path().join("project with ' spaces; touch unexpected");
    std::fs::create_dir(&project).unwrap();
    let operation =
        controller::build_operation(&request(directory.path(), Some(&project)), observer()).unwrap();
    assert_eq!(operation.project.as_deref(), Some(project.as_path()));
    assert!(!parent.path().join("unexpected").exists());
}

#[test]
fn environment_selections_are_ordered_and_deduplicated() {
    let directory = controller_directory("22.23.2");
    let project = tempfile::tempdir().unwrap();
    let mut input = request(directory.path(), Some(project.path()));
    input.platforms = vec!["macos".to_owned(), "windows".to_owned(), "macos".to_owned()];
    let operation = controller::build_operation(&input, observer()).unwrap();
    assert_eq!(operation.platforms, vec![Platform::Windows, Platform::Macos]);
}

/// Tool results describe the machine definition they were measured against.
#[test]
fn tool_results_reload_from_disk_and_do_not_apply_to_a_changed_definition() {
    let directory = controller_directory("22.23.2");
    let root = directory.path();
    let machine =
        build_machine_core::config::Machine::load(&root.join("machine.json")).unwrap();
    let configuration = serde_json::to_value(&machine).unwrap();
    let results = serde_json::json!({"linux": {"success": true, "status": "passed"}});
    std::fs::create_dir_all(root.join(".state")).unwrap();
    std::fs::write(
        root.join(".state/tool-status.json"),
        serde_json::json!({"configuration": configuration, "results": results}).to_string(),
    )
    .unwrap();
    assert_eq!(controller::tool_status(root, &machine), results);

    let changed = controller_directory("24.0.0");
    let other = build_machine_core::config::Machine::load(&changed.path().join("machine.json")).unwrap();
    assert_eq!(controller::tool_status(root, &other), serde_json::json!({}));
}
