//! `actions/upload-pages-artifact` and `actions/deploy-pages`: the site is
//! kept as it was uploaded, and the deployment is a recorded dry run.

use build_machine_worker::pages::{deploy, upload};
use std::path::Path;

fn site(workspace: &Path) {
    std::fs::create_dir_all(workspace.join("site/nested")).unwrap();
    std::fs::create_dir_all(workspace.join("site/.git")).unwrap();
    std::fs::write(workspace.join("site/index.json"), "{}").unwrap();
    std::fs::write(workspace.join("site/nested/a.txt"), "abc").unwrap();
    std::fs::write(workspace.join("site/.git/HEAD"), "ref").unwrap();
}

/// The deployment lists the files as they were uploaded, with their sizes and
/// SHA-256, without `.git`, as the action's archive leaves it out.
#[test]
fn the_deployment_records_the_uploaded_files() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let store = directory.path().join("pages");
    site(&workspace);
    upload(&workspace, "site", &store, "github-pages").unwrap();
    std::fs::write(workspace.join("site/index.json"), "changed after the upload").unwrap();
    let files = deploy(&store, "github-pages").unwrap();
    let listed: Vec<(&str, u64, &str)> = files.iter().map(|file| (file.path.as_str(), file.size, file.sha256.as_str())).collect();
    assert_eq!(
        listed,
        [
            ("index.json", 2, "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"),
            ("nested/a.txt", 3, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        ]
    );
}

/// deploy-pages deploys the artifact an earlier step uploaded; without one
/// the step fails and names it.
#[test]
fn a_deployment_without_an_uploaded_artifact_fails() {
    let directory = tempfile::tempdir().unwrap();
    let error = deploy(&directory.path().join("pages"), "github-pages").unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("github-pages") && message.contains("업로드"), "{message}");
}

/// An artifact name is uploaded once in a run, and the uploaded path has to
/// exist.
#[test]
fn an_upload_of_a_missing_path_or_a_second_upload_fails() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let store = directory.path().join("pages");
    std::fs::create_dir_all(&workspace).unwrap();
    let error = upload(&workspace, "site", &store, "github-pages").unwrap_err();
    assert!(format!("{error:#}").contains("site"), "{error:#}");
    site(&workspace);
    upload(&workspace, "site", &store, "github-pages").unwrap();
    let error = upload(&workspace, "site", &store, "github-pages").unwrap_err();
    assert!(format!("{error:#}").contains("이미"), "{error:#}");
}
