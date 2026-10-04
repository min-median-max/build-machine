//! Which directory a development build drives the machine from.

use build_machine_controller::development_root;

/// Tauri copies the bundle's resources, `machine.json` and `workers/`, into the
/// target directory beside the executables. That copy is not the controller
/// root: the Parallels share is published for the workspace, and a root of
/// `target/debug` failed with "The named build-machine share already belongs
/// to another directory".
#[test]
fn a_development_build_drives_the_workspace_that_compiled_it() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("build-machine");
    let debug = workspace.join("target").join("debug");
    std::fs::create_dir_all(debug.join("workers")).unwrap();
    std::fs::write(workspace.join("machine.json"), "{}").unwrap();
    std::fs::write(debug.join("machine.json"), "{}").unwrap();
    let executable = debug.join("build-machine");
    std::fs::write(&executable, "").unwrap();

    assert_eq!(development_root(&executable, &workspace), Some(workspace.canonicalize().unwrap()));
}

/// A binary outside the workspace that compiled it — a released application,
/// or a copy — is not a development build of it and has no declared root.
#[test]
fn a_binary_outside_its_workspace_has_no_development_root() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("build-machine");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("machine.json"), "{}").unwrap();
    let elsewhere = directory.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("machine.json"), "{}").unwrap();
    let executable = elsewhere.join("build-machine");
    std::fs::write(&executable, "").unwrap();

    assert_eq!(development_root(&executable, &workspace), None);
}
