//! A replay's snapshot carries the history of every other repository its
//! checkout steps name, from the local clone `machine.json` maps it to.

use build_machine_controller::{snapshot, Operation};
use build_machine_core::config::Machine;
use build_machine_core::report::{Action, ExecutionMode};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn repository(path: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.email", "test@example.invalid"]);
    git(path, &["config", "user.name", "test"]);
    for (name, text) in files {
        let file = path.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "feat(core): Add one"]);
}

/// The machine definition with only what a replay reads, and `repositories`.
fn machine(repositories: serde_json::Value) -> Machine {
    let pinned = serde_json::json!({ "version": "1", "url": "https://example.invalid", "sha256": "0" });
    serde_json::from_value(serde_json::json!({
        "vm": "vm", "share": "share", "windowsRoot": "C:\\BuildMachine", "architecture": "arm64",
        "node": pinned, "pnpm": { "version": "1" },
        "rust": { "version": "1", "host": "h", "installerUrl": "https://example.invalid", "installerSha256": "0" },
        "go": pinned, "git": pinned,
        "msvc": { "installPath": "C:\\BuildTools", "installerUrl": "https://example.invalid", "components": [] },
        "platforms": {},
        "repositories": repositories,
    }))
    .unwrap()
}

fn operation(root: &Path, machine: Machine, project: &Path) -> Operation {
    Operation {
        root: root.to_path_buf(),
        machine,
        action: Action::Ci,
        platforms: Vec::new(),
        execution: ExecutionMode::Sequential,
        project: Some(project.to_path_buf()),
        framework: None,
        command: None,
        artifact: None,
        launch: false,
        workflow: Some(".github/workflows/release.yml".to_owned()),
        event: "push".to_owned(),
        reference: None,
        result_file: None,
        observer: None,
    }
}

const WORKFLOW: &str = r#"# build-machine: skip smoke reason=fixture
name: release
on:
  push:
    tags: ['v*']
jobs:
  release:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - uses: actions/checkout@v4
        with:
          repository: example/core
          ref: v0.0.2
          path: core
      - name: Test
        run: make test
      - name: Build
        run: make build
"#;

struct Fixture {
    _directory: tempfile::TempDir,
    base: PathBuf,
    project: PathBuf,
    core: PathBuf,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let core = base.join("core");
    repository(&core, &[("value.txt", "core")]);
    git(&core, &["tag", "v0.0.2"]);
    let project = base.join("files");
    repository(&project, &[(".github/workflows/release.yml", WORKFLOW)]);
    Fixture { _directory: directory, base, project, core }
}

#[test]
fn a_replay_bundles_every_repository_its_checkout_steps_name() {
    let fixture = fixture();
    let machine = machine(serde_json::json!({ "example/core": fixture.core.to_string_lossy() }));
    let state = fixture.base.join("state");
    let replay = snapshot::for_replay(&operation(&fixture.base, machine, &fixture.project), &state).unwrap();
    let snapshot = serde_json::to_value(&replay.snapshot).unwrap();
    let recorded = &snapshot["repositories"]["example/core"];
    let bundle = PathBuf::from(recorded["bundle"].as_str().unwrap_or_else(|| panic!("{snapshot:#}")));
    assert!(bundle.starts_with(&state), "{}", bundle.display());
    assert_eq!(recorded["sha256"], build_machine_core::source::sha256_file(&bundle).unwrap());
    let heads = git(&fixture.base, &["bundle", "list-heads", &bundle.to_string_lossy()]);
    assert!(heads.contains("refs/tags/v0.0.2") && heads.contains("refs/heads/main"), "{heads}");
}

/// A repository `machine.json` does not map has no local history to check
/// out, so validation names it and the map.
#[test]
fn a_repository_the_machine_does_not_map_fails_validation() {
    let fixture = fixture();
    let machine = machine(serde_json::json!({}));
    let error = snapshot::for_replay(&operation(&fixture.base, machine, &fixture.project), &fixture.base.join("state"))
        .err()
        .expect("an unmapped repository is refused");
    let message = format!("{error:#}");
    assert!(message.contains("example/core") && message.contains("machine.json repositories"), "{message}");
}

/// The workflow's `github.ref_name` is the tag a replay checks out, so a
/// replay of a commit no branch or tag names cannot give it a value.
#[test]
fn a_replay_of_a_commit_without_a_ref_name_is_refused_when_the_workflow_reads_one() {
    let fixture = fixture();
    let workflow = WORKFLOW.replace("ref: v0.0.2", "ref: ${{ github.ref_name }}");
    std::fs::write(fixture.project.join(".github/workflows/release.yml"), workflow).unwrap();
    git(&fixture.project, &["commit", "-qam", "ci: Read the ref name"]);
    git(&fixture.project, &["tag", "v0.0.2"]);
    let machine = || machine(serde_json::json!({ "example/core": fixture.core.to_string_lossy() }));
    let state = fixture.base.join("state");

    let mut tag = operation(&fixture.base, machine(), &fixture.project);
    tag.reference = Some("v0.0.2".to_owned());
    let replay = snapshot::for_replay(&tag, &state).unwrap();
    assert_eq!(replay.snapshot.checkout_ref.as_deref(), Some("refs/tags/v0.0.2"));

    // The working tree is on main.
    snapshot::for_replay(&operation(&fixture.base, machine(), &fixture.project), &state).unwrap();

    let mut commit = operation(&fixture.base, machine(), &fixture.project);
    commit.reference = Some(git(&fixture.project, &["rev-parse", "HEAD"]));
    let error = snapshot::for_replay(&commit, &state).err().expect("a commit has no ref name");
    let message = format!("{error:#}");
    assert!(message.contains("github.ref") && message.contains("branch나 tag"), "{message}");
}
