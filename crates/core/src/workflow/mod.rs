//! The repository workflow read as this machine's build contract.

pub mod adapter;
pub mod condition;
pub mod parse;
pub mod stage;

pub use adapter::{Adapter, PhpCoverage, STAGE_ORDER};
pub use condition::{ref_name, Condition, Github, JobResult, JobStatus, Outputs, Reference, Template};
pub use parse::{checkout_inputs, checkout_path, checkout_target, names_secret, reads_input, CheckoutTarget, Environment, Job, Step, Workflow, DEFAULT_JOB_TIMEOUT_MINUTES};
pub use stage::{check_gates, jobs_for, stage_counts, stage_of, stages, stages_for};

use crate::Platform;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The platform a runner label names, when it names one this machine drives.
pub fn platform_for_runner(label: &str) -> Option<Platform> {
    let label = label.to_lowercase();
    if label.starts_with("macos") {
        Some(Platform::Macos)
    } else if label.starts_with("ubuntu") || label.starts_with("linux") {
        Some(Platform::Linux)
    } else if label.starts_with("windows") {
        Some(Platform::Windows)
    } else {
        None
    }
}

/// The platforms this workflow has jobs for.
///
/// A workflow with a job per operating system is what a three-OS release looks
/// like. An empty set means no job named a runner this machine recognises.
pub fn declared_platforms(workflow: &Workflow) -> BTreeSet<Platform> {
    workflow.jobs.iter().filter_map(|job| platform_for_runner(&job.runs_on)).collect()
}

/// The entry of `repositories` that names a repository. GitHub resolves an
/// owner and a repository without regard to case, so the lookup does too.
pub fn find_repository<'a, V>(repositories: &'a BTreeMap<String, V>, name: &str) -> Option<(&'a String, &'a V)> {
    repositories.iter().find(|(key, _)| key.eq_ignore_ascii_case(name))
}

/// Every other repository the workflow's checkout steps name, by its
/// `repositories` key, with the local clone that entry maps it to. A
/// repository named by an expression is read with the replay's `github`
/// values. A repository the map does not name has no history to check out,
/// so it fails validation.
pub fn checkout_repositories(
    workflow: &Workflow,
    repositories: &BTreeMap<String, String>,
    github: &Github,
) -> Result<BTreeMap<String, PathBuf>> {
    let mut found = BTreeMap::new();
    for step in workflow.jobs.iter().flat_map(|job| &job.steps).filter(|step| step.adapter == Adapter::Checkout) {
        let Some(target) = checkout_target(&step.with)? else { continue };
        let Some(repository) = target.repository.as_deref() else { continue };
        let owner = format!("job {}의 step {} ({})", step.job_id, step.index, step.name);
        let repository = Template::parse(repository).with_context(|| owner.clone())?.render(&Outputs::new(), github);
        parse::check_repository_name(&repository).with_context(|| owner.clone())?;
        let Some((key, clone)) = find_repository(repositories, &repository) else {
            let known: Vec<&str> = repositories.keys().map(String::as_str).collect();
            bail!(
                "job {}의 step {} ({})이 checkout하는 {}가 machine.json repositories에 없어요. 이 저장소의 로컬 clone 경로를 등록해주세요. 등록된 저장소: {}",
                step.job_id,
                step.index,
                step.name,
                repository,
                if known.is_empty() { "없음".to_owned() } else { known.join(", ") }
            );
        };
        let clone = PathBuf::from(clone);
        if !clone.is_absolute() {
            bail!("machine.json repositories의 {key} 경로는 절대 경로여야 해요: {}", clone.display());
        }
        found.insert(key.clone(), clone);
    }
    Ok(found)
}

/// The `github` values of a replay of `sha` for `event`, checked out as
/// `reference` (`refs/heads/<branch>` or `refs/tags/<tag>`), with the event
/// `payload` the replay was given. A commit that no branch or tag names has
/// no `github.ref`, and a payload has only the values it holds, so jobs that
/// read anything else are refused rather than given an empty value.
pub fn github_context(
    jobs: &[Job],
    event: &str,
    sha: &str,
    reference: Option<&str>,
    payload: Option<&serde_json::Value>,
) -> Result<Github> {
    for job in jobs {
        for path in &job.event_paths {
            let names: Vec<String> = path.split('.').map(str::to_owned).collect();
            match payload {
                None => bail!(
                    "job {}가 github.event.{path}를 읽지만 이 재현에는 이벤트 payload가 없어요. --event-payload로 {event} 이벤트의 payload JSON 파일을 줘야 해요.",
                    job.id
                ),
                Some(payload) if condition::event_value(payload, &names).is_none() => bail!(
                    "job {}가 github.event.{path}를 읽지만 이벤트 payload에 그 값이 없어요.",
                    job.id
                ),
                Some(_) => {}
            }
        }
    }
    if reference.is_none() {
        if let Some(job) = jobs.iter().find(|job| job.reads_ref) {
            bail!(
                "job {}가 github.ref 또는 github.ref_name을 읽지만 이 재현에는 ref 이름이 없어요: {sha}는 branch나 tag로 checkout되지 않아요(commit SHA 또는 detached HEAD). branch나 tag를 재현해야 해요.",
                job.id
            );
        }
    }
    Ok(Github {
        event: payload.cloned(),
        event_name: event.to_owned(),
        sha: sha.to_owned(),
        reference: reference.map(str::to_owned),
    })
}

/// Read and validate a workflow file.
pub fn load(path: &Path, event: &str, reference: Option<&str>) -> Result<Workflow> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("워크플로 파일을 찾을 수 없어요: {}", path.display()))?;
    let workflow = parse::parse(&path.to_string_lossy(), &source, event, reference)?;
    check_gates(&workflow)?;
    Ok(workflow)
}

/// Read and validate workflow text that was extracted from an immutable ref;
/// `read` gives the text of a reusable workflow at that ref.
pub fn load_text(
    path: &str,
    source: &str,
    event: &str,
    reference: Option<&str>,
    read: &dyn Fn(&str) -> Result<String>,
) -> Result<Workflow> {
    let workflow = parse::parse_with(path, source, event, reference, read)?;
    check_gates(&workflow)?;
    Ok(workflow)
}

/// Locate the workflow to replay. A repository with more than one must say
/// which, rather than the machine picking for it.
pub fn discover(root: &Path, requested: Option<&str>, event: &str, reference: Option<&str>) -> Result<Workflow> {
    if let Some(requested) = requested {
        let path = PathBuf::from(requested);
        let path = if path.is_absolute() { path } else { root.join(path) };
        return load(&path, event, reference);
    }
    let directory = root.join(".github").join("workflows");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    matches!(path.extension().and_then(|value| value.to_str()), Some("yml") | Some("yaml"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    match files.len() {
        0 => bail!(".github/workflows에 워크플로가 없어요."),
        1 => load(&files[0], event, reference),
        _ => bail!("워크플로가 여러 개예요. --workflow로 하나를 선택해야 해요."),
    }
}
