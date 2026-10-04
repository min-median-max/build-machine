//! Replaying the supported part of a repository workflow.
//!
//! Every external GitHub service is replaced by a local adapter and recorded as
//! a limit. A local success never establishes production signing, notarization
//! or release publication.

use crate::actions;
use crate::build::{collect_artifacts, development_bundle, extract_source, project_root, workflow_shell};
use crate::runner::Runner;
use anyhow::Context;
use crate::provision::Tools;
use crate::stream;
use anyhow::{bail, Result};
use build_machine_core::report::{Artifact, Outcome, PlatformResult, Stage, Step as ReportStep};
use build_machine_core::request::WorkRequest;
use build_machine_core::workflow::{Adapter, Condition, Step, STAGE_ORDER};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

const SEEDED_LIMIT: &str = "Local CI never signs, notarizes or uploads to GitHub.";

fn timeout_for(stage: &str) -> Duration {
    Duration::from_secs(match stage {
        "test" => 1800,
        "build" => 3600,
        "smoke" => 600,
        _ => 900,
    })
}

/// A secret's value is never available locally, so it is replaced by an empty
/// value and the substitution is recorded rather than hidden.
fn step_environment(base: &[(String, String)], step: &Step) -> (Vec<(String, String)>, Vec<String>) {
    let mut environment = base.to_vec();
    let mut limits = Vec::new();
    let merged: BTreeMap<&String, &String> = step.job_env.iter().chain(step.env.iter()).collect();
    for (key, value) in merged {
        let replacement = if value.contains("secrets.") || value.contains("github.token") || key.contains("GITHUB_TOKEN")
        {
            limits.push("GitHub secret values were replaced by an empty local adapter value.".to_owned());
            String::new()
        } else if value.contains("${{") {
            limits.push(format!("Expression for {key} is not available in the local runner."));
            String::new()
        } else {
            value.to_string()
        };
        environment.retain(|(existing, _)| existing != key);
        environment.push((key.clone(), replacement));
    }
    (environment, limits)
}

fn working_directory(source: &Path, step: &Step) -> Result<std::path::PathBuf> {
    let relative = step.working_directory.as_deref().unwrap_or(".");
    crate::build::contained_in(source, &source.join(relative))
        .context("Workflow working-directory must be inside the source snapshot.")
}

fn tauri_action(
    step: &Step,
    source: &Path,
    request: &WorkRequest,
    environment: &[(String, String)],
) -> Result<Vec<Artifact>> {
    let (manager, install, base) = if source.join("pnpm-lock.yaml").is_file() {
        ("pnpm", vec!["install".to_owned(), "--frozen-lockfile".to_owned()], vec!["exec".to_owned(), "tauri".to_owned(), "build".to_owned()])
    } else if source.join("package-lock.json").is_file() {
        ("npm", vec!["ci".to_owned()], vec!["exec".to_owned(), "--".to_owned(), "tauri".to_owned(), "build".to_owned()])
    } else {
        bail!("The Tauri action requires a pnpm or npm lockfile.");
    };
    let program = if cfg!(windows) { format!("{manager}.cmd") } else { manager.to_owned() };
    let working = working_directory(source, step)?;
    stream::checked(&program, &install, Some(&working), environment)?;
    // The workflow's own `args` is the author's intent. The machine supplies a
    // target or a bundle only where the workflow named none.
    let extra: Vec<String> =
        step.with.get("args").map(|args| args.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
    let mut arguments = base;
    arguments.extend(["--ci".to_owned(), "--no-sign".to_owned()]);
    if !extra.iter().any(|value| value == "--target") {
        arguments.extend(["--target".to_owned(), request.target.clone()]);
    }
    if !extra.iter().any(|value| value == "--bundles" || value == "--no-bundle") {
        match request.bundle.clone() {
            Some(bundle) => arguments.extend(["--bundles".to_owned(), bundle]),
            None => arguments.extend(development_bundle()),
        }
    }
    arguments.extend(extra);
    arguments.extend(["--".to_owned(), "--locked".to_owned()]);
    stream::checked(&program, &arguments, Some(source), environment)?;
    let output =
        source.join("src-tauri").join("target").join(&request.target).join("release").join("bundle");
    let mut artifacts = Vec::new();
    for extension in ["dmg", "deb", "exe", "msi"] {
        artifacts.extend(collect_artifacts(&output, extension)?);
    }
    Ok(artifacts)
}

/// The machine's environment for a replay, without signing credentials and
/// without the machine's own Rust pin: a workflow selects its toolchain itself,
/// through `rust-toolchain.toml` or rustup, as it does on a runner.
fn replay_environment(tools: &Tools) -> Vec<(String, String)> {
    tools
        .environment
        .iter()
        .filter(|(key, _)| {
            !key.starts_with("APPLE_")
                && !matches!(
                    key.as_str(),
                    "TAURI_SIGNING_PRIVATE_KEY" | "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" | "GITHUB_TOKEN" | "RUSTUP_TOOLCHAIN"
                )
        })
        .cloned()
        .collect()
}

/// Every step of the request in workflow order, with the stage it reports under.
fn ordered_steps(request: &WorkRequest) -> Vec<(&'static str, &Step)> {
    let mut steps: Vec<(&'static str, &Step)> = STAGE_ORDER
        .iter()
        .flat_map(|stage| request.stages.get(*stage).into_iter().flatten().map(move |step| (*stage, step)))
        .collect();
    steps.sort_by_key(|(_, step)| step.position);
    steps
}

/// Read one file of the source archive without extracting it.
#[cfg(target_os = "linux")]
fn archive_reader(archive: &Path) -> impl Fn(&str) -> Result<String> + '_ {
    move |relative: &str| {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        let relative = relative.trim_start_matches("./");
        let mut entry = zip.by_name(relative).with_context(|| format!("소스에 {relative}가 없어요."))?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text)?;
        Ok(text)
    }
}

/// What a replay needs installed system-wide, before it runs as the desktop
/// user: the machine's own prerequisites and the packages a `setup-php` step
/// declares. Runs elevated.
pub fn prepare_system(request: &WorkRequest, tools: &Tools) -> Result<()> {
    tools.setup_system()?;
    let archive = Path::new(&request.archive);
    if build_machine_core::source::sha256_file(archive)? != request.snapshot.source_hash {
        bail!("Source snapshot checksum mismatch.");
    }
    for (_, step) in ordered_steps(request) {
        if step.adapter != Adapter::PhpSetup {
            continue;
        }
        #[cfg(target_os = "linux")]
        crate::actions::setup_php_system(step, &archive_reader(archive), tools)
            .with_context(|| format!("step {} ({})", step.index, step.name))?;
        #[cfg(not(target_os = "linux"))]
        bail!("setup-php 어댑터는 아직 Linux 작업자에서만 설치해요: step {} ({})", step.index, step.name);
    }
    Ok(())
}

/// A step's own result, before it is placed in its stage.
struct Ran {
    status: Outcome,
    command: Option<String>,
    exit_code: Option<i32>,
    output: Option<String>,
    reason: Option<String>,
    timeout_seconds: Option<u64>,
    /// The step did not run. A step skipped for a secret condition is also a
    /// recorded limit, so its status alone does not say it did not run.
    skipped: bool,
}

impl Ran {
    fn new(status: Outcome) -> Ran {
        Ran {
            status,
            command: None,
            exit_code: None,
            output: None,
            reason: None,
            timeout_seconds: None,
            skipped: status == Outcome::Skipped,
        }
    }
}

fn worst(left: Outcome, right: Outcome) -> Outcome {
    let rank = |outcome: Outcome| match outcome {
        Outcome::Skipped => 0,
        Outcome::Passed => 1,
        Outcome::PassedWithLimits => 2,
        Outcome::Failed => 3,
        Outcome::Timeout => 4,
    };
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

#[allow(clippy::too_many_arguments)]
fn execute(
    stage_name: &str,
    step: &Step,
    source: &Path,
    request: &WorkRequest,
    base: &[(String, String)],
    tools: &Tools,
    runner: &mut Runner,
    result: &mut PlatformResult,
) -> Result<Ran> {
    let files = runner.begin_step(step.position)?;
    let (environment, limits) = step_environment(&runner.environment(base, &files), step);
    result.limits.extend(limits);
    let ran = match step.adapter {
        Adapter::Skip => {
            result.limits.push(format!("{stage_name} skipped: {}", step.reason.clone().unwrap_or_default()));
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.reason = step.reason.clone();
            ran
        }
        Adapter::NodeSetup | Adapter::GoSetup | Adapter::PhpSetup => {
            let installed = match step.adapter {
                Adapter::NodeSetup => actions::setup_node(step, source, tools, runner)?,
                Adapter::GoSetup => actions::setup_go(step, source, tools, runner)?,
                _ => actions::setup_php(step, source, tools, runner)?,
            };
            println!("{}", installed.summary);
            let mut ran = Ran::new(if installed.limits.is_empty() { Outcome::Passed } else { Outcome::PassedWithLimits });
            ran.output = Some(installed.summary);
            result.limits.extend(installed.limits);
            ran
        }
        Adapter::Checkout => {
            let mut ran = Ran::new(Outcome::Passed);
            if step.with.get("fetch-depth").is_some_and(|depth| depth.trim() != "1") {
                let limit = "actions/checkout fetch-depth asks for Git history; the source snapshot carries the files of one revision and no .git.".to_owned();
                result.limits.push(limit.clone());
                ran.status = Outcome::PassedWithLimits;
                ran.reason = Some(limit);
            }
            ran
        }
        adapter if adapter.is_local_stand_in() => Ran::new(Outcome::Passed),
        adapter if adapter.is_external_service() => {
            result.limits.push(format!("{}: external GitHub service replaced by local artifact store", step.name));
            Ran::new(Outcome::PassedWithLimits)
        }
        Adapter::TauriBuild => {
            let artifacts = tauri_action(step, source, request, &environment)?;
            result.artifacts.extend(artifacts);
            Ran::new(Outcome::Passed)
        }
        Adapter::Run => {
            let command = step.run.clone().unwrap_or_default();
            if command.trim().is_empty() {
                bail!("Workflow step {} has an empty run command.", step.index);
            }
            let working = working_directory(source, step)?;
            let (shell, arguments) = workflow_shell(&command);
            println!("> {command}");
            let timeout = timeout_for(stage_name);
            let finished = stream::run(shell, &arguments, Some(&working), &environment, Some(timeout))?;
            let mut ran = Ran::new(Outcome::Passed);
            ran.command = Some(command);
            ran.output = Some(finished.output.clone());
            if finished.timed_out {
                ran.status = Outcome::Timeout;
                ran.timeout_seconds = Some(timeout.as_secs());
            } else {
                ran.exit_code = Some(finished.code);
                if finished.code != 0 {
                    ran.status = Outcome::Failed;
                }
            }
            ran
        }
        other => bail!("Unsupported workflow adapter: {}", other.as_str()),
    };
    runner.finish_step(&files)?;
    Ok(ran)
}

pub fn replay(request: &WorkRequest, tools: &Tools) -> Result<PlatformResult> {
    tools.setup_system()?;
    tools.setup_user()?;
    let root = project_root(request)?;
    let signature = request.workflow_signature.clone().unwrap_or_else(|| request.snapshot.source_hash.clone());
    let directory = root.join(format!("ci-{}", &signature[..signature.len().min(24)]));
    let source = extract_source(Path::new(&request.archive), &request.snapshot.source_hash, &directory)?;
    let base = replay_environment(tools);

    let mut result = PlatformResult::passed(build_machine_core::now(), String::new());
    result.limits.push(SEEDED_LIMIT.to_owned());
    result.signing = Some("unverified".to_owned());

    let diagnosis = tools.doctor()?;
    if !diagnosis.ready {
        result.status = Outcome::Failed;
        result.success = false;
        result.error = Some(format!("Native tool diagnosis failed: {}", diagnosis.missing.join(", ")));
        result.finished_at = build_machine_core::now();
        return Ok(result);
    }

    let mut runner = Runner::new(&directory.join("runner"), &source, tools.platform)?;
    // Steps run in workflow order, as a runner runs them. The stage is how a
    // step is reported, never when it runs: `make test-servers` has to start
    // the servers before the step that reads their environment file.
    let mut stages: BTreeMap<&'static str, Stage> = BTreeMap::new();
    let mut failure: Option<String> = None;
    for (stage_name, step) in ordered_steps(request) {
        let started_at = build_machine_core::now();
        let condition = Condition::parse(step.condition.as_deref())?;
        let ran = if condition == Condition::Secret {
            result.limits.push("A GitHub secret or token condition is false in the local runner.".to_owned());
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.reason = Some("if condition depends on a secret, which is empty locally".to_owned());
            ran.skipped = true;
            ran
        } else if !condition.runs(failure.is_some()) {
            let mut ran = Ran::new(Outcome::Skipped);
            ran.reason = Some(if failure.is_some() {
                "not run: an earlier step failed and this step's if does not run after a failure".to_owned()
            } else {
                "if condition evaluated false locally".to_owned()
            });
            ran
        } else {
            match execute(stage_name, step, &source, request, &base, tools, &mut runner, &mut result) {
                Ok(ran) => ran,
                // An adapter that cannot do its work fails its step, and the
                // report says why, rather than ending the replay without one.
                Err(error) => {
                    let mut ran = Ran::new(Outcome::Failed);
                    ran.output = Some(format!("{error:#}"));
                    eprintln!("ERROR: step {} ({}): {error:#}", step.index, step.name);
                    ran
                }
            }
        };
        if matches!(ran.status, Outcome::Failed | Outcome::Timeout) && failure.is_none() {
            failure = Some(format!("step {} ({}) {}", step.index, step.name, match (ran.status, ran.exit_code) {
                (Outcome::Timeout, _) => "timed out".to_owned(),
                (_, Some(code)) => format!("exited with {code}"),
                _ => "failed".to_owned(),
            }));
        }
        let stage = stages.entry(stage_name).or_insert_with(|| Stage {
            status: Outcome::Skipped,
            started_at: started_at.clone(),
            finished_at: None,
            error: None,
            steps: Vec::new(),
        });
        stage.status = worst(stage.status, ran.status);
        if matches!(ran.status, Outcome::Failed | Outcome::Timeout) && stage.error.is_none() {
            stage.error = Some(format!("step {} ({}) {}", step.index, step.name, if ran.status == Outcome::Timeout { "timed out" } else { "failed" }));
        }
        stage.finished_at = Some(build_machine_core::now());
        stage.steps.push(ReportStep {
            index: step.index,
            name: step.name.clone(),
            adapter: step.adapter.as_str().to_owned(),
            status: ran.status,
            started_at,
            finished_at: Some(build_machine_core::now()),
            command: ran.command,
            exit_code: ran.exit_code,
            output: ran.output,
            reason: ran.reason.or_else(|| step.reason.clone()),
            skipped: ran.skipped,
            local_adapter: step.adapter.is_local_stand_in(),
            timeout_seconds: ran.timeout_seconds,
        });
    }
    for (name, stage) in stages {
        result.stages.insert(name.to_owned(), stage);
    }
    result.limits.sort();
    result.limits.dedup();
    result.finished_at = build_machine_core::now();
    if let Some(error) = failure {
        result.status = Outcome::Failed;
        result.success = false;
        result.error = Some(error);
        return Ok(result);
    }
    crate::workspace::prune(&root, &directory, &tools.machine.retention)?;
    // The seeded limit is always present, so a replay is never plain `passed`.
    result.status = Outcome::PassedWithLimits;
    result.success = true;
    Ok(result)
}
