//! `github.event`, the payload of the replayed event.
//!
//! The soksak registry's `validate.yml` checks out `github.event.pull_request
//! .base.sha` and `.head.sha` and reads `.user.login` and `.number`; its
//! `merge.yml` reads `github.event.workflow_run.id`. A replay is given the
//! payload as GitHub sends it, and a path the payload does not hold fails
//! validation instead of becoming an empty value.

use build_machine_core::workflow::{self, Outputs};
use serde_json::json;

fn load(steps: &str) -> workflow::Workflow {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("validate.yml");
    std::fs::write(
        &path,
        format!("# build-machine: skip build reason=fixture\n# build-machine: skip test reason=fixture\n# build-machine: skip smoke reason=fixture\nname: validate\non: pull_request\njobs:\n  validate:\n    runs-on: ubuntu-26.04-arm\n    steps:\n{steps}"),
    )
    .unwrap();
    workflow::load(&path, "pull_request", None).unwrap()
}

fn pull_request() -> serde_json::Value {
    json!({
        "number": 7,
        "pull_request": {
            "number": 7,
            "base": { "sha": "1111111" },
            "head": { "sha": "2222222", "repo": { "full_name": "alice/registry" } },
            "user": { "login": "alice" },
            "draft": false
        }
    })
}

#[test]
fn an_expression_reads_the_event_payload() {
    let parsed = load(
        r#"      - name: check
        if: github.event.pull_request.draft == false
        env:
          AUTHOR: ${{ github.event.pull_request.user.login }}
        run: echo ${{ github.event.pull_request.number }} ${{ github.event.pull_request.head.repo.full_name }} ${{ github.event.pull_request.base.sha }}
"#,
    );
    let payload = pull_request();
    let github = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), Some(&payload)).unwrap();
    let step = parsed.jobs[0].steps[0].resolve(&Outputs::new(), &github).unwrap();
    assert_eq!(step.env["AUTHOR"], "alice");
    assert_eq!(step.run.as_deref(), Some("echo 7 alice/registry 1111111"));
    assert!(workflow::Condition::parse(Some("github.event.pull_request.draft == false"))
        .unwrap()
        .runs(workflow::JobStatus::Success, &Outputs::new(), &github));
}

#[test]
fn a_path_the_payload_does_not_hold_fails_validation() {
    let parsed = load("      - run: echo ${{ github.event.pull_request.merged_by.login }}\n");
    let payload = pull_request();
    let error = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), Some(&payload)).unwrap_err();
    assert!(format!("{error:#}").contains("github.event.pull_request.merged_by.login"), "{error:#}");
    let error = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), None).unwrap_err();
    assert!(format!("{error:#}").contains("--event-payload"), "{error:#}");
}

#[test]
fn a_workflow_that_reads_no_payload_needs_none() {
    let parsed = load("      - run: echo ${{ github.sha }}\n");
    let github = workflow::github_context(&parsed.jobs, "push", "2222222", None, None).unwrap();
    assert_eq!(github.event, None);
}
