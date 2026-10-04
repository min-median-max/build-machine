//! How a replay orders steps and bounds their time.

use build_machine_core::workflow;
use build_machine_worker::ci::{ordered_steps, step_limit, Limit};
use std::time::Duration;

/// A step without `timeout-minutes` is bounded only by what is left of its
/// job's limit, as on GitHub; a step with one by the smaller of the two.
#[test]
fn a_step_runs_for_its_own_limit_within_its_job_s() {
    let hour = Duration::from_secs(3600);
    assert_eq!(step_limit(None, hour), (hour, Limit::Job));
    assert_eq!(step_limit(Some(5), hour), (Duration::from_secs(300), Limit::Step));
    assert_eq!(step_limit(Some(90), hour), (hour, Limit::Job));
}

/// orm's "database servers" step runs `make test-servers`, classified as
/// test, and the next step reads the file it writes and is classified as
/// setup. The request a worker receives runs them in workflow order.
#[test]
fn the_worker_runs_steps_in_workflow_order_whatever_their_stage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ci.yml");
    std::fs::write(
        &path,
        r#"# build-machine: skip build reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: ubuntu-26.04-arm
    steps:
      - uses: actions/checkout@v5
      - name: database servers
        run: make test-servers
      - name: environment of the database servers
        run: sed -e 's/^export //' .runtime/servers/env >> "$GITHUB_ENV"
      - name: make check
        run: make check
      - name: decoder fuzz smoke checks
        if: ${{ !cancelled() }}
        run: make fuzz-check
      - name: Go
        run: go test ./...
"#,
    )
    .unwrap();
    let parsed = workflow::load(&path, "workflow_dispatch", None).unwrap();
    let request = build_machine_core::request::WorkRequest {
        protocol: build_machine_core::request::PROTOCOL.to_owned(),
        snapshot: serde_json::from_value(serde_json::json!({
            "revision": "r", "dirty": false, "sourceHash": "h", "fileCount": 0, "sourceMode": "local",
            "projectKey": "orm", "project": "/orm", "archive": ""
        }))
        .unwrap(),
        archive: String::new(),
        history: None,
        repositories: Default::default(),
        target: String::new(),
        bundle: None,
        framework: None,
        command: None,
        artifact: None,
        jobs: workflow::jobs_for(&parsed, Some(build_machine_core::Platform::Linux)),
        skips: parsed.skips.clone(),
        workflow_signature: None,
    };
    let document = serde_json::to_string(&request).unwrap();
    let request: build_machine_core::request::WorkRequest = serde_json::from_str(&document).unwrap();
    let order: Vec<(&str, &str)> =
        ordered_steps(&request).into_iter().map(|(stage, step)| (stage, step.name.as_str())).collect();
    assert_eq!(
        order,
        [
            ("setup", "actions/checkout@v5"),
            ("test", "database servers"),
            ("setup", "environment of the database servers"),
            ("test", "make check"),
            ("smoke", "decoder fuzz smoke checks"),
            ("test", "Go"),
        ]
    );
}

/// A job asks GitHub for a runner image; the replay runs on this machine. A
/// difference the label states — another Ubuntu release or another
/// architecture — is recorded, not passed over.
#[test]
fn a_runner_image_the_machine_does_not_match_is_a_recorded_limit() {
    use build_machine_worker::ci::runner_image_limit;
    assert_eq!(runner_image_limit("ubuntu-26.04-arm", "26.04", "aarch64"), None);
    assert_eq!(runner_image_limit("ubuntu-26.04", "26.04", "x86_64"), None);
    let release = runner_image_limit("ubuntu-24.04-arm", "26.04", "aarch64").unwrap();
    assert!(release.contains("24.04") && release.contains("26.04"), "{release}");
    let arch = runner_image_limit("ubuntu-26.04", "26.04", "aarch64").unwrap();
    assert!(arch.contains("x86_64") || arch.contains("X64"), "{arch}");
    // ubuntu-latest names no release, which is itself a difference to record.
    assert!(runner_image_limit("ubuntu-latest", "26.04", "aarch64").is_some());
}

/// `$GITHUB_STEP_SUMMARY` exists for every step, as on a runner, so a step
/// that appends to it does not fail on an unset variable.
#[test]
fn every_step_has_a_step_summary_file() {
    use build_machine_worker::runner::Runner;
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(&directory.path().join("runner"), directory.path(), build_machine_core::Platform::Linux).unwrap();
    let files = runner.begin_step(1).unwrap();
    let environment = runner.environment(&[], &files);
    let summary = environment.iter().find(|(key, _)| key == "GITHUB_STEP_SUMMARY").map(|(_, value)| value.clone()).unwrap();
    assert!(std::path::Path::new(&summary).is_file());
}

/// A job's workspace is removed when the job ends: two orm replays left 21 GB
/// of workspaces, a Rust target of 7 GB each, in the Linux machine. What the
/// result names — the artifacts with their checksums — is kept beside it.
#[test]
fn a_finished_job_leaves_only_its_artifacts() {
    use build_machine_core::report::Artifact;
    use build_machine_worker::ci::finish_job;
    let directory = tempfile::tempdir().unwrap();
    let work = directory.path().join("work");
    let workspace = work.join("orm").join("orm");
    let bundle = workspace.join("src-tauri/target/release/bundle/deb");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::create_dir_all(workspace.join("target/debug")).unwrap();
    std::fs::write(workspace.join("target/debug/huge"), vec![0u8; 4096]).unwrap();
    std::fs::write(bundle.join("app.deb"), b"package").unwrap();
    let sha256 = build_machine_core::source::sha256_file(&bundle.join("app.deb")).unwrap();
    let elsewhere = directory.path().join("other.deb");
    std::fs::write(&elsewhere, b"not this job's").unwrap();
    let mut artifacts = vec![
        Artifact { path: bundle.join("app.deb").to_string_lossy().into_owned(), sha256: sha256.clone(), size: 7 },
        Artifact { path: elsewhere.to_string_lossy().into_owned(), sha256: "x".to_owned(), size: 1 },
    ];
    let keep = directory.path().join("artifacts").join("test");

    finish_job(&work, &keep, &mut artifacts).unwrap();

    assert!(!work.exists());
    let kept = keep.join("orm/orm/src-tauri/target/release/bundle/deb/app.deb");
    assert_eq!(artifacts[0].path, kept.to_string_lossy());
    assert_eq!(build_machine_core::source::sha256_file(&kept).unwrap(), sha256);
    assert_eq!(artifacts[1].path, elsewhere.to_string_lossy());
}
