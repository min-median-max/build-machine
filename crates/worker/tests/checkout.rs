//! `actions/checkout` gives a job a Git repository at the replayed revision.
//!
//! orm's `make check` reads history (`git log <baseline>..HEAD`), the tracked
//! files (`git ls-files`) and searches them (`git grep`), so a checkout without
//! `.git` fails checks that pass on a runner.

use build_machine_core::source;
use build_machine_worker::checkout::{checkout, checkout_repository, Checkout, RepositoryCheckout};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Fixture {
    _directory: tempfile::TempDir,
    base: PathBuf,
    project: PathBuf,
}

/// Three commits on main, a tag on the first, and a side branch.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let project = base.join("orm");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["config", "user.email", "test@example.invalid"]);
    git(&project, &["config", "user.name", "test"]);
    std::fs::write(project.join(".gitignore"), "ignored/\n").unwrap();
    for (index, subject) in ["feat(core): Add one", "fix(core): Fix two", "docs(core): Note three"].iter().enumerate() {
        std::fs::write(project.join("value.txt"), format!("commit {index}")).unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-qm", subject]);
        if index == 0 {
            git(&project, &["tag", "v0.1.0"]);
            git(&project, &["branch", "side"]);
        }
    }
    Fixture { _directory: directory, base, project }
}

/// Snapshot the project as the controller does and check it out as a job does.
fn replay(fixture: &Fixture, reference: Option<&str>, depth: u32, name: &str) -> (PathBuf, String) {
    let archive = fixture.base.join(format!("{name}.zip"));
    let history = fixture.base.join(format!("{name}.bundle"));
    let (revision, dirty, _hash, _count, _mode) = source::make_archive(&fixture.project, &archive, reference).unwrap();
    let recorded = source::make_history(&fixture.project, reference, &history).unwrap();
    let workspace = fixture.base.join(name).join("orm");
    std::fs::create_dir_all(&workspace).unwrap();
    let summary = checkout(
        &Checkout {
            history: &history,
            mirror: &fixture.base.join(format!("{name}-mirror.git")),
            workspace: &workspace,
            archive: &archive,
            revision: &revision,
            reference: recorded.reference.as_deref(),
            dirty,
            fetch_depth: depth,
            fetch_tags: false,
        },
        &[("PATH".to_owned(), std::env::var("PATH").unwrap())],
    )
    .unwrap();
    assert!(!summary.is_empty());
    (workspace, revision)
}

#[test]
fn fetch_depth_zero_carries_every_branch_and_tag_and_checks_out_the_branch() {
    let fixture = fixture();
    let (workspace, revision) = replay(&fixture, None, 0, "full");
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), revision);
    assert_eq!(git(&workspace, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(git(&workspace, &["log", "--format=%s"]).lines().count(), 3);
    assert_eq!(git(&workspace, &["log", "--format=%s", "v0.1.0..HEAD"]).lines().count(), 2);
    assert!(git(&workspace, &["branch", "-r"]).contains("origin/side"));
    assert_eq!(git(&workspace, &["status", "--porcelain"]), "");
    assert_eq!(std::fs::read_to_string(workspace.join("value.txt")).unwrap(), "commit 2");
}

#[test]
fn the_default_depth_is_one_commit_without_tags() {
    let fixture = fixture();
    let (workspace, revision) = replay(&fixture, None, 1, "shallow");
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), revision);
    assert_eq!(git(&workspace, &["log", "--format=%s"]).lines().count(), 1);
    assert_eq!(git(&workspace, &["rev-parse", "--is-shallow-repository"]), "true");
    assert_eq!(git(&workspace, &["tag"]), "");
}

#[test]
fn a_tag_is_checked_out_at_its_commit() {
    let fixture = fixture();
    let (workspace, revision) = replay(&fixture, Some("v0.1.0"), 0, "tag");
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), revision);
    assert_eq!(std::fs::read_to_string(workspace.join("value.txt")).unwrap(), "commit 0");
    assert_eq!(git(&workspace, &["status", "--porcelain"]), "");
}

/// Uncommitted edits, deletions and untracked files appear staged on top of
/// the replayed commit, whose history stays the repository's own: the
/// workflow sees the files that would be committed next, and `git log` sees
/// only real commits.
#[test]
fn uncommitted_changes_are_staged_on_the_replayed_commit() {
    let fixture = fixture();
    std::fs::write(fixture.project.join("value.txt"), "edited").unwrap();
    std::fs::write(fixture.project.join("new.txt"), "untracked").unwrap();
    std::fs::remove_file(fixture.project.join(".gitignore")).unwrap();
    std::fs::write(fixture.project.join(".gitignore"), "ignored/\nnot-this.txt\n").unwrap();
    std::fs::write(fixture.project.join("not-this.txt"), "ignored").unwrap();
    let (workspace, revision) = replay(&fixture, None, 0, "dirty");
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), revision);
    assert_eq!(git(&workspace, &["log", "--format=%s"]).lines().count(), 3);
    let status = git(&workspace, &["status", "--porcelain"]);
    assert!(status.contains("M  value.txt"), "{status}");
    assert!(status.contains("A  new.txt"), "{status}");
    assert!(git(&workspace, &["ls-files"]).lines().any(|name| name == "new.txt"));
    assert!(!workspace.join("not-this.txt").exists());
    assert_eq!(std::fs::read_to_string(workspace.join("value.txt")).unwrap(), "edited");
}

/// Another repository a step names, checked out from its bundle into
/// `directory`: `fixture.project` stands for that repository's local clone.
fn other(fixture: &Fixture, reference: Option<&str>, depth: u32, name: &str) -> anyhow::Result<(PathBuf, String)> {
    let history = fixture.base.join(format!("{name}-other.bundle"));
    source::make_history(&fixture.project, None, &history).unwrap();
    let workspace = fixture.base.join(name).join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    // The job's own checkout is already in the workspace.
    std::fs::write(workspace.join("go.mod"), "module files").unwrap();
    let directory = workspace.join("core");
    let summary = checkout_repository(
        &RepositoryCheckout {
            repository: "example/orm",
            history: &history,
            mirror: &fixture.base.join(format!("{name}-other-mirror.git")),
            directory: &directory,
            reference,
            fetch_depth: depth,
            fetch_tags: false,
        },
        &[("PATH".to_owned(), std::env::var("PATH").unwrap())],
    )?;
    Ok((directory, summary))
}

/// The soksak releases check out `soksak-app/core` at the release tag into
/// `core`: the tag's commit, without the clone's uncommitted edits.
#[test]
fn another_repository_is_checked_out_at_a_tag_into_its_path() {
    let fixture = fixture();
    std::fs::write(fixture.project.join("value.txt"), "uncommitted").unwrap();
    let (directory, summary) = other(&fixture, Some("v0.1.0"), 1, "tag-path").unwrap();
    assert_eq!(git(&directory, &["rev-parse", "HEAD"]), git(&fixture.project, &["rev-parse", "v0.1.0^{commit}"]));
    assert_eq!(std::fs::read_to_string(directory.join("value.txt")).unwrap(), "commit 0");
    assert_eq!(git(&directory, &["status", "--porcelain"]), "");
    assert_eq!(git(&directory, &["log", "--format=%s"]).lines().count(), 1);
    assert!(directory.parent().unwrap().join("go.mod").is_file());
    assert!(summary.contains("example/orm") && summary.contains("v0.1.0"), "{summary}");
}

/// A branch is checked out as that branch; `fetch-depth: 0` carries every
/// branch and tag.
#[test]
fn another_repository_is_checked_out_at_a_branch() {
    let fixture = fixture();
    let (directory, _) = other(&fixture, Some("side"), 0, "branch").unwrap();
    assert_eq!(git(&directory, &["symbolic-ref", "HEAD"]), "refs/heads/side");
    assert_eq!(git(&directory, &["rev-parse", "HEAD"]), git(&fixture.project, &["rev-parse", "side"]));
    assert!(git(&directory, &["tag"]).contains("v0.1.0"));
}

/// A commit SHA is checked out detached; no `ref` is the bundle's `HEAD`.
#[test]
fn another_repository_is_checked_out_at_a_commit_or_its_head() {
    let fixture = fixture();
    let first = git(&fixture.project, &["rev-list", "--max-parents=0", "HEAD"]);
    let (directory, _) = other(&fixture, Some(&first), 1, "commit").unwrap();
    assert_eq!(git(&directory, &["rev-parse", "HEAD"]), first);
    let (directory, _) = other(&fixture, None, 1, "head").unwrap();
    assert_eq!(git(&directory, &["rev-parse", "HEAD"]), git(&fixture.project, &["rev-parse", "HEAD"]));
}

/// An annotated tag is checked out at the commit it tags.
#[test]
fn another_repository_is_checked_out_at_an_annotated_tag() {
    let fixture = fixture();
    git(&fixture.project, &["tag", "-a", "v0.1.1", "-m", "note", "side"]);
    let (directory, _) = other(&fixture, Some("v0.1.1"), 1, "annotated").unwrap();
    assert_eq!(git(&directory, &["rev-parse", "HEAD"]), git(&fixture.project, &["rev-parse", "side"]));
}

/// A ref the repository does not have fails the step and names both.
#[test]
fn an_unknown_ref_of_another_repository_fails_the_step() {
    let fixture = fixture();
    let error = other(&fixture, Some("v9.9.9"), 1, "unknown").unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("example/orm") && message.contains("v9.9.9"), "{message}");
}

/// Another repository goes into an empty or absent directory, as the
/// project's own checkout does.
#[test]
fn another_repository_is_not_checked_out_over_files() {
    let fixture = fixture();
    let directory = fixture.base.join("occupied").join("workspace").join("core");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("left.txt"), "left").unwrap();
    let error = other(&fixture, Some("v0.1.0"), 1, "occupied").unwrap_err();
    assert!(format!("{error:#}").contains("비어 있지 않아요"), "{error:#}");
}
