//! Driving the selected environments and recording one run.
//!
//! A multi-platform operation captures one source snapshot, waits for every
//! selected platform, records each result as it arrives and fails overall if
//! any platform fails.

use crate::oplog::OperationLog;
use crate::snapshot;
use crate::transport::{Local, Parallels, Transport};
use crate::{state_directory, Operation};
use anyhow::{bail, Context, Result};
use build_machine_core::report::{
    write_atomic, Action, ExecutionMode, Outcome, PlatformResult, RunReport, RunStatus,
};
use build_machine_core::request::WorkRequest;
use build_machine_core::source::Snapshot;
use build_machine_core::{now, Platform};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const REPORT_BEGIN: &str = "BUILD_MACHINE_REPORT_BEGIN";
const REPORT_END: &str = "BUILD_MACHINE_REPORT_END";

/// Where the worker binaries live beside the controller.
fn worker_binary(root: &Path, platform: Platform) -> PathBuf {
    let name = match platform {
        Platform::Windows => "build-machine-worker-windows.exe",
        Platform::Linux => "build-machine-worker-linux",
        Platform::Macos => "build-machine-worker-macos",
    };
    root.join("workers").join(name)
}

fn transport(operation: &Operation, platform: Platform) -> Result<Box<dyn Transport>> {
    let worker = worker_binary(&operation.root, platform);
    Ok(match platform {
        Platform::Macos => Box::new(Local { root: operation.root.clone(), worker }),
        other => Box::new(Parallels::new(operation.root.clone(), &operation.machine, other, worker)?),
    })
}

/// Write the request this platform's worker will read, with every path
/// translated into that worker's own namespace.
fn write_request(
    operation: &Operation,
    platform: Platform,
    snapshot: &Snapshot,
    stages: &BTreeMap<String, Vec<build_machine_core::workflow::Step>>,
    transport: &dyn Transport,
    state: &Path,
) -> Result<(PathBuf, String)> {
    let profile = operation.machine.profile(platform)?;
    let archive = if snapshot.archive.is_empty() {
        String::new()
    } else {
        transport.locate(Path::new(&snapshot.archive))?
    };
    let request = WorkRequest {
        snapshot: snapshot.clone(),
        archive,
        target: profile.target.clone(),
        bundle: profile.bundle.clone(),
        framework: snapshot.framework.as_deref().and_then(|value| value.parse().ok()),
        command: snapshot.command.clone(),
        artifact: snapshot.artifact.clone(),
        stages: stages.clone(),
        workflow_signature: snapshot.workflow_path.as_ref().map(|_| snapshot.source_hash.clone()),
    };
    let path = state
        .join("projects")
        .join(&snapshot.project_key)
        .join(format!("{platform}-request.json"));
    request.write(&path)?;
    let located = transport.locate(&path)?;
    Ok((path, located))
}

fn worker_arguments(operation: &Operation, request: Option<&str>) -> Vec<String> {
    let mut arguments = match operation.action {
        Action::Doctor => vec!["doctor".to_owned()],
        Action::Setup => vec!["setup".to_owned()],
        Action::Build => vec!["build".to_owned()],
        Action::Release => vec!["release".to_owned()],
        Action::Run => vec!["run".to_owned()],
        Action::Ci => vec!["ci".to_owned()],
    };
    if let Some(request) = request {
        arguments.push("--request".to_owned());
        arguments.push(request.to_owned());
    }
    if operation.launch && operation.action == Action::Build {
        arguments.push("--run".to_owned());
    }
    arguments
}

/// Pull the structured platform result out of the worker's own output, so it
/// never has to write a file back onto a read-only share.
fn parse_report(output: &str) -> Result<PlatformResult> {
    let start = output.find(REPORT_BEGIN).context("워커가 구조화된 결과를 내보내지 않았어요.")?;
    let rest = &output[start + REPORT_BEGIN.len()..];
    let end = rest.find(REPORT_END).context("워커 결과가 끝나지 않았어요.")?;
    Ok(serde_json::from_str(rest[..end].trim())?)
}

struct PlatformOutcome {
    result: PlatformResult,
    log: PathBuf,
}

fn run_platform(
    operation: &Operation,
    platform: Platform,
    snapshot: Option<&Snapshot>,
    stages: &BTreeMap<String, Vec<build_machine_core::workflow::Step>>,
    state: &Path,
    stamp: &str,
    operation_log: &OperationLog,
) -> PlatformOutcome {
    let platform_log = state.join("logs").join(format!("{stamp}-{platform}.log"));
    operation_log.note(&format!(
        "PLATFORM {platform} started; command output goes to {}",
        platform_log.display()
    ));
    let log = match operation_log.sibling(platform_log.clone()) {
        Ok(log) => log,
        Err(error) => {
            return PlatformOutcome {
                result: PlatformResult::failed(now(), platform_log.to_string_lossy().into_owned(), error.to_string()),
                log: platform_log,
            }
        }
    };
    let result = execute_platform(operation, platform, snapshot, stages, state, &log);
    let outcome = match result {
        Ok(mut result) => {
            result.log = platform_log.to_string_lossy().into_owned();
            result
        }
        Err(error) => {
            operation_log.failure(&format!("{error:#}"));
            PlatformResult::failed(now(), platform_log.to_string_lossy().into_owned(), format!("{error:#}"))
        }
    };
    operation_log.note(&format!(
        "PLATFORM {platform} {:?}{}",
        outcome.status,
        outcome.error.as_ref().map(|error| format!(" error={error}")).unwrap_or_default()
    ));
    PlatformOutcome { result: outcome, log: platform_log }
}

fn execute_platform(
    operation: &Operation,
    platform: Platform,
    snapshot: Option<&Snapshot>,
    stages: &BTreeMap<String, Vec<build_machine_core::workflow::Step>>,
    state: &Path,
    log: &OperationLog,
) -> Result<PlatformResult> {
    let transport = transport(operation, platform)?;
    transport.prepare()?;
    // System-wide prerequisites need rights the desktop user does not have, so
    // that step runs on its own before the work the desktop user must do.
    if matches!(operation.action, Action::Setup | Action::Build | Action::Release | Action::Ci)
        && platform != Platform::Macos
    {
        transport.invoke_elevated(&["setup-system".to_owned()], log)?;
    }
    let request = match snapshot {
        Some(snapshot) => Some(write_request(operation, platform, snapshot, stages, transport.as_ref(), state)?.1),
        None => None,
    };
    let arguments = worker_arguments(operation, request.as_deref());
    let output = transport.invoke(&arguments, log)?;
    if operation.action == Action::Ci {
        return parse_report(&output);
    }
    Ok(PlatformResult::passed(now(), String::new()))
}

pub fn execute(operation: &Operation) -> Result<RunReport> {
    let state = state_directory(&operation.root);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%6f").to_string();
    let log = OperationLog::with_observer(
        state.join("logs").join(format!("{stamp}-matrix.log")),
        operation.observer.clone(),
    )?;
    let report_path = state.join("runs").join(&stamp).join("report.json");

    let mut report = RunReport {
        run_id: stamp.clone(),
        action: operation.action,
        project: operation.project.as_ref().map(|path| path.to_string_lossy().into_owned()),
        platforms: operation.platforms.clone(),
        execution_mode: operation.execution,
        status: RunStatus::Running,
        started_at: now(),
        finished_at: None,
        source: None,
        results: BTreeMap::new(),
        log: log.path().to_string_lossy().into_owned(),
        error: None,
    };
    let publish = |report: &RunReport| -> Result<()> {
        write_atomic(&report_path, report)?;
        if let Some(result_file) = &operation.result_file {
            write_atomic(result_file, report)?;
        }
        Ok(())
    };
    publish(&report)?;
    log.note(&format!(
        "START run={stamp} action={} platforms={} execution={:?}",
        operation.action,
        operation.platforms.iter().map(|value| value.as_str()).collect::<Vec<_>>().join(","),
        operation.execution
    ));
    log.note(&format!("REPORT {}", report_path.display()));

    let prepared = prepare(operation, &state);
    let mut stages = BTreeMap::new();
    match prepared {
        Ok((snapshot, replay_stages)) => {
            if let Some(snapshot) = &snapshot {
                report.project = Some(snapshot.project.clone());
                log.note(&format!(
                    "SOURCE revision={} dirty={} hash={}",
                    snapshot.revision, snapshot.dirty, snapshot.source_hash
                ));
            }
            stages = replay_stages;
            report.source = snapshot;
        }
        Err(error) => {
            report.error = Some(format!("{error:#}"));
            log.failure(&format!("{error:#}"));
        }
    }
    publish(&report)?;

    if report.error.is_none() {
        let snapshot = report.source.clone();
        if operation.execution == ExecutionMode::Parallel && operation.platforms.len() > 1 {
            let collected = Arc::new(Mutex::new(Vec::new()));
            std::thread::scope(|scope| {
                for platform in &operation.platforms {
                    let collected = collected.clone();
                    let snapshot = snapshot.clone();
                    let stages = &stages;
                    let log = &log;
                    let state = &state;
                    let stamp = &stamp;
                    scope.spawn(move || {
                        let outcome =
                            run_platform(operation, *platform, snapshot.as_ref(), stages, state, stamp, log);
                        collected.lock().unwrap().push((*platform, outcome));
                    });
                }
            });
            let mut collected = collected.lock().unwrap();
            collected.sort_by_key(|(platform, _)| *platform);
            for (platform, outcome) in collected.drain(..) {
                report.results.insert(platform, outcome.result);
                let _ = outcome.log;
            }
            publish(&report)?;
        } else {
            for platform in &operation.platforms {
                let outcome =
                    run_platform(operation, *platform, snapshot.as_ref(), &stages, &state, &stamp, &log);
                report.results.insert(*platform, outcome.result);
                publish(&report)?;
            }
        }
    }

    report.settle(now());
    publish(&report)?;
    log.note(&format!("FINISH {:?}", report.status));
    if let Err(error) = build_machine_core::retention::apply(
        &state,
        &stamp,
        report.project.as_deref(),
        &operation.machine.retention,
    ) {
        // A retention failure must stay visible in the operation log but cannot
        // turn an already completed build into an invented build failure.
        log.note(&format!("WARNING: could not apply run retention policy: {error:#}"));
    }
    Ok(report)
}

type Prepared = (Option<Snapshot>, BTreeMap<String, Vec<build_machine_core::workflow::Step>>);

fn prepare(operation: &Operation, state: &Path) -> Result<Prepared> {
    match operation.action {
        Action::Build | Action::Release => Ok((Some(snapshot::for_build(operation, state)?), BTreeMap::new())),
        Action::Run => Ok((Some(snapshot::for_run(operation)?), BTreeMap::new())),
        Action::Ci => {
            let replay = snapshot::for_replay(operation, state)?;
            // Refuse a replay the workflow was never written to perform, before
            // any environment is touched.
            for platform in &operation.platforms {
                build_machine_core::workflow::check_platform(&replay.workflow, *platform)?;
            }
            let stages = build_machine_core::workflow::stages(&replay.workflow);
            Ok((Some(replay.snapshot), stages))
        }
        Action::Doctor | Action::Setup => Ok((None, BTreeMap::new())),
    }
}

/// Record the diagnosis and setup results the desktop application displays, and
/// drop them when the machine definition they were measured against changes.
pub fn record_tool_status(root: &Path, machine: &build_machine_core::config::Machine, report: &RunReport) -> Result<()> {
    if !matches!(report.action, Action::Doctor | Action::Setup) {
        return Ok(());
    }
    let path = state_directory(root).join("tool-status.json");
    let previous: Option<serde_json::Value> =
        std::fs::read(&path).ok().and_then(|data| serde_json::from_slice(&data).ok());
    let configuration = serde_json::to_value(machine)?;
    let mut results = previous
        .filter(|value| value.get("configuration") == Some(&configuration))
        .and_then(|value| value.get("results").cloned())
        .unwrap_or_else(|| serde_json::json!({}));
    for (platform, result) in &report.results {
        let mut entry = serde_json::to_value(result)?;
        entry["action"] = serde_json::json!(report.action.as_str());
        results[platform.as_str()] = entry;
    }
    write_atomic(&path, &serde_json::json!({ "configuration": configuration, "results": results }))?;
    Ok(())
}

/// Reject an outcome the report cannot describe rather than reporting success.
pub fn ensure_settled(report: &RunReport) -> Result<()> {
    if report.results.values().any(|result| result.status == Outcome::Timeout) {
        bail!("A workflow step timed out. Inspect the recorded step output.");
    }
    Ok(())
}
