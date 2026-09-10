use build_machine_core::workflow;
use std::path::PathBuf;

fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let workflows = directory.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    let path = workflows.join("build.yml");
    std::fs::write(&path, text).unwrap();
    (directory, path)
}

fn load(text: &str) -> anyhow::Result<workflow::Workflow> {
    let (_directory, path) = write(text);
    workflow::load(&path, "workflow_dispatch", None)
}

#[test]
fn supported_steps_and_explicit_skip_markers_are_read() {
    let parsed = load(
        r#"# build-machine: skip test reason=no test command in this release workflow
# build-machine: skip smoke reason=smoke is verified by the desktop fixture
name: release
on:
  workflow_dispatch:
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - run: pnpm install --frozen-lockfile
      - uses: tauri-apps/tauri-action@v0
        with:
          args: --target universal-apple-darwin
"#,
    )
    .unwrap();
    assert_eq!(parsed.name, "release");
    assert_eq!(parsed.skips["smoke"], "smoke is verified by the desktop fixture");
    assert_eq!(parsed.jobs[0].steps.len(), 4);
    let stages = workflow::stages(&parsed);
    assert_eq!(stages["build"][0].adapter, workflow::Adapter::TauriBuild);
}

#[test]
fn unsupported_action_fails_closed() {
    let error = load(
        r#"name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - uses: evil/action@v1
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("어댑터"), "{error:#}");
}

#[test]
fn a_missing_gate_requires_a_reasoned_comment() {
    let error = load(
        r#"name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - name: build
        run: echo build
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("test 단계"), "{error:#}");
}

/// `actions/checkout` contains "check". Reading an action's own name as a stage
/// let a workflow with no test command satisfy the test gate.
#[test]
fn checkout_does_not_satisfy_the_test_gate() {
    let error = load(
        r#"name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("test 단계"), "{error:#}");
}

#[test]
fn action_steps_are_classified_by_adapter_not_by_name() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - name: Restore build cache
        uses: actions/cache@v4
      - uses: tauri-apps/tauri-action@v0
      - uses: actions/upload-artifact@v4
"#,
    )
    .unwrap();
    let stages = workflow::stages(&parsed);
    let adapters = |stage: &str| -> Vec<workflow::Adapter> {
        stages[stage].iter().map(|step| step.adapter).collect()
    };
    assert_eq!(adapters("setup"), [workflow::Adapter::Checkout, workflow::Adapter::Cache]);
    assert_eq!(adapters("build"), [workflow::Adapter::TauriBuild]);
    assert_eq!(adapters("release"), [workflow::Adapter::ArtifactUpload]);
    assert_eq!(adapters("test"), [workflow::Adapter::Skip]);
}

#[test]
fn an_unsupported_job_shape_fails_closed() {
    for unsupported in ["container: node:20", "strategy:\n      matrix:\n        os: [a, b]"] {
        let text = format!(
            "name: bad\non: workflow_dispatch\njobs:\n  build:\n    runs-on: ubuntu\n    {unsupported}\n    steps:\n      - run: echo build\n"
        );
        let error = load(&text).unwrap_err();
        assert!(format!("{error:#}").contains("지원하지 않아요"), "{error:#}");
    }
}

#[test]
fn an_event_the_workflow_does_not_declare_is_rejected() {
    let (_directory, path) = write("name: bad\non: push\njobs:\n  build:\n    runs-on: ubuntu\n    steps:\n      - run: echo build\n");
    let error = workflow::load(&path, "workflow_dispatch", None).unwrap_err();
    assert!(format!("{error:#}").contains("workflow_dispatch 이벤트"), "{error:#}");
}

#[test]
fn job_order_follows_needs_and_a_cycle_is_rejected() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: ordered
on: workflow_dispatch
jobs:
  publish:
    runs-on: ubuntu
    needs: compile
    steps:
      - uses: actions/upload-artifact@v4
  compile:
    runs-on: ubuntu
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    assert_eq!(parsed.jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(), ["compile", "publish"]);

    let error = load(
        r#"name: cycle
on: workflow_dispatch
jobs:
  a:
    runs-on: ubuntu
    needs: b
    steps:
      - run: echo build
  b:
    runs-on: ubuntu
    needs: a
    steps:
      - run: echo build
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("순환"), "{error:#}");
}

/// The controller writes the stages into a request document and the worker
/// reads them back. An adapter that does not survive that trip is read as a
/// shell step, and the run fails on a command that was never there.
#[test]
fn every_adapter_survives_the_request_document() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
      - run: echo compile
      - uses: tauri-apps/tauri-action@v0
      - uses: actions/upload-artifact@v4
      - uses: softprops/action-gh-release@v2
"#,
    )
    .unwrap();
    let stages = workflow::stages(&parsed);
    let document = serde_json::to_string(&stages).unwrap();
    let restored: std::collections::BTreeMap<String, Vec<workflow::Step>> =
        serde_json::from_str(&document).unwrap();
    assert_eq!(restored.len(), stages.len());
    for (stage, steps) in &stages {
        let read_back = &restored[stage];
        assert_eq!(read_back.len(), steps.len(), "{stage}");
        for (before, after) in steps.iter().zip(read_back) {
            assert_eq!(after.adapter, before.adapter, "{stage} step {}", before.index);
            assert_eq!(after.run, before.run, "{stage} step {}", before.index);
            assert_eq!(after.reason, before.reason, "{stage} step {}", before.index);
        }
    }
    // The one adapter that is not a shell step must not read back as one.
    assert!(restored["setup"].iter().all(|step| step.adapter != workflow::Adapter::Run));
}

/// A workflow written for one runner cannot be carried out on another. Saying
/// so before anything starts beats failing inside someone else's build.
#[test]
fn a_replay_is_refused_on_a_platform_the_workflow_was_not_written_for() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    assert!(workflow::check_platform(&parsed, build_machine_core::Platform::Macos).is_ok());
    let error = workflow::check_platform(&parsed, build_machine_core::Platform::Linux).unwrap_err();
    assert!(format!("{error:#}").contains("재현할 수 없습니다"), "{error:#}");
}

/// A workflow that names no runner this machine knows places no constraint.
#[test]
fn an_unfamiliar_runner_does_not_constrain_the_replay() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: self-hosted
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    for platform in build_machine_core::Platform::ALL {
        assert!(workflow::check_platform(&parsed, platform).is_ok(), "{platform}");
    }
}
