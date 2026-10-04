//! `actions/upload-artifact` and `actions/download-artifact`: an artifact is
//! kept by the run that uploaded it, and a later step or a later replay
//! downloads it by name and run id.

use build_machine_worker::artifacts::{download, upload};
use std::path::Path;

fn tree(root: &Path) {
    std::fs::create_dir_all(root.join("validation/nested")).unwrap();
    std::fs::write(root.join("validation/pull.json"), "{\"number\": 7}").unwrap();
    std::fs::write(root.join("validation/nested/a.txt"), "abc").unwrap();
    std::fs::write(root.join("single.txt"), "one").unwrap();
}

#[test]
fn an_uploaded_artifact_is_downloaded_by_a_later_run() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let store = directory.path().join("artifacts");
    tree(&workspace);
    assert_eq!(upload(&workspace, "validation", &store.join("run-1"), "validation").unwrap(), 2);
    // The artifact holds the files as they were when the step ran.
    std::fs::write(workspace.join("validation/pull.json"), "changed").unwrap();
    let later = directory.path().join("later");
    assert_eq!(download(&store.join("run-1"), "validation", &later).unwrap(), 2);
    assert_eq!(std::fs::read_to_string(later.join("pull.json")).unwrap(), "{\"number\": 7}");
    assert_eq!(std::fs::read_to_string(later.join("nested/a.txt")).unwrap(), "abc");
}

/// Several paths keep their places under their least common ancestor, as
/// the action lays them out.
#[test]
fn a_file_and_several_paths_are_uploaded() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let store = directory.path().join("artifacts");
    tree(&workspace);
    assert_eq!(upload(&workspace, "single.txt\nvalidation/nested", &store, "both").unwrap(), 2);
    let out = directory.path().join("out");
    download(&store, "both", &out).unwrap();
    assert_eq!(std::fs::read_to_string(out.join("single.txt")).unwrap(), "one");
    assert_eq!(std::fs::read_to_string(out.join("validation/nested/a.txt")).unwrap(), "abc");
}

#[test]
fn a_missing_artifact_a_repeated_name_and_an_unsupported_path_fail() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let store = directory.path().join("artifacts");
    tree(&workspace);
    let error = download(&store, "validation", &directory.path().join("out")).unwrap_err();
    assert!(format!("{error:#}").contains("validation"), "{error:#}");
    upload(&workspace, "validation", &store, "validation").unwrap();
    let error = upload(&workspace, "validation", &store, "validation").unwrap_err();
    assert!(format!("{error:#}").contains("이미"), "{error:#}");
    for path in ["dist/*", "../outside", "/absolute"] {
        let error = upload(&workspace, path, &store, "other").unwrap_err();
        assert!(format!("{error:#}").contains(path), "{path}: {error:#}");
    }
    // No file is a warning on GitHub, and no artifact is kept.
    assert_eq!(upload(&workspace, "absent", &store, "absent").unwrap(), 0);
    assert!(!store.join("absent").exists());
}
