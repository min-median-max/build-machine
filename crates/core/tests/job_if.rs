//! A job's `if:`, read as GitHub reads it.
//!
//! orm's docs-pages.yml deploys only for a push to main:
//! `if: github.event_name != 'pull_request' && github.ref == 'refs/heads/main'`,
//! and `ci validate` refused it with "job deploy의 'if'는 아직 지원하지 않아요".

use build_machine_core::workflow::{self, Condition, Github, JobResult};
use std::collections::BTreeMap;

const GATES: &str = "# build-machine: skip build reason=fixture\n# build-machine: skip test reason=fixture\n# build-machine: skip smoke reason=fixture\n";

fn load(jobs: &str) -> anyhow::Result<workflow::Workflow> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("docs-pages.yml");
    std::fs::write(&path, format!("{GATES}name: docs-pages\non:\n  push:\n  workflow_dispatch:\njobs:\n{jobs}")).unwrap();
    workflow::load(&path, "push", None)
}

const BUILD: &str = "  build:\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - run: make docs\n";

#[test]
fn orm_s_deploy_job_condition_and_permissions_pass_validation() {
    let parsed = load(&format!(
        "{BUILD}  deploy:\n    if: github.event_name != 'pull_request' && github.ref == 'refs/heads/main'\n    needs: build\n    runs-on: ubuntu-26.04-arm\n    permissions:\n      pages: write\n      id-token: write\n    steps:\n      - run: echo deploy\n"
    ))
    .unwrap();
    let deploy = parsed.jobs.iter().find(|job| job.id == "deploy").unwrap();
    assert!(deploy.condition.is_some());
    assert!(deploy.reads_ref);
}

fn github(event: &str, reference: &str) -> Github {
    Github { event_name: event.to_owned(), sha: "s".to_owned(), reference: Some(reference.to_owned()) }
}

#[test]
fn a_job_condition_follows_the_jobs_it_needs() {
    let runs = |text: Option<&str>, ancestors: &[JobResult], needs: &[(&str, JobResult)], github: &Github| {
        let needs: BTreeMap<String, JobResult> = needs.iter().map(|(job, result)| ((*job).to_owned(), *result)).collect();
        Condition::parse(text).unwrap().job_runs(ancestors, &needs, github)
    };
    let main = github("push", "refs/heads/main");
    let ok = [JobResult::Success];
    // No condition is success(): every job it needs, and theirs, succeeded.
    assert!(runs(None, &ok, &[("build", JobResult::Success)], &main));
    assert!(!runs(None, &[JobResult::Failure], &[("build", JobResult::Failure)], &main));
    assert!(!runs(None, &[JobResult::Skipped], &[("build", JobResult::Skipped)], &main));
    // orm's deploy condition: a push to main runs it, a pull request or another branch does not.
    let deploy = "github.event_name != 'pull_request' && github.ref == 'refs/heads/main'";
    assert!(runs(Some(deploy), &ok, &[("build", JobResult::Success)], &main));
    assert!(!runs(Some(deploy), &ok, &[("build", JobResult::Success)], &github("pull_request", "refs/heads/main")));
    assert!(!runs(Some(deploy), &ok, &[("build", JobResult::Success)], &github("push", "refs/heads/topic")));
    // Without a status function the condition still needs success().
    assert!(!runs(Some(deploy), &[JobResult::Failure], &[("build", JobResult::Failure)], &main));
    // Status functions and needs.<job>.result.
    assert!(runs(Some("always()"), &[JobResult::Failure], &[("build", JobResult::Failure)], &main));
    assert!(runs(Some("failure()"), &[JobResult::Failure], &[("build", JobResult::Failure)], &main));
    assert!(!runs(Some("failure()"), &[JobResult::Skipped], &[("build", JobResult::Skipped)], &main));
    assert!(runs(Some("${{ always() && needs.build.result == 'skipped' }}"), &[JobResult::Skipped], &[("build", JobResult::Skipped)], &main));
    assert!(!runs(Some("cancelled()"), &ok, &[("build", JobResult::Success)], &main));
}

#[test]
fn a_job_condition_without_a_local_value_fails_validation_naming_it() {
    for (condition, expected) in [
        ("needs.other.result == 'success'", "other"),
        ("needs.build.outputs.version != ''", "needs.build.outputs.version"),
        ("steps.a.outputs.b != ''", "steps.a.outputs.b"),
        ("github.actor == 'me'", "github.actor"),
    ] {
        let error = load(&format!(
            "{BUILD}  deploy:\n    if: {condition}\n    needs: build\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - run: echo deploy\n"
        ))
        .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{condition}: {error:#}");
    }
    // needs is read in a job's if, not in a step's.
    let error = load(&format!(
        "{BUILD}  deploy:\n    needs: build\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - run: echo deploy\n        if: needs.build.result == 'success'\n"
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("needs.build.result"), "{error:#}");
}
