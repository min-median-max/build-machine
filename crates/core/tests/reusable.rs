//! A job that calls a reusable workflow of the same repository with
//! `uses: ./.github/workflows/<file>`.
//!
//! The soksak registry's `merge.yml` merges a checked pull request and then
//! runs `publish.yml`, which declares `on: workflow_call`, as its job
//! `publish`.

use build_machine_core::workflow;

const HEADER: &str = "# build-machine: skip build reason=fixture\n# build-machine: skip test reason=fixture\n# build-machine: skip smoke reason=fixture\n";

fn repository(merge: &str, publish: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let workflows = directory.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(workflows.join("publish.yml"), publish).unwrap();
    let path = workflows.join("merge.yml");
    std::fs::write(&path, format!("{HEADER}{merge}")).unwrap();
    (directory, path)
}

const PUBLISH: &str = "name: publish\non:\n  push:\n    branches: [main]\n  workflow_call:\nenv:\n  CORE_RELEASE: v0.0.3\njobs:\n  build:\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - run: make build\n  deploy:\n    runs-on: ubuntu-26.04-arm\n    needs: build\n    steps:\n      - run: echo $CORE_RELEASE\n";

#[test]
fn a_called_workflow_s_jobs_run_in_place_of_the_calling_job() {
    let (_directory, path) = repository(
        "name: merge\non: workflow_dispatch\njobs:\n  merge:\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - run: make merge\n  publish:\n    needs: merge\n    uses: ./.github/workflows/publish.yml\n    permissions:\n      pages: write\n  after:\n    runs-on: ubuntu-26.04-arm\n    needs: publish\n    steps:\n      - run: make after\n",
        PUBLISH,
    );
    let parsed = workflow::load(&path, "workflow_dispatch", None).unwrap();
    let jobs: Vec<(&str, Vec<&str>)> =
        parsed.jobs.iter().map(|job| (job.id.as_str(), job.needs.iter().map(String::as_str).collect())).collect();
    assert_eq!(
        jobs,
        [
            ("merge", vec![]),
            ("publish/build", vec!["merge"]),
            ("publish/deploy", vec!["publish/build"]),
            ("after", vec!["publish/build", "publish/deploy"]),
        ]
    );
    // The called workflow's env reaches its jobs.
    assert_eq!(parsed.jobs[2].env["CORE_RELEASE"], "v0.0.3");
}

#[test]
fn a_call_is_refused_where_the_replay_would_differ() {
    let call = |job: &str, publish: &str| {
        let (_directory, path) = repository(&format!("name: merge\non: workflow_dispatch\njobs:\n  publish:\n{job}"), publish);
        workflow::load(&path, "workflow_dispatch", None).unwrap_err()
    };
    let not_callable = PUBLISH.replace("  workflow_call:\n", "");
    let error = call("    uses: ./.github/workflows/publish.yml\n", &not_callable);
    assert!(format!("{error:#}").contains("workflow_call"), "{error:#}");
    for (key, job) in [
        ("with", "    uses: ./.github/workflows/publish.yml\n    with:\n      target: main\n"),
        ("secrets", "    uses: ./.github/workflows/publish.yml\n    secrets: inherit\n"),
        ("if", "    uses: ./.github/workflows/publish.yml\n    if: always()\n"),
        ("uses", "    uses: soksak-app/registry/.github/workflows/publish.yml@main\n"),
    ] {
        let error = call(job, PUBLISH);
        assert!(format!("{error:#}").contains(key), "{key}: {error:#}");
    }
}
