//! A step ends when its process exits, as on a GitHub runner.

#![cfg(unix)]

use build_machine_worker::stream;
use std::time::{Duration, Instant};

/// `make test-servers` leaves a log reader running in the background that
/// still holds the step's stderr. GitHub's runner waits five seconds for the
/// output streams after the step's process exits and then completes the step;
/// waiting for the streams to close would hold the step until the servers
/// stop.
#[test]
fn a_background_process_holding_the_output_does_not_hold_the_step() {
    let started = Instant::now();
    let finished = stream::run(
        "bash",
        &["-e".to_owned(), "-c".to_owned(), "sleep 60 >/dev/null & echo started".to_owned()],
        None,
        &[("PATH".to_owned(), std::env::var("PATH").unwrap())],
        None,
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(finished.code, 0);
    assert!(finished.output.contains("started"), "{}", finished.output);
    assert!(elapsed >= stream::STREAM_GRACE, "{elapsed:?}");
    assert!(elapsed < stream::STREAM_GRACE + Duration::from_secs(5), "{elapsed:?}");
}

/// Output written before the process exits is never lost to that grace.
#[test]
fn a_step_without_background_processes_returns_at_once_with_all_its_output() {
    let started = Instant::now();
    let finished = stream::run(
        "bash",
        &["-c".to_owned(), "for i in 1 2 3; do echo line $i; echo error $i >&2; done".to_owned()],
        None,
        &[("PATH".to_owned(), std::env::var("PATH").unwrap())],
        None,
    )
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    for i in 1..=3 {
        assert!(finished.output.contains(&format!("line {i}")) && finished.output.contains(&format!("error {i}")));
    }
}
