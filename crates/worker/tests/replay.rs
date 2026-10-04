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
        snapshot: serde_json::from_value(serde_json::json!({
            "revision": "r", "dirty": false, "sourceHash": "h", "fileCount": 0, "sourceMode": "local",
            "projectKey": "orm", "project": "/orm", "archive": ""
        }))
        .unwrap(),
        archive: String::new(),
        history: None,
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
