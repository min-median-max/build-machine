// Ageing a file to test the byte cap uses `utimes`; the logic under test is
// platform-neutral, so exercising it here is enough.
#![cfg(unix)]

use build_machine_core::config::Retention;
use std::fs;
use std::path::Path;

use build_machine_worker::workspace;

fn workspace_dir(root: &Path, name: &str, bytes: usize, age: u64) -> std::path::PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.join("source")).unwrap();
    fs::write(path.join("source/payload.bin"), vec![b'x'; bytes]).unwrap();
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
    filetime(&path, when);
    path
}

fn filetime(path: &Path, when: std::time::SystemTime) {
    let seconds = when.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    #[repr(C)]
    struct Timeval {
        seconds: i64,
        microseconds: i64,
    }
    extern "C" {
        fn utimes(path: *const std::ffi::c_char, times: *const Timeval) -> i32;
    }
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [Timeval { seconds, microseconds: 0 }, Timeval { seconds, microseconds: 0 }];
    unsafe {
        utimes(raw.as_ptr(), times.as_ptr());
    }
}

fn names(root: &Path) -> Vec<String> {
    let mut found: Vec<String> =
        fs::read_dir(root).unwrap().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    found.sort();
    found
}

fn policy(max_bytes: u64) -> Retention {
    Retention { max_runs_per_project: 20, max_bytes, days: 30 }
}

/// A build directory is a whole dependency tree, so the byte cap is what keeps
/// the machine that does the work from filling its own disk.
#[test]
fn the_oldest_build_directories_go_first_once_over_the_cap() {
    let root = tempfile::tempdir().unwrap();
    workspace_dir(root.path(), "old", 5000, 3000);
    workspace_dir(root.path(), "mid", 5000, 2000);
    workspace_dir(root.path(), "new", 5000, 1000);
    let current = workspace_dir(root.path(), "current", 100, 0);
    workspace::prune(root.path(), &current, &policy(12_000)).unwrap();
    assert_eq!(names(root.path()), ["current", "mid", "new"]);
}

/// The current build is in use and the one the receipt names is what a launch
/// would run. Neither is ever removed, however tight the cap.
#[test]
fn the_current_build_and_the_one_the_receipt_names_are_kept() {
    let root = tempfile::tempdir().unwrap();
    let referenced = workspace_dir(root.path(), "referenced", 9000, 5000);
    workspace_dir(root.path(), "stale", 9000, 4000);
    let current = workspace_dir(root.path(), "current", 9000, 0);
    fs::write(
        root.path().join("latest.json"),
        serde_json::json!({ "executable": referenced.join("source/app").to_string_lossy() }).to_string(),
    )
    .unwrap();
    workspace::prune(root.path(), &current, &policy(1)).unwrap();
    assert_eq!(names(root.path()), ["current", "referenced"]);
}

#[test]
fn nothing_is_removed_while_the_total_fits() {
    let root = tempfile::tempdir().unwrap();
    workspace_dir(root.path(), "one", 1000, 2000);
    let current = workspace_dir(root.path(), "current", 1000, 0);
    workspace::prune(root.path(), &current, &policy(1_000_000)).unwrap();
    assert_eq!(names(root.path()), ["current", "one"]);
}
