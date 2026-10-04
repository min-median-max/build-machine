//! Replaying the supported part of a repository workflow.
//!
//! Every external GitHub service is replaced by a local adapter and recorded as
//! a limit. A local success never establishes production signing, notarization
//! or release publication.

use crate::actions;
use crate::build::{collect_artifacts, development_bundle, project_root, workflow_shell};
use crate::runner::Runner;
use anyhow::Context;
use crate::provision::Tools;
use crate::stream;
use anyhow::{bail, Result};
use build_machine_core::report::{Artifact, Outcome, PlatformResult, Stage, Step as ReportStep};
use build_machine_core::request::WorkRequest;
use build_machine_core::workflow::{
    checkout_path, checkout_target, find_repository, github_context, names_secret, ref_name, stage_of, Adapter, CheckoutTarget, Condition,
    Github, Job, JobStatus, Step,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

const SEEDED_LIMIT: &str = "Local CI never signs, notarizes or uploads to GitHub.";

/// What a hosted runner starts each job with and a replay cannot: a fresh
/// machine image.
const MACHINE_LIMIT: &str = "Each job starts in an empty workspace, but on this machine: system packages, services and files outside the workspace persist across jobs and replays, where a hosted runner starts every job on a fresh image.";

/// Which limit bounds a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The step's own `timeout-minutes`. Exceeding it fails the step.
    Step,
    /// What is left of the job's `timeout-minutes`. Exceeding it cancels the job.
    Job,
}

/// How long a step may run: its own `timeout-minutes` when it declares one,
/// within what is left of its job's. GitHub sets no other limit on a step.
pub fn step_limit(step_minutes: Option<u64>, job_remaining: Duration) -> (Duration, Limit) {
    match step_minutes.map(|minutes| Duration::from_secs(minutes * 60)) {
        Some(own) if own < job_remaining => (own, Limit::Step),
        _ => (job_remaining, Limit::Job),
    }
}

/// A secret's value is never available locally, so it is replaced by an empty
/// value and the substitution is recorded rather than hidden.
fn step_environment(base: &[(String, String)], step: &Step) -> (Vec<(String, String)>, Vec<String>) {
    let mut environment = base.to_vec();
    let mut limits = Vec::new();
    let merged: BTreeMap<&String, &String> = step.job_env.iter().chain(step.env.iter()).collect();
    for (key, value) in merged {
        // Every other expression was resolved before the step, and validation
        // refused any that has no local value.
        let replacement = if names_secret(value) || key.contains("GITHUB_TOKEN") {
            limits.push("GitHub secret values were replaced by an empty local adapter value.".to_owned());
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

/// `actions/checkout` of another repository into `path` under the workspace,
/// from the history the controller bundled from its local clone.
fn checkout_other(
    target: &CheckoutTarget,
    workspace: &Path,
    request: &WorkRequest,
    depth: (u32, bool),
    environment: &[(String, String)],
) -> Result<String> {
    let (name, recorded) = find_repository(&request.snapshot.repositories, &target.repository)
        .with_context(|| format!("요청에 {}의 Git 기록이 없어요.", target.repository))?;
    let history = request.repositories.get(name).with_context(|| format!("요청에 {name}의 Git 기록 경로가 없어요."))?;
    let history = Path::new(history);
    if build_machine_core::source::sha256_file(history)? != recorded.sha256 {
        bail!("{name} Git history checksum mismatch.");
    }
    let directory = workspace.join(target.path.as_deref().unwrap_or("."));
    let mirror = project_root(request)?.join("repositories").join(format!("{name}.git"));
    crate::checkout::checkout_repository(
        &crate::checkout::RepositoryCheckout {
            repository: name,
            history,
            mirror: &mirror,
            directory: &directory,
            reference: target.reference.as_deref(),
            fetch_depth: depth.0,
            fetch_tags: depth.1,
        },
        environment,
    )
}

/// `actions/checkout` into the job's empty workspace, or of another
/// repository into its `path`.
fn checkout_step(step: &Step, workspace: &Path, request: &WorkRequest, environment: &[(String, String)]) -> Result<String> {
    let (fetch_depth, fetch_tags) = build_machine_core::workflow::checkout_inputs(&step.with)?;
    if let Some(target) = checkout_target(&step.with)? {
        return checkout_other(&target, workspace, request, (fetch_depth, fetch_tags), environment);
    }
    let history = request.history.as_deref().context("요청에 checkout할 Git 기록이 없어요.")?;
    let history = Path::new(history);
    if Some(build_machine_core::source::sha256_file(history)?) != request.snapshot.history_sha256 {
        bail!("Git history checksum mismatch.");
    }
    let mirror = project_root(request)?.join("history.git");
    let directory = workspace.join(checkout_path(&step.with)?.as_deref().unwrap_or("."));
    crate::checkout::checkout(
        &crate::checkout::Checkout {
            history,
            mirror: &mirror,
            workspace: &directory,
            archive: Path::new(&request.archive),
            revision: &request.snapshot.revision,
            reference: request.snapshot.checkout_ref.as_deref(),
            dirty: request.snapshot.dirty,
            fetch_depth,
            fetch_tags,
        },
        environment,
    )
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

/// Every step of the request in workflow order — jobs in `needs` order, each
/// job's steps as written — with the stage it reports under. The stage never
/// decides when a step runs.
pub fn ordered_steps(request: &WorkRequest) -> Vec<(&'static str, &Step)> {
    request.jobs.iter().flat_map(|job| job.steps.iter().map(|step| (stage_of(step), step))).collect()
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
    step: &Step,
    job: &Job,
    limit: Option<Duration>,
    source: &Path,
    request: &WorkRequest,
    base: &[(String, String)],
    tools: &Tools,
    runner: &mut Runner,
    result: &mut PlatformResult,
    pages: &Path,
) -> Result<Ran> {
    let files = runner.begin_step(step.position)?;
    let (environment, limits) = step_environment(&runner.environment(base, &files), step);
    result.limits.extend(limits);
    let ran = match step.adapter {
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
            let summary = checkout_step(step, source, request, &environment)?;
            println!("{summary}");
            match checkout_target(&step.with)? {
                Some(target) => result.limits.push(format!(
                    "actions/checkout of {} fetches the committed branches and tags of its local clone named in machine.json repositories, not from the GitHub remote; uncommitted changes of that clone are not checked out.",
                    target.repository
                )),
                None => {
                    result.limits.push(
                        "actions/checkout fetches from this repository's own branches and tags through a local mirror, not from the GitHub remote.".to_owned(),
                    );
                    if request.snapshot.dirty {
                        result.limits.push(format!(
                            "Uncommitted changes of the working tree are staged on {} in the checkout; GitHub checks out committed files only.",
                            request.snapshot.revision
                        ));
                    }
                }
            }
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.output = Some(summary);
            ran
        }
        adapter if adapter.is_local_stand_in() => Ran::new(Outcome::Passed),
        Adapter::PagesArtifact => {
            let name = step.with.get("name").map(String::as_str).unwrap_or("github-pages");
            let path = step.with.get("path").map(String::as_str).unwrap_or("_site/");
            let count = crate::pages::upload(source, path, pages, name)?;
            let summary = format!("Kept {count} files of {path} as the pages artifact {name} for this replay.");
            println!("{summary}");
            result.limits.push(format!("{}: the pages artifact {name} is kept for this replay and not uploaded to GitHub.", step.name));
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.output = Some(summary);
            ran
        }
        Adapter::DeployPages => {
            let name = step.with.get("artifact_name").map(String::as_str).unwrap_or("github-pages");
            let files = crate::pages::deploy(pages, name)?;
            let mut summary = format!("dry-run: not deployed. The pages artifact {name} holds {} files:", files.len());
            for file in &files {
                summary.push_str(&format!("\n{} {} {}", file.sha256, file.size, file.path));
            }
            println!("{summary}");
            result.limits.push(format!(
                "{}: dry-run: not deployed. The files of the pages artifact {name} are recorded in deployments.",
                step.name
            ));
            result.deployments.push(build_machine_core::report::Deployment {
                job: job.id.clone(),
                environment: job.environment.clone(),
                artifact: name.to_owned(),
                files,
            });
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.output = Some(summary);
            ran
        }
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
            let finished = stream::run(shell, &arguments, Some(&working), &environment, limit)?;
            let mut ran = Ran::new(Outcome::Passed);
            ran.command = Some(command);
            ran.output = Some(finished.output.clone());
            if finished.timed_out {
                ran.status = Outcome::Timeout;
                ran.timeout_seconds = limit.map(|limit| limit.as_secs());
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
    runner.finish_step(&files, step.id.as_deref())?;
    Ok(ran)
}

/// What a job's Ubuntu runner label asks for that this machine is not: another
/// Ubuntu release, or another architecture (`-arm` is ARM64, no suffix x64).
pub fn runner_image_limit(runs_on: &str, version_id: &str, arch: &str) -> Option<String> {
    let label = runs_on.to_ascii_lowercase();
    let rest = label.strip_prefix("ubuntu-")?;
    let (release, wants_arm) = match rest.strip_suffix("-arm") {
        Some(release) => (release, true),
        None => (rest, false),
    };
    let is_arm = matches!(arch, "aarch64" | "arm64");
    let mut differences = Vec::new();
    if release != version_id {
        differences.push(format!("Ubuntu {release}"));
    }
    if wants_arm != is_arm {
        differences.push(if wants_arm { "ARM64".to_owned() } else { "x86_64 (X64)".to_owned() });
    }
    (!differences.is_empty()).then(|| {
        format!(
            "The job runs on {runs_on}, which is {}; this machine is Ubuntu {version_id} {arch}.",
            differences.join(" and ")
        )
    })
}

/// The variables a runner gives every step of a job.
fn runner_context(request: &WorkRequest, job: &Job, tracking: &str) -> Vec<(&'static str, String)> {
    let mut context = vec![
        ("GITHUB_SHA", request.snapshot.revision.clone()),
        ("GITHUB_JOB", job.id.clone()),
        ("RUNNER_TRACKING_ID", tracking.to_owned()),
    ];
    if let Some(event) = &request.snapshot.event {
        context.push(("GITHUB_EVENT_NAME", event.clone()));
    }
    if let Some(reference) = &request.snapshot.checkout_ref {
        context.push(("GITHUB_REF", reference.clone()));
        let kind = if reference.starts_with("refs/heads/") { "branch" } else { "tag" };
        context.push(("GITHUB_REF_NAME", ref_name(reference).to_owned()));
        context.push(("GITHUB_REF_TYPE", kind.to_owned()));
    }
    context
}

/// Run one job's steps in order and return where the job stands at its end.
#[allow(clippy::too_many_arguments)]
fn run_job(
    job: &Job,
    mut runner: Runner,
    source: &Path,
    request: &WorkRequest,
    base: &[(String, String)],
    tools: &Tools,
    result: &mut PlatformResult,
    stages: &mut BTreeMap<&'static str, Stage>,
    failure: &mut Option<String>,
    github: &Github,
    pages: &Path,
) -> Result<JobStatus> {
    let job_limit = Duration::from_secs(job.timeout_minutes * 60);
    let started = Instant::now();
    let mut status = JobStatus::Success;
    for step in &job.steps {
        let started_at = build_machine_core::now();
        let condition = Condition::parse(step.condition.as_deref())?;
        let ran = if condition == Condition::Secret {
            result.limits.push("A GitHub secret or token condition is false in the local runner.".to_owned());
            let mut ran = Ran::new(Outcome::PassedWithLimits);
            ran.reason = Some("if condition depends on a secret, which is empty locally".to_owned());
            ran.skipped = true;
            ran
        } else if !condition.runs(status, runner.outputs(), github) {
            let mut ran = Ran::new(Outcome::Skipped);
            ran.reason = Some(match status {
                JobStatus::Success => "if condition evaluated false locally".to_owned(),
                JobStatus::Failure => {
                    "not run: an earlier step failed and this step's if does not run after a failure".to_owned()
                }
                JobStatus::Cancelled => {
                    "not run: the job was cancelled and this step's if does not run after a cancellation".to_owned()
                }
            });
            ran
        } else {
            // A cancelled job has no time left; a step that still runs after
            // the cancellation is bounded by its own limit only.
            let remaining = job_limit.saturating_sub(started.elapsed());
            let (limit, bound) = match (status, step.timeout_minutes) {
                (JobStatus::Cancelled, minutes) => (minutes.map(|minutes| Duration::from_secs(minutes * 60)), Limit::Step),
                _ => {
                    let (limit, bound) = step_limit(step.timeout_minutes, remaining);
                    (Some(limit), bound)
                }
            };
            // A step's expressions take the outputs of the steps before it,
            // just before it runs.
            let ran = step
                .resolve(runner.outputs(), github)
                .and_then(|resolved| {
                    execute(&resolved, job, limit, source, request, base, tools, &mut runner, result, pages)
                });
            let mut ran = match ran {
                Ok(ran) => ran,
                // An adapter that cannot do its work fails its step, and the
                // report says why, rather than ending the replay without one.
                Err(error) => {
                    let mut ran = Ran::new(Outcome::Failed);
                    ran.output = Some(format!("{error:#}"));
                    eprintln!("ERROR: step {} ({}): {error:#}", step.index, step.name);
                    ran
                }
            };
            if ran.status == Outcome::Timeout {
                ran.reason = Some(match bound {
                    Limit::Step => format!("the step exceeded its timeout-minutes of {}", step.timeout_minutes.unwrap_or_default()),
                    Limit::Job => format!(
                        "job {} exceeded its timeout-minutes of {}; GitHub cancels the job",
                        job.id, job.timeout_minutes
                    ),
                });
            }
            if status == JobStatus::Success || status == JobStatus::Failure {
                match (ran.status, bound) {
                    (Outcome::Timeout, Limit::Job) => status = JobStatus::Cancelled,
                    (Outcome::Failed | Outcome::Timeout, _) => status = JobStatus::Failure,
                    _ => {}
                }
            }
            ran
        };
        if matches!(ran.status, Outcome::Failed | Outcome::Timeout) && failure.is_none() {
            *failure = Some(format!("step {} ({}) {}", step.index, step.name, match (ran.status, ran.exit_code) {
                (Outcome::Timeout, _) => "timed out".to_owned(),
                (_, Some(code)) => format!("exited with {code}"),
                _ => "failed".to_owned(),
            }));
        }
        record(stages, step, ran, started_at);
    }
    Ok(status)
}

/// Place a step's result in the stage it reports under.
fn record(stages: &mut BTreeMap<&'static str, Stage>, step: &Step, ran: Ran, started_at: String) {
    record_in(stages, stage_of(step), step, ran, started_at)
}

fn record_in(stages: &mut BTreeMap<&'static str, Stage>, stage_name: &'static str, step: &Step, ran: Ran, started_at: String) {
    let stage = stages.entry(stage_name).or_insert_with(|| Stage {
        status: Outcome::Skipped,
        started_at: started_at.clone(),
        finished_at: None,
        error: None,
        steps: Vec::new(),
    });
    stage.status = worst(stage.status, ran.status);
    if matches!(ran.status, Outcome::Failed | Outcome::Timeout) && stage.error.is_none() {
        stage.error = Some(format!(
            "step {} ({}) {}",
            step.index,
            step.name,
            if ran.status == Outcome::Timeout { "timed out" } else { "failed" }
        ));
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

/// End a job on this machine: move the artifacts it produced out of `work`
/// into `keep`, under the same relative path, and remove `work`.
///
/// A job's workspace is a whole build — orm's Rust target alone is 7 GB — and
/// a hosted runner discards it with the machine. Only what the result names is
/// kept, and the result is updated to where it now is.
pub fn finish_job(work: &Path, keep: &Path, artifacts: &mut [Artifact]) -> Result<()> {
    for artifact in artifacts.iter_mut() {
        let path = Path::new(&artifact.path);
        let Ok(relative) = path.strip_prefix(work) else { continue };
        let destination = keep.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(path, &destination)
            .with_context(|| format!("산출물을 옮기지 못했어요: {}", path.display()))?;
        artifact.path = destination.to_string_lossy().into_owned();
    }
    if work.exists() {
        std::fs::remove_dir_all(work).with_context(|| format!("job workspace를 지우지 못했어요: {}", work.display()))?;
    }
    Ok(())
}

pub fn replay(request: &WorkRequest, tools: &Tools) -> Result<PlatformResult> {
    tools.setup_system()?;
    tools.setup_user()?;
    let root = project_root(request)?;
    let signature = request.workflow_signature.clone().unwrap_or_else(|| request.snapshot.source_hash.clone());
    let directory = root.join(format!("ci-{}", &signature[..signature.len().min(24)]));
    if build_machine_core::source::sha256_file(Path::new(&request.archive))? != request.snapshot.source_hash {
        bail!("Source snapshot checksum mismatch.");
    }
    // GITHUB_WORKSPACE is <work>/<repository>/<repository>, as on a runner.
    let repository = Path::new(&request.snapshot.project)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context("프로젝트 이름이 없어요.")?;
    let work = directory.join("work");
    let workspace = work.join(&repository).join(&repository);
    let base = replay_environment(tools);
    // The byte cap of machine.json retention holds before a replay adds to the
    // machine, not only after one succeeds.
    crate::workspace::prune(&root, &directory, &tools.machine.retention)?;

    let mut result = PlatformResult::passed(build_machine_core::now(), String::new());
    result.limits.push(SEEDED_LIMIT.to_owned());
    result.limits.push(MACHINE_LIMIT.to_owned());
    result.signing = Some("unverified".to_owned());

    let diagnosis = tools.doctor()?;
    if !diagnosis.ready {
        result.status = Outcome::Failed;
        result.success = false;
        result.error = Some(format!("Native tool diagnosis failed: {}", diagnosis.missing.join(", ")));
        result.finished_at = build_machine_core::now();
        return Ok(result);
    }

    // Steps run in workflow order, as a runner runs them. The stage is how a
    // step is reported, never when it runs: `make test-servers` has to start
    // the servers before the step that reads their environment file.
    let mut stages: BTreeMap<&'static str, Stage> = BTreeMap::new();
    let mut failure: Option<String> = None;
    let mut finished: BTreeMap<&str, JobStatus> = BTreeMap::new();
    let event = request.snapshot.event.as_deref().context("요청에 workflow event가 없어요.")?;
    let github =
        github_context(&request.jobs, event, &request.snapshot.revision, request.snapshot.checkout_ref.as_deref())?;
    // Pages artifacts live for one replay, as an artifact lives for one run.
    let pages = directory.join("pages");
    if pages.exists() {
        std::fs::remove_dir_all(&pages)
            .with_context(|| format!("이전 pages 산출물을 지우지 못했어요: {}", pages.display()))?;
    }
    for job in &request.jobs {
        // A job runs when every job it needs succeeded, as GitHub's default
        // job condition `success()` reads it.
        let blocked = job.needs.iter().find(|need| finished.get(need.as_str()) != Some(&JobStatus::Success));
        if let Some(need) = blocked {
            let reason = format!("job {} did not run: the job it needs, {need}, did not succeed", job.id);
            for step in &job.steps {
                let mut ran = Ran::new(Outcome::Skipped);
                ran.reason = Some(reason.clone());
                record(&mut stages, step, ran, build_machine_core::now());
            }
            continue;
        }
        #[cfg(target_os = "linux")]
        if let Some(version_id) = std::fs::read_to_string("/etc/os-release").ok().and_then(|text| {
            text.lines().find_map(|line| line.strip_prefix("VERSION_ID=").map(|value| value.trim_matches('"').to_owned()))
        }) {
            result.limits.extend(runner_image_limit(&job.runs_on, &version_id, std::env::consts::ARCH));
        }
        // What of the runner image the job asks for this machine does not have.
        if let Some(image) = tools.profile()?.image.as_ref().filter(|image| image.runner == job.runs_on) {
            for missing in &image.not_provided {
                result.limits.push(format!("Runner image {} {}: not provided: {missing}", image.runner, image.version));
            }
        }
        if let Some(environment) = &job.environment {
            let url = environment.url.as_deref().map(|url| format!(" ({url})")).unwrap_or_default();
            result.limits.push(format!(
                "job {} environment {}{url}: recorded only; its protection rules, secrets and deployment record have no local effect.",
                job.id, environment.name
            ));
        }
        // Every job starts in an empty workspace; actions/checkout fills it.
        if workspace.exists() {
            std::fs::remove_dir_all(&workspace)
                .with_context(|| format!("이전 workspace를 지우지 못했어요: {}", workspace.display()))?;
        }
        std::fs::create_dir_all(&workspace)?;
        let mut runner = Runner::new(&directory.join("runner").join(&job.id), &workspace, tools.platform)?;
        let tracking = format!("build-machine-{}-{}-{}", std::process::id(), job.id, build_machine_core::now());
        for (key, value) in runner_context(request, job, &tracking) {
            runner.set(key, &value);
        }
        let status =
            run_job(job, runner, &workspace, request, &base, tools, &mut result, &mut stages, &mut failure, &github, &pages)?;
        finished.insert(job.id.as_str(), status);
        #[cfg(target_os = "linux")]
        for process in crate::runner::terminate_orphans(&tracking) {
            println!("Terminate orphan process: pid {process}");
        }
        // The job's workspace and runner files go with the job; its artifacts stay.
        finish_job(&work, &directory.join("artifacts").join(&job.id), &mut result.artifacts)?;
        let runner_files = directory.join("runner").join(&job.id);
        if runner_files.exists() {
            std::fs::remove_dir_all(&runner_files)?;
        }
        #[cfg(not(target_os = "linux"))]
        result.limits.push(
            "Processes a job leaves running are not terminated at its end on this platform, as a runner terminates them.".to_owned(),
        );
    }
    for (stage, reason) in &request.skips {
        let present = stages.get(stage.as_str()).is_some_and(|stage| !stage.steps.is_empty());
        if present {
            continue;
        }
        let Some(stage_name) = build_machine_core::workflow::STAGE_ORDER.iter().find(|name| **name == stage.as_str()) else {
            continue;
        };
        result.limits.push(format!("{stage} skipped: {reason}"));
        let marker = Step { name: format!("skip {stage}"), adapter: Adapter::Skip, reason: Some(reason.clone()), ..Step::default() };
        let mut ran = Ran::new(Outcome::PassedWithLimits);
        ran.reason = Some(reason.clone());
        record_in(&mut stages, stage_name, &marker, ran, build_machine_core::now());
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
    result.require_executed_steps();
    Ok(result)
}
