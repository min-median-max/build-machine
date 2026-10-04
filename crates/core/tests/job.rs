//! Job and workflow keys, and the time limits a runner applies.
//!
//! A key a replay does not implement changes what GitHub runs, so it fails
//! validation instead of being ignored. `timeout-minutes` is read where the
//! workflow writes it, with GitHub's default of 360 minutes per job.

use build_machine_core::workflow;
use build_machine_core::workflow::JobStatus;

const GATES: &str = "# build-machine: skip build reason=fixture\n# build-machine: skip smoke reason=fixture\n";

fn load(text: &str) -> anyhow::Result<workflow::Workflow> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ci.yml");
    std::fs::write(&path, format!("{GATES}{text}")).unwrap();
    workflow::load(&path, "workflow_dispatch", None)
}

fn job_with(key: &str) -> String {
    format!("name: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    {key}\n    steps:\n      - run: make check\n")
}

#[test]
fn a_job_key_the_replay_does_not_implement_fails_closed() {
    for key in [
        "continue-on-error: true",
        "defaults:\n      run:\n        shell: sh",
        "outputs:\n      version: x",
        "strategy:\n      fail-fast: false",
        "container: node:22",
        "services:\n      db:\n        image: mysql",
        "uses: ./.github/workflows/other.yml",
    ] {
        let error = load(&job_with(key)).unwrap_err();
        let name = key.split(':').next().unwrap();
        assert!(format!("{error:#}").contains(&format!("'{name}'")), "{key}: {error:#}");
    }
    for accepted in [
        "name: tests",
        "timeout-minutes: 30",
        "env:\n      A: b",
        "environment: production",
        "if: github.ref == 'refs/heads/main'",
        "permissions:\n      contents: read",
    ] {
        load(&job_with(accepted)).unwrap_or_else(|error| panic!("{accepted}: {error:#}"));
    }
}

#[test]
fn a_workflow_key_the_replay_does_not_implement_fails_closed() {
    let text = "name: ci\non: workflow_dispatch\ndefaults:\n  run:\n    shell: sh\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: make check\n";
    let error = load(text).unwrap_err();
    assert!(format!("{error:#}").contains("'defaults'"), "{error:#}");
    for accepted in ["permissions:\n  contents: read\n", "concurrency: ci\n", "run-name: check\n"] {
        load(&format!("{accepted}{}", job_with("name: tests"))).unwrap_or_else(|error| panic!("{accepted}: {error:#}"));
    }
}

/// Workflow `env` reaches every step, under the job's and the step's own.
#[test]
fn workflow_env_reaches_every_step_under_job_and_step_env() {
    let parsed = load(
        "name: ci\non: workflow_dispatch\nenv:\n  A: workflow\n  B: workflow\n  C: workflow\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    env:\n      B: job\n      C: job\n    steps:\n      - run: make check\n        env:\n          C: step\n",
    )
    .unwrap();
    let step = &parsed.jobs[0].steps[0];
    assert_eq!(step.job_env["A"], "workflow");
    assert_eq!(step.job_env["B"], "job");
    assert_eq!(step.env["C"], "step");
}

#[test]
fn timeouts_are_the_workflow_s_own_with_github_s_job_default() {
    let parsed = load(
        "name: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n    steps:\n      - run: make check\n        timeout-minutes: 5\n      - run: make fuzz-check\n  other:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: make check\n",
    )
    .unwrap();
    let test = parsed.jobs.iter().find(|job| job.id == "test").unwrap();
    let other = parsed.jobs.iter().find(|job| job.id == "other").unwrap();
    assert_eq!(test.timeout_minutes, 30);
    assert_eq!(test.steps[0].timeout_minutes, Some(5));
    assert_eq!(test.steps[1].timeout_minutes, None);
    assert_eq!(other.timeout_minutes, 360);

    for invalid in ["0", "-1", "1.5", "${{ inputs.minutes }}", "soon"] {
        let error = load(&job_with(&format!("timeout-minutes: {invalid}"))).unwrap_err();
        assert!(format!("{error:#}").contains("timeout-minutes"), "{invalid}: {error:#}");
    }
}

/// A job that times out is cancelled, as GitHub cancels it: `success()` and
/// `failure()` are false after that and `cancelled()` is true.
#[test]
fn a_cancelled_job_runs_only_steps_that_ask_to_run_after_cancellation() {
    use workflow::Condition;
    let runs = |text: Option<&str>, status: JobStatus| Condition::parse(text).unwrap().runs(status, &Default::default(), &Default::default());
    assert!(!runs(None, JobStatus::Cancelled));
    assert!(!runs(Some("${{ !cancelled() }}"), JobStatus::Cancelled));
    assert!(!runs(Some("failure()"), JobStatus::Cancelled));
    assert!(runs(Some("always()"), JobStatus::Cancelled));
    assert!(runs(Some("cancelled()"), JobStatus::Cancelled));
    assert!(runs(Some("${{ !cancelled() }}"), JobStatus::Failure));
    assert!(!runs(Some("cancelled()"), JobStatus::Success));
}

/// Each platform replays its own jobs, so a job that waits for another
/// platform's job could never be replayed as GitHub runs it.
#[test]
fn a_job_cannot_need_a_job_of_another_platform() {
    let error = load(
        "name: ci\non: workflow_dispatch\njobs:\n  build:\n    runs-on: macos-14\n    steps:\n      - run: make check\n  publish:\n    runs-on: ubuntu-24.04\n    needs: build\n    steps:\n      - run: make check\n",
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("needs"), "{error:#}");
}

/// `actions/checkout` reads `fetch-depth`, `fetch-tags` and a `path` inside
/// the workspace; an input that
/// would check out something else fails validation instead of being ignored.
#[test]
fn checkout_inputs_are_read_or_refused() {
    let checkout = |with: &str| {
        load(&format!(
            "name: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: actions/checkout@v5\n        with:\n          {with}\n      - run: make check\n"
        ))
    };
    checkout("fetch-depth: 0").unwrap();
    checkout("fetch-tags: true").unwrap();
    checkout("path: src").unwrap();
    for refused in ["ref: main", "path: ../src", "submodules: true", "fetch-depth: all", "fetch-tags: yes"] {
        let error = checkout(refused).unwrap_err();
        let name = refused.split(':').next().unwrap();
        assert!(format!("{error:#}").contains(name), "{refused}: {error:#}");
    }
}
