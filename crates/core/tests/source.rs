use build_machine_core::source::{self, SourceMode};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Repo {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(root).args(arguments).status().unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

/// A project whose name needs escaping, so the key and the archive both have to
/// cope with it.
fn repo() -> Repo {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project with ' spaces");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "user.name", "test"]);
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    std::fs::write(root.join("value.txt"), "committed").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    Repo { _directory: directory, root }
}

fn archive(repo: &Repo, name: &str, reference: Option<&str>) -> (PathBuf, String, bool, usize, SourceMode) {
    let path = repo.root.parent().unwrap().join(name);
    let (_revision, dirty, hash, count, mode) =
        source::make_archive(&repo.root, &path, reference).unwrap();
    (path, hash, dirty, count, mode)
}

fn entries(path: &Path) -> Vec<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    (0..zip.len()).map(|index| zip.by_index(index).unwrap().name().to_owned()).collect()
}

#[test]
fn current_edits_and_untracked_files_are_included_but_ignored_data_is_not() {
    let repo = repo();
    std::fs::write(repo.root.join("value.txt"), "edited").unwrap();
    std::fs::write(repo.root.join("untracked.txt"), "new").unwrap();
    std::fs::create_dir(repo.root.join("ignored")).unwrap();
    std::fs::write(repo.root.join("ignored/secret.txt"), "must not travel").unwrap();
    let (path, _hash, dirty, _count, mode) = archive(&repo, "source.zip", None);
    let names = entries(&path);
    assert!(names.contains(&"untracked.txt".to_owned()));
    assert!(!names.iter().any(|name| name.contains("ignored")));
    assert!(dirty);
    assert_eq!(mode, SourceMode::Local);
}

#[test]
fn identical_contents_hash_the_same_and_an_edit_invalidates_it() {
    let repo = repo();
    let (_first, first_hash, _, _, _) = archive(&repo, "a.zip", None);
    let (_second, second_hash, _, _, _) = archive(&repo, "b.zip", None);
    assert_eq!(first_hash, second_hash, "the same tree must produce the same hash");
    std::fs::write(repo.root.join("value.txt"), "changed").unwrap();
    let (_third, third_hash, _, _, _) = archive(&repo, "c.zip", None);
    assert_ne!(first_hash, third_hash);
}

#[test]
fn a_ref_archive_is_clean_and_excludes_worktree_edits() {
    let repo = repo();
    std::fs::write(repo.root.join("value.txt"), "dirty").unwrap();
    let (path, _hash, dirty, _count, mode) = archive(&repo, "ref.zip", Some("HEAD"));
    assert!(!dirty);
    assert_eq!(mode, SourceMode::Ref);
    let file = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut content = String::new();
    use std::io::Read;
    zip.by_name("value.txt").unwrap().read_to_string(&mut content).unwrap();
    assert_eq!(content, "committed");
}

#[test]
fn a_tracked_deletion_is_not_restored_from_git() {
    let repo = repo();
    std::fs::remove_file(repo.root.join("value.txt")).unwrap();
    let (path, _hash, _, _, _) = archive(&repo, "deleted.zip", None);
    assert!(!entries(&path).contains(&"value.txt".to_owned()));
}

#[test]
fn a_symlink_cannot_export_files_outside_the_project() {
    let repo = repo();
    let outside = repo.root.parent().unwrap().join("outside.txt");
    std::fs::write(&outside, "must not travel").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, repo.root.join("link.txt")).unwrap();
    let path = repo.root.parent().unwrap().join("symlink.zip");
    let error = source::make_archive(&repo.root, &path, None).unwrap_err();
    assert!(format!("{error:#}").contains("outside the project"), "{error:#}");
}

#[test]
fn a_repository_subdirectory_is_not_a_project_root() {
    let repo = repo();
    let inner = repo.root.join("apps/web");
    std::fs::create_dir_all(&inner).unwrap();
    let error = source::repository_root(&inner).unwrap_err();
    assert!(format!("{error:#}").contains("Git 저장소 루트"), "{error:#}");
}

#[test]
fn projects_with_the_same_name_have_separate_keys() {
    let first = source::project_key(Path::new("/one/airdata"));
    let second = source::project_key(Path::new("/two/airdata"));
    assert_ne!(first, second);
    assert!(first.starts_with("airdata-"));
    assert!(second.starts_with("airdata-"));
}
