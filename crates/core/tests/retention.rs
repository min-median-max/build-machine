use build_machine_core::config::Retention;
use build_machine_core::retention;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

struct State {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

fn state() -> State {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(".state");
    fs::create_dir_all(path.join("runs")).unwrap();
    fs::create_dir_all(path.join("logs")).unwrap();
    State { _directory: directory, path }
}

fn age(path: &Path, seconds: u64) {
    let when = SystemTime::now() - Duration::from_secs(seconds);
    let time = filetime(when);
    set_times(path, time);
}

fn filetime(when: SystemTime) -> Duration {
    when.duration_since(SystemTime::UNIX_EPOCH).unwrap()
}

#[cfg(unix)]
fn set_times(path: &Path, when: Duration) {
    use std::os::unix::ffi::OsStrExt;
    let seconds = when.as_secs() as i64;
    let times = [
        libc_timeval { tv_sec: seconds, tv_usec: 0 },
        libc_timeval { tv_sec: seconds, tv_usec: 0 },
    ];
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    unsafe {
        utimes(raw.as_ptr(), times.as_ptr());
    }
}

#[repr(C)]
struct libc_timeval {
    tv_sec: i64,
    tv_usec: i64,
}

extern "C" {
    fn utimes(path: *const std::ffi::c_char, times: *const libc_timeval) -> i32;
}

/// Create a recorded run in the layout the controller writes.
fn run(state: &State, name: &str, seconds: u64, project: &str, status: &str, payload: usize) {
    let directory = state.path.join("runs").join(name);
    fs::create_dir_all(&directory).unwrap();
    let report = directory.join("report.json");
    fs::write(
        &report,
        format!("{{\"runId\":\"{name}\",\"project\":\"{project}\",\"status\":\"{status}\"}}\n"),
    )
    .unwrap();
    let log = state.path.join("logs").join(format!("{name}-linux.log"));
    fs::write(&log, "x".repeat(payload)).unwrap();
    age(&report, seconds);
    age(&log, seconds);
    age(&directory, seconds);
}

fn names(state: &State) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(state.path.join("runs"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn policy(max_runs: u32, max_bytes: u64, days: i64) -> Retention {
    Retention { max_runs_per_project: max_runs, max_bytes, days }
}

#[test]
fn byte_cap_removes_the_oldest_run_not_the_newest() {
    let state = state();
    for (index, name) in ["old", "mid", "new"].iter().enumerate() {
        run(&state, name, 3000 - index as u64 * 1000, "/app", "success", 5000);
    }
    run(&state, "current", 0, "/app", "success", 10);
    retention::apply(&state.path, "current", Some("/app"), &policy(20, 12_000, 3650)).unwrap();
    assert_eq!(names(&state), ["current", "mid", "new"]);
}

#[test]
fn evicting_a_run_also_removes_its_logs() {
    let state = state();
    run(&state, "old", 3000, "/app", "success", 5000);
    run(&state, "current", 0, "/app", "success", 0);
    retention::apply(&state.path, "current", Some("/app"), &policy(20, 100, 3650)).unwrap();
    assert_eq!(names(&state), ["current"]);
    assert!(!state.path.join("logs/old-linux.log").exists());
}

#[test]
fn per_project_limit_keeps_the_newest_runs_of_each_project() {
    let state = state();
    for index in 0..4u64 {
        run(&state, &format!("a{index}"), 4000 - index * 100, "/first", "success", 0);
    }
    run(&state, "b0", 4000, "/second", "success", 0);
    retention::apply(&state.path, "current", Some("/first"), &policy(2, u64::MAX, 3650)).unwrap();
    // The current run occupies one of /first's two slots, so one older survives.
    assert_eq!(names(&state), ["a3", "b0"]);
}

#[test]
fn expiry_reclaims_an_abandoned_running_report() {
    let state = state();
    run(&state, "stale", 60 * 60 * 24 * 40, "/app", "running", 0);
    run(&state, "recent", 60, "/app", "running", 0);
    retention::apply(&state.path, "current", Some("/app"), &policy(20, u64::MAX, 30)).unwrap();
    assert_eq!(names(&state), ["recent"]);
}

#[test]
fn a_running_report_survives_the_count_and_byte_policies() {
    let state = state();
    run(&state, "busy", 100, "/app", "running", 5000);
    retention::apply(&state.path, "current", Some("/app"), &policy(1, 10, 3650)).unwrap();
    assert_eq!(names(&state), ["busy"]);
}

#[test]
fn the_current_run_is_never_removed() {
    let state = state();
    run(&state, "current", 0, "/app", "success", 9000);
    retention::apply(&state.path, "current", Some("/app"), &policy(1, 1, 1)).unwrap();
    assert_eq!(names(&state), ["current"]);
}

#[test]
fn expired_logs_with_no_run_are_removed_but_retained_runs_keep_theirs() {
    let state = state();
    run(&state, "kept", 60, "/app", "success", 0);
    let orphan = state.path.join("logs/20250101-090000-000000.log");
    fs::write(&orphan, "output from a run that is gone").unwrap();
    age(&orphan, 60 * 60 * 24 * 40);
    let recent = state.path.join("logs/20260101-090000-000000.log");
    fs::write(&recent, "recent output").unwrap();
    retention::apply(&state.path, "current", Some("/app"), &policy(20, u64::MAX, 30)).unwrap();
    assert!(!orphan.exists());
    assert!(recent.is_file());
    assert!(state.path.join("logs/kept-linux.log").is_file());
}
