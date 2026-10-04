//! `actions/checkout` of the workflow's own repository at a `ref`, and of a
//! repository that an expression names.
//!
//! The soksak registry's `validate.yml` checks out the base commit of a pull
//! request and its head, named by `github.event.pull_request`, and its
//! `publish.yml` checks out `main`.

use build_machine_core::workflow::{self, checkout_target};
use serde_json::json;
use std::collections::BTreeMap;

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

fn payload(head_repository: &str) -> serde_json::Value {
    json!({ "pull_request": { "base": { "sha": "1111111" }, "head": { "sha": "2222222", "repo": { "full_name": head_repository } } } })
}

#[test]
fn a_checkout_with_only_a_ref_checks_out_the_own_repository() {
    let with: BTreeMap<String, String> =
        [("ref".to_owned(), "main".to_owned()), ("path".to_owned(), "registry".to_owned())].into();
    let target = checkout_target(&with).unwrap().unwrap();
    assert_eq!(target.repository, None);
    assert_eq!(target.reference.as_deref(), Some("main"));
    assert_eq!(target.path.as_deref(), Some("registry"));
    let plain: BTreeMap<String, String> = BTreeMap::new();
    assert_eq!(checkout_target(&plain).unwrap(), None);
}

#[test]
fn a_repository_named_by_an_expression_is_found_with_the_event_payload() {
    let parsed = load(
        r#"      - uses: actions/checkout@v4
        with:
          ref: ${{ github.event.pull_request.base.sha }}
          path: base
      - uses: actions/checkout@v4
        with:
          repository: ${{ github.event.pull_request.head.repo.full_name }}
          ref: ${{ github.event.pull_request.head.sha }}
          path: head
"#,
    );
    let repositories: BTreeMap<String, String> =
        [("soksak-app/registry".to_owned(), "/work/registry".to_owned())].into();
    let event = payload("soksak-app/registry");
    let github = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), Some(&event)).unwrap();
    let found = workflow::checkout_repositories(&parsed, &repositories, &github).unwrap();
    assert_eq!(found.into_iter().collect::<Vec<_>>(), [("soksak-app/registry".to_owned(), "/work/registry".into())]);

    let fork = payload("alice/registry");
    let github = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), Some(&fork)).unwrap();
    let error = workflow::checkout_repositories(&parsed, &repositories, &github).unwrap_err();
    assert!(format!("{error:#}").contains("alice/registry"), "{error:#}");

    let invalid = payload("not a name");
    let github = workflow::github_context(&parsed.jobs, "pull_request", "2222222", Some("refs/heads/topic"), Some(&invalid)).unwrap();
    let error = workflow::checkout_repositories(&parsed, &repositories, &github).unwrap_err();
    assert!(format!("{error:#}").contains("owner/name"), "{error:#}");
}
