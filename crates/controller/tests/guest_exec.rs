//! `prlctl exec` of Parallels 27 intermittently fails before the guest command
//! starts, exiting 255 with `PrlJob_GetRetCode: Invalid argument` or
//! `PrlJob_GetResult: Invalid argument`. Measured on 2026-10-04: 18 of 150
//! `prlctl exec … touch /tmp/pe/<n>` failed that way and none of the 18 files
//! existed afterwards, while all 132 others did. Only that failure is tried
//! again; every attempt is logged.

#![cfg(unix)]

use build_machine_controller::oplog::OperationLog;
use build_machine_controller::transport::{exec_not_started, exec_with_retry, GuestExecNotStarted, GUEST_EXEC_ATTEMPTS};
use std::process::Command;

const NOT_STARTED: &str = "PrlJob_GetRetCode: Invalid argument. An invalid argument was passed.";

#[test]
fn only_parallels_s_not_started_failure_is_recognised() {
    assert!(exec_not_started(Some(255), &format!("{NOT_STARTED}\n")).is_some());
    assert!(exec_not_started(Some(255), "PrlJob_GetResult: Invalid argument. An invalid argument was passed.\n").is_some());
    // Output of a command that ran means it started; another code is the command's own.
    assert!(exec_not_started(Some(255), &format!("worker output\n{NOT_STARTED}\n")).is_none());
    assert!(exec_not_started(Some(1), &format!("{NOT_STARTED}\n")).is_none());
    assert!(exec_not_started(Some(255), "").is_none());
}

fn script(body: &str) -> impl FnMut() -> anyhow::Result<Command> + use<'_> {
    move || {
        let mut command = Command::new("sh");
        command.args(["-c", body]);
        Ok(command)
    }
}

#[test]
fn a_guest_command_that_did_not_start_is_started_again_and_each_attempt_is_logged() {
    let directory = tempfile::tempdir().unwrap();
    let counter = directory.path().join("attempts");
    let log = OperationLog::create(directory.path().join("log")).unwrap();
    let body = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{0}'; if [ $n -lt 3 ]; then echo '{NOT_STARTED}' >&2; exit 255; fi; echo ran",
        counter.display()
    );
    let output = exec_with_retry(&mut script(&body), &log).unwrap();
    assert!(output.contains("ran"), "{output}");
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "3");
    let text = std::fs::read_to_string(log.path()).unwrap();
    assert_eq!(text.matches("did not start the guest command").count(), 2, "{text}");
}

#[test]
fn a_command_that_failed_after_it_started_is_not_run_again() {
    let directory = tempfile::tempdir().unwrap();
    let counter = directory.path().join("attempts");
    let log = OperationLog::create(directory.path().join("log")).unwrap();
    let body = format!("echo x >> '{}'; echo partial; exit 255", counter.display());
    assert!(exec_with_retry(&mut script(&body), &log).is_err());
    assert_eq!(std::fs::read_to_string(&counter).unwrap().lines().count(), 1);
}

#[test]
fn a_guest_that_never_starts_the_command_fails_with_that_cause() {
    let directory = tempfile::tempdir().unwrap();
    let log = OperationLog::create(directory.path().join("log")).unwrap();
    let body = format!("echo '{NOT_STARTED}' >&2; exit 255");
    let error = exec_with_retry(&mut script(&body), &log).unwrap_err();
    let cause = error.downcast_ref::<GuestExecNotStarted>().expect("the error names the Parallels failure");
    assert_eq!(cause.attempts, GUEST_EXEC_ATTEMPTS);
    assert!(format!("{error:#}").contains("PrlJob_GetRetCode"), "{error:#}");
}
