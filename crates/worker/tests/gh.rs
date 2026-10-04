//! The `gh` of a replay: it answers `gh pr view` from the declared pull
//! request and turns `gh pr merge` into a recorded dry run, so a replayed
//! step never changes GitHub.

use build_machine_core::source::PullRequest;
use build_machine_worker::gh::{install, merges};
use std::process::Command;

fn pull_request() -> PullRequest {
    PullRequest {
        number: 7,
        head_sha: "2222222222222222222222222222222222222222".to_owned(),
        files: vec!["plugins/probe.json".to_owned(), "sidecars/acme-worker.json".to_owned()],
    }
}

fn gh(directory: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    Command::new(directory.join("gh")).args(arguments).output().unwrap()
}

#[test]
fn gh_pr_view_answers_from_the_declared_pull_request() {
    let directory = tempfile::tempdir().unwrap();
    let bin = install(directory.path(), Some(&pull_request())).unwrap();
    let output = gh(&bin, &["pr", "view", "7", "--repo", "soksak-app/registry", "--json", "files", "--jq", ".files[].path"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "plugins/probe.json\nsidecars/acme-worker.json\n");
    let output = gh(&bin, &["pr", "view", "7", "--json", "headRefOid", "--jq", ".headRefOid"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "2222222222222222222222222222222222222222\n");
    let other = gh(&bin, &["pr", "view", "8", "--json", "files"]);
    assert!(!other.status.success());
}

#[test]
fn gh_pr_merge_is_a_recorded_dry_run_of_the_checked_commit() {
    let directory = tempfile::tempdir().unwrap();
    let bin = install(directory.path(), Some(&pull_request())).unwrap();
    let moved = gh(&bin, &["pr", "merge", "7", "--squash", "--match-head-commit", "3333333333333333333333333333333333333333"]);
    assert!(!moved.status.success());
    assert_eq!(merges(&bin).unwrap(), Vec::<String>::new());
    let output = gh(
        &bin,
        &["pr", "merge", "7", "--repo", "soksak-app/registry", "--squash", "--match-head-commit", "2222222222222222222222222222222222222222"],
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("dry-run"));
    assert_eq!(merges(&bin).unwrap(), ["squash 7 2222222222222222222222222222222222222222"]);
}

#[test]
fn any_other_gh_command_and_a_replay_without_a_pull_request_fail() {
    let directory = tempfile::tempdir().unwrap();
    let bin = install(directory.path(), Some(&pull_request())).unwrap();
    let output = gh(&bin, &["release", "create", "v1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("gh release"));
    let without = tempfile::tempdir().unwrap();
    let bin = install(without.path(), None).unwrap();
    let output = gh(&bin, &["pr", "view", "7", "--json", "files"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--pull-request"));
}
