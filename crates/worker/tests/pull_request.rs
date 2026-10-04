//! `peter-evans/create-pull-request`: the replay pushes nothing and records
//! the pull request the action would open, with the changed files' sizes and
//! SHA-256.

use build_machine_worker::pull_request::proposed;
use std::path::Path;
use std::process::Command;

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(directory).args(arguments).status().unwrap();
    assert!(status.success(), "git {arguments:?}");
}

fn repository(root: &Path) {
    std::fs::create_dir_all(root.join("plugins")).unwrap();
    std::fs::write(root.join("plugins/old.json"), "{}").unwrap();
    std::fs::write(root.join("README.md"), "readme").unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "add", "."]);
    git(root, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "base"]);
}

#[test]
fn the_changed_files_under_add_paths_are_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    repository(root);
    std::fs::write(root.join("plugins/browser.json"), "abc").unwrap();
    std::fs::write(root.join("plugins/old.json"), "{\"changed\": true}").unwrap();
    std::fs::write(root.join("README.md"), "outside add-paths").unwrap();
    let files = proposed(root, Some("plugins")).unwrap();
    let listed: Vec<(&str, u64, &str)> = files.iter().map(|file| (file.path.as_str(), file.size, file.sha256.as_str())).collect();
    assert_eq!(
        listed,
        [
            ("plugins/browser.json", 3, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            ("plugins/old.json", 17, "fdddfdbd9da79f3af723d799f5c36b6112a58fc9fccd0781ae91aece09b5d591"),
        ]
    );
    let everything = proposed(root, None).unwrap();
    assert_eq!(everything.len(), 3);
}

#[test]
fn a_repository_without_changes_proposes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    repository(directory.path());
    assert!(proposed(directory.path(), None).unwrap().is_empty());
}
