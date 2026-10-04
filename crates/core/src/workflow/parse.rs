//! Reading a repository workflow as a contract.
//!
//! Only the subset this machine can actually reproduce is accepted. Anything
//! else — an unknown action, a container, a matrix, an expression whose value
//! is not knowable locally — fails validation instead of being quietly
//! changed into something the machine can run.

use super::adapter::Adapter;
use super::condition::{Condition, Github, Outputs, Reference, Template};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub index: u32,
    /// The step's `id`, which names its outputs for the steps after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Where the step falls in the whole workflow, jobs in `needs` order. A
    /// replay runs steps in this order; the stage is only how they are reported.
    #[serde(default)]
    pub position: u32,
    pub name: String,
    pub adapter: Adapter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The step's own `timeout-minutes`. Without it a step is bounded only by
    /// its job's limit, as on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub with: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub job_env: BTreeMap<String, String>,
    pub job_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    /// The runner the workflow asks GitHub for. A replay has to run somewhere
    /// that can do what this job was written for.
    pub runs_on: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    /// The job's `timeout-minutes`, or GitHub's default of 360.
    pub timeout_minutes: u64,
    /// The workflow's `env` under the job's own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub steps: Vec<Step>,
    /// The job's `environment`. It has no local effect: a replay records it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Whether an expression of the job reads `github.ref` or
    /// `github.ref_name`, which a replay of a commit without a branch or tag
    /// name cannot give.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reads_ref: bool,
    /// The `github.event` paths the job's expressions read, written with
    /// dots. A replay's event payload must hold each.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub event_paths: BTreeSet<String>,
    /// The job's `if:`. Without one a job runs when every job before it
    /// succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// A job's deployment environment, given as a name or as `{name, url}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A job's `environment`. The name and the URL are recorded as written, so an
/// expression, which has no value before the job runs, is refused.
fn environment(value: Option<&Yaml>, job: &str) -> Result<Option<Environment>> {
    let literal = |value: Option<String>, key: &str| -> Result<Option<String>> {
        match value {
            Some(text) if text.contains("${{") => bail!("job {job}의 environment의 {key}에는 식을 쓸 수 없어요: {text}"),
            other => Ok(other),
        }
    };
    let Some(value) = value else { return Ok(None) };
    let (name, url) = match value {
        Yaml::Mapping(_) => {
            check_keys(value, &["name", "url"], &format!("job {job}의 environment"))?;
            (text(get(value, "name")), text(get(value, "url")))
        }
        other => (text(Some(other)), None),
    };
    let name = literal(name, "name")?
        .filter(|name| !name.trim().is_empty())
        .with_context(|| format!("job {job}의 environment의 name이 필요해요."))?;
    // GitHub evaluates the URL after the job's steps; it is checked against
    // them once they are read, and recorded as written.
    Ok(Some(Environment { name, url }))
}

/// GitHub's limit for a job that declares no `timeout-minutes`.
pub const DEFAULT_JOB_TIMEOUT_MINUTES: u64 = 360;

#[derive(Clone, Debug)]
pub struct Workflow {
    pub path: String,
    pub name: String,
    pub event: String,
    pub reference: Option<String>,
    pub jobs: Vec<Job>,
    pub skips: BTreeMap<String, String>,
}

type Yaml = serde_yaml_ng::Value;

fn as_map(value: &Yaml) -> Option<&serde_yaml_ng::Mapping> {
    value.as_mapping()
}

fn get<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    as_map(value).and_then(|map| map.get(Yaml::from(key)))
}

fn text(value: Option<&Yaml>) -> Option<String> {
    match value? {
        Yaml::String(value) => Some(value.clone()),
        Yaml::Number(value) => Some(value.to_string()),
        Yaml::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn string_map(value: Option<&Yaml>) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    if let Some(map) = value.and_then(as_map) {
        for (key, item) in map {
            if let (Some(key), Some(item)) = (text(Some(key)), text(Some(item))) {
                result.insert(key, item);
            }
        }
    }
    result
}

/// Which events the workflow declares. `on` is a YAML 1.1 boolean, so a parser
/// may hand it back as the key `true`; both spellings are accepted.
fn declared_events(document: &Yaml) -> Vec<String> {
    let triggers = get(document, "on").or_else(|| as_map(document).and_then(|map| map.get(Yaml::Bool(true))));
    match triggers {
        Some(Yaml::String(value)) => vec![value.clone()],
        Some(Yaml::Sequence(values)) => values.iter().filter_map(|value| text(Some(value))).collect(),
        Some(Yaml::Mapping(map)) => map.keys().filter_map(|key| text(Some(key))).collect(),
        _ => Vec::new(),
    }
}

/// The step keys a replay honours. Any other key — `shell`,
/// `continue-on-error` — changes how GitHub runs the step, so it fails
/// validation rather than being dropped.
const STEP_KEYS: [&str; 9] = ["name", "id", "uses", "run", "with", "env", "if", "working-directory", "timeout-minutes"];

/// The job keys a replay implements. `environment` is recorded and has no
/// local effect. `if`, `continue-on-error`, `strategy`, `defaults`,
/// `outputs`, `permissions`, containers, services and reusable
/// workflows each change which steps GitHub runs or how, so any other key
/// fails validation.
const JOB_KEYS: [&str; 9] =
    ["name", "runs-on", "needs", "env", "steps", "timeout-minutes", "environment", "if", "permissions"];

/// The workflow keys a replay implements. `permissions` and `concurrency`
/// only govern the GitHub token and overlapping runs, neither of which a
/// local replay has; `defaults` changes how every step runs and is refused.
const WORKFLOW_KEYS: [&str; 7] = ["name", "run-name", "on", "env", "jobs", "permissions", "concurrency"];

/// Refuse a key outside `accepted`.
fn check_keys(value: &Yaml, accepted: &[&str], owner: &str) -> Result<()> {
    if let Some(map) = as_map(value) {
        for key in map.keys() {
            let key = match key {
                // `on` is a YAML 1.1 boolean.
                Yaml::Bool(true) => "on".to_owned(),
                other => text(Some(other)).unwrap_or_default(),
            };
            if !accepted.contains(&key.as_str()) {
                bail!("{owner}의 '{key}'는 아직 지원하지 않아요.");
            }
        }
    }
    Ok(())
}

/// A `timeout-minutes` value: a whole number of minutes, at least one. An
/// expression has no value before the run, so it is refused with the rest.
fn timeout_minutes(value: Option<&Yaml>, owner: &str) -> Result<Option<u64>> {
    match value {
        None => Ok(None),
        Some(Yaml::Number(number)) if number.as_u64().is_some_and(|minutes| minutes > 0) => Ok(number.as_u64()),
        Some(other) => bail!(
            "{owner}의 timeout-minutes는 1 이상의 정수여야 해요: {}",
            serde_yaml_ng::to_string(other).unwrap_or_default().trim()
        ),
    }
}

/// Stage omissions the workflow documents on purpose.
fn skip_comments(source: &str) -> BTreeMap<String, String> {
    let mut skips = BTreeMap::new();
    for line in source.lines() {
        let Some(rest) = line.split_once("build-machine:").map(|(_, rest)| rest.trim()) else { continue };
        let Some(rest) = rest.strip_prefix("skip") else { continue };
        let rest = rest.trim();
        let Some((stage, reason)) = rest.split_once("reason=") else { continue };
        let stage = stage.trim();
        let reason = reason.trim();
        if matches!(stage, "build" | "test" | "smoke") && !reason.is_empty() {
            skips.insert(stage.to_owned(), reason.to_owned());
        }
    }
    skips
}

/// `actions/checkout`'s inputs: `fetch-depth` (1 by default, 0 for the whole
/// history) and `fetch-tags`. `clean` and `persist-credentials` change nothing
/// in a workspace that starts empty and has no credentials to keep.
pub fn checkout_inputs(with: &BTreeMap<String, String>) -> Result<(u32, bool)> {
    let depth = match with.get("fetch-depth") {
        None => 1,
        Some(value) => value.trim().parse().ok().with_context(|| format!("fetch-depth는 0 이상의 정수여야 해요: {value}"))?,
    };
    let flag = |input: &str| -> Result<bool> {
        match with.get(input).map(|value| value.trim()) {
            None => Ok(false),
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(other) => bail!("{input}는 true 또는 false여야 해요: {other}"),
        }
    };
    flag("clean")?;
    flag("persist-credentials")?;
    Ok((depth, flag("fetch-tags")?))
}

/// Another repository a checkout step names: `repository`, the `ref` to check
/// out (its bundle's `HEAD` when absent) and the `path` under
/// `GITHUB_WORKSPACE` (the workspace itself when absent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutTarget {
    pub repository: String,
    pub reference: Option<String>,
    pub path: Option<String>,
}

/// Whether a name is GitHub's `owner/name`. It is looked up in
/// `machine.json`'s `repositories` before the run, so an expression is
/// refused with every other form.
fn is_repository_name(name: &str) -> bool {
    let part = |value: &str| {
        !value.is_empty() && value.chars().all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
    };
    matches!(name.split_once('/'), Some((owner, rest)) if part(owner) && part(rest) && !matches!(rest, "." | ".."))
}

/// `actions/checkout`'s `path`: where under `GITHUB_WORKSPACE` the step checks
/// out, for the job's own repository and for another. `None` is the workspace
/// itself.
pub fn checkout_path(with: &BTreeMap<String, String>) -> Result<Option<String>> {
    let Some(path) = with.get("path").map(|value| value.trim().to_owned()) else { return Ok(None) };
    // Read the same on every worker: either separator, no drive, no root.
    let inside = !path.contains("${{")
        && !path.contains(':')
        && !path.starts_with(['/', '\\'])
        && path.split(['/', '\\']).all(|part| part != "..");
    if !inside {
        bail!("checkout의 path는 GITHUB_WORKSPACE 안의 상대 경로여야 해요: {path}");
    }
    Ok(Some(path))
}

/// `actions/checkout`'s `repository` and `ref`, with its `path`. Without
/// `repository` the step checks out the replayed revision, so `ref` is
/// refused there rather than ignored.
pub fn checkout_target(with: &BTreeMap<String, String>) -> Result<Option<CheckoutTarget>> {
    let path = checkout_path(with)?;
    let Some(repository) = with.get("repository").map(|value| value.trim()) else {
        if with.contains_key("ref") {
            bail!("checkout의 ref는 repository와 함께 다른 저장소를 checkout할 때만 지원해요.");
        }
        return Ok(None);
    };
    if !is_repository_name(repository) {
        bail!("checkout의 repository는 owner/name 형식이어야 해요: {repository}");
    }
    Ok(Some(CheckoutTarget {
        repository: repository.to_owned(),
        reference: with.get("ref").map(|value| value.trim().to_owned()),
        path,
    }))
}

/// A value that names a secret or the GitHub token. Neither exists locally:
/// an `env` value of one is empty in a replay, as an unset secret is on
/// GitHub, and the replay records that as a limit.
pub fn names_secret(value: &str) -> bool {
    value.contains("secrets.") || value.contains("github.token")
}

/// Whether an adapter reads this `with` input. An input it does not read is
/// never evaluated, so an expression in it has no effect on the replay.
pub fn reads_input(adapter: Adapter, input: &str) -> bool {
    match adapter {
        Adapter::TauriBuild => input == "args",
        other => other.inputs().is_some_and(|inputs| inputs.contains(&input)),
    }
}

/// Check that every step output an expression reads belongs to an earlier
/// `run` step of the same job with that `id`.
fn check_references(references: Vec<Reference>, defined: &[String], owner: &str) -> Result<()> {
    for reference in references {
        if !defined.contains(&reference.step) {
            bail!(
                "{owner}: steps.{}.outputs.{}의 {}는 이 job에서 앞선 run 단계의 id가 아니에요.",
                reference.step,
                reference.output,
                reference.step
            );
        }
    }
    Ok(())
}

/// What a job's expressions read of the `github` context that a replay has
/// to give: the ref name and the paths of the event payload.
#[derive(Default)]
struct Reads {
    reference: bool,
    event: BTreeSet<String>,
}

impl Reads {
    fn template(&mut self, template: &Template) {
        self.reference |= template.reads_ref();
        self.event.extend(template.event_paths());
    }

    fn condition(&mut self, condition: &Condition) {
        self.reference |= condition.reads_ref();
        self.event.extend(condition.event_paths());
    }
}

/// Check a template and note what it reads of the `github` context.
fn check_template(text: &str, defined: &[String], owner: &str, reads: &mut Reads) -> Result<()> {
    let template = Template::parse(text).with_context(|| owner.to_owned())?;
    check_references(template.references(), defined, owner)?;
    reads.template(&template);
    Ok(())
}

impl Step {
    /// The step with the outputs of the steps before it and the `github`
    /// values in place, as GitHub evaluates a step's expressions just before
    /// it runs. Secret values are left for the runner to empty; inputs the
    /// adapter does not read are left as written.
    pub fn resolve(&self, outputs: &Outputs, github: &Github) -> Result<Step> {
        let render = |text: &str| -> Result<String> { Ok(Template::parse(text)?.render(outputs, github)) };
        let mut step = self.clone();
        step.run = self.run.as_deref().map(render).transpose()?;
        step.working_directory = self.working_directory.as_deref().map(render).transpose()?;
        for (key, value) in step.env.iter_mut() {
            if !names_secret(value) {
                *value = render(value).with_context(|| format!("env {key}"))?;
            }
        }
        for (key, value) in step.with.iter_mut() {
            if reads_input(self.adapter, key) {
                *value = render(value).with_context(|| format!("with {key}"))?;
            }
        }
        Ok(step)
    }
}

/// The job's steps, and what their expressions and the job's `env` read of
/// the `github` context.
fn parse_steps(job_id: &str, job_env: &BTreeMap<String, String>, value: Option<&Yaml>) -> Result<(Vec<Step>, Reads)> {
    let Some(Yaml::Sequence(items)) = value else {
        bail!("각 job에는 하나 이상의 steps가 필요해요.");
    };
    if items.is_empty() {
        bail!("각 job에는 하나 이상의 steps가 필요해요.");
    }
    let mut steps = Vec::new();
    // The ids of the earlier `run` steps, whose outputs later steps may read.
    let mut defined: Vec<String> = Vec::new();
    let mut reads = Reads::default();
    for (key, value) in job_env {
        if !names_secret(value) {
            check_template(value, &[], &format!("job {job_id}의 env {key}"), &mut reads)?;
        }
    }
    for (position, item) in items.iter().enumerate() {
        let index = position as u32 + 1;
        if as_map(item).is_none() {
            bail!("step {index}가 객체가 아니에요.");
        }
        check_keys(item, &STEP_KEYS, &format!("job {job_id}의 step {index}"))?;
        let uses = text(get(item, "uses"));
        let run = text(get(item, "run"));
        let (adapter, action, action_ref) = match (&uses, &run) {
            (Some(uses), _) => {
                let (name, reference) = uses
                    .split_once('@')
                    .with_context(|| format!("액션 ref가 없는 uses 단계는 지원하지 않아요: {uses}"))?;
                let adapter = Adapter::for_action(name).with_context(|| {
                    format!("어댑터가 없는 GitHub action은 실행할 수 없어요: {name}@{reference}")
                })?;
                (adapter, Some(name.to_owned()), Some(reference.to_owned()))
            }
            (None, Some(_)) => (Adapter::Run, None, None),
            (None, None) => bail!("step {index}에는 uses 또는 run이 필요해요."),
        };
        let name = text(get(item, "name"))
            .or_else(|| uses.clone())
            .unwrap_or_else(|| "run".to_owned());
        let with = string_map(get(item, "with"));
        if let Some(accepted) = adapter.inputs() {
            if let Some(input) = with.keys().find(|input| !accepted.contains(&input.as_str())) {
                bail!("{name}의 with 입력 '{input}'은 아직 지원하지 않아요. 지원 입력: {}", accepted.join(", "));
            }
        }
        if let Some(coverage) = with.get("coverage").filter(|_| adapter == Adapter::PhpSetup) {
            // An expression is checked when the step runs, with its value.
            if !coverage.contains("${{") {
                super::adapter::PhpCoverage::parse(coverage).with_context(|| format!("job {job_id}의 step {index} ({name})"))?;
            }
        }
        if adapter == Adapter::Checkout {
            checkout_inputs(&with).with_context(|| format!("job {job_id}의 step {index} ({name})"))?;
            checkout_target(&with).with_context(|| format!("job {job_id}의 step {index} ({name})"))?;
        }
        let owner = format!("job {job_id}의 step {index} ({name})");
        let condition = text(get(item, "if"));
        let parsed = Condition::parse(condition.as_deref()).with_context(|| owner.clone())?;
        check_references(parsed.references(), &defined, &owner)?;
        if let Some(job) = parsed.needs().first() {
            bail!("{owner}: needs.{job}.result는 job의 if에서만 읽어요.");
        }
        reads.condition(&parsed);
        for text in [&run, &text(get(item, "working-directory"))].into_iter().flatten() {
            check_template(text, &defined, &owner, &mut reads)?;
        }
        let env = string_map(get(item, "env"));
        for (key, value) in &env {
            if !names_secret(value) {
                check_template(value, &defined, &format!("{owner}의 env {key}"), &mut reads)?;
            }
        }
        for (key, value) in &with {
            if reads_input(adapter, key) {
                check_template(value, &defined, &format!("{owner}의 with {key}"), &mut reads)?;
            }
        }
        let id = text(get(item, "id"));
        if let Some(id) = &id {
            if steps.iter().any(|step: &Step| step.id.as_ref() == Some(id)) {
                bail!("{owner}: id {id}가 이 job에서 두 번 쓰였어요.");
            }
            // Only a `run` step writes outputs here; no adapter sets any.
            if adapter == Adapter::Run {
                defined.push(id.clone());
            }
        }
        steps.push(Step {
            index,
            id,
            position: 0,
            timeout_minutes: timeout_minutes(get(item, "timeout-minutes"), &format!("job {job_id}의 step {index}"))?,
            name,
            adapter,
            action,
            action_ref,
            run,
            working_directory: text(get(item, "working-directory")),
            condition,
            reason: None,
            env,
            with,
            job_env: job_env.clone(),
            job_id: job_id.to_owned(),
        });
    }
    Ok((steps, reads))
}

fn order_jobs(mut jobs: Vec<Job>) -> Result<Vec<Job>> {
    let known: Vec<String> = jobs.iter().map(|job| job.id.clone()).collect();
    for job in &jobs {
        let missing: Vec<&String> = job.needs.iter().filter(|need| !known.contains(need)).collect();
        if !missing.is_empty() {
            bail!(
                "job {}가 없는 needs를 참조해요: {}",
                job.id,
                missing.iter().map(|value| value.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
    let mut ordered: Vec<Job> = Vec::new();
    while !jobs.is_empty() {
        let done: Vec<String> = ordered.iter().map(|job| job.id.clone()).collect();
        let (ready, pending): (Vec<Job>, Vec<Job>) = jobs
            .into_iter()
            .partition(|job| job.needs.iter().all(|need| done.contains(need)));
        if ready.is_empty() {
            bail!("job needs 순환 또는 실행 순서를 확인할 수 없어요.");
        }
        ordered.extend(ready);
        jobs = pending;
    }
    Ok(ordered)
}

pub fn parse(path: &str, source: &str, event: &str, reference: Option<&str>) -> Result<Workflow> {
    let document: Yaml = serde_yaml_ng::from_str(source)
        .with_context(|| format!("워크플로 YAML을 읽지 못했어요: {path}"))?;
    let events = declared_events(&document);
    if events.is_empty() {
        bail!("워크플로에 이벤트(on)가 없어요.");
    }
    if !events.iter().any(|value| value == event) {
        let mut sorted = events.clone();
        sorted.sort();
        bail!("이 워크플로는 {event} 이벤트를 지원하지 않아요. 지원 이벤트: {}", sorted.join(", "));
    }
    check_keys(&document, &WORKFLOW_KEYS, "워크플로")?;
    let workflow_env = string_map(get(&document, "env"));
    let Some(Yaml::Mapping(job_map)) = get(&document, "jobs") else {
        bail!("워크플로에 jobs가 없어요.");
    };
    if job_map.is_empty() {
        bail!("워크플로에 jobs가 없어요.");
    }
    let mut jobs = Vec::new();
    for (key, value) in job_map {
        let id = text(Some(key)).unwrap_or_default();
        if as_map(value).is_none() {
            bail!("job {id}가 객체가 아니에요.");
        }
        let Some(runs_on) = text(get(value, "runs-on")) else {
            bail!("job {id}에 runs-on이 없어요.");
        };
        check_keys(value, &JOB_KEYS, &format!("job {id}"))?;
        let needs = match get(value, "needs") {
            None => Vec::new(),
            Some(Yaml::String(value)) => vec![value.clone()],
            Some(Yaml::Sequence(values)) => values.iter().filter_map(|value| text(Some(value))).collect(),
            Some(_) => bail!("job {id}의 needs 형식이 올바르지 않아요."),
        };
        let mut env = workflow_env.clone();
        env.extend(string_map(get(value, "env")));
        let timeout_minutes =
            timeout_minutes(get(value, "timeout-minutes"), &format!("job {id}"))?.unwrap_or(DEFAULT_JOB_TIMEOUT_MINUTES);
        let environment = environment(get(value, "environment"), &id)?;
        let (steps, mut reads) = parse_steps(&id, &env, get(value, "steps"))?;
        if let Some(url) = environment.as_ref().and_then(|environment| environment.url.as_deref()) {
            let owner = format!("job {id}의 environment의 url");
            let template = Template::parse(url).with_context(|| owner.clone())?;
            if let Some(reference) = template.references().into_iter().find(|reference| {
                !steps.iter().any(|step| step.id.as_deref() == Some(reference.step.as_str()))
            }) {
                bail!("{owner}: steps.{}.outputs.{}의 {}는 이 job의 단계 id가 아니에요.", reference.step, reference.output, reference.step);
            }
            reads.template(&template);
        }
        // A job's `if` reads the github context and the results of the jobs
        // it needs; it runs before any step, so no step output exists yet.
        let condition = text(get(value, "if"));
        let parsed = Condition::parse(condition.as_deref()).with_context(|| format!("job {id}의 if"))?;
        if let Some(reference) = parsed.references().first() {
            bail!("job {id}의 if가 steps.{}.outputs.{}를 읽어요. job의 if에는 step 출력이 없어요.", reference.step, reference.output);
        }
        if let Some(job) = parsed.needs().into_iter().find(|job| !needs.contains(job)) {
            bail!("job {id}의 if가 needs.{job}.result를 읽지만 {job}는 이 job의 needs에 없어요.");
        }
        reads.condition(&parsed);
        jobs.push(Job {
            id,
            runs_on,
            needs,
            timeout_minutes,
            env,
            steps,
            environment,
            reads_ref: reads.reference,
            event_paths: reads.event,
            condition,
        });
    }
    // Each platform replays its own jobs, so a job can only wait for a job
    // that is replayed wherever it is.
    for job in &jobs {
        let platform = super::platform_for_runner(&job.runs_on);
        for need in &job.needs {
            let needed = jobs.iter().find(|other| &other.id == need).map(|other| super::platform_for_runner(&other.runs_on));
            if let Some(Some(needed)) = needed {
                if platform != Some(needed) {
                    bail!("job {}가 다른 운영체제의 job {need}를 needs로 기다려요. 운영체제마다 따로 재현하므로 지원하지 않아요.", job.id);
                }
            }
        }
    }
    let mut jobs = order_jobs(jobs)?;
    let mut position = 0;
    for step in jobs.iter_mut().flat_map(|job| job.steps.iter_mut()) {
        position += 1;
        step.position = position;
    }
    Ok(Workflow {
        path: path.to_owned(),
        name: text(get(&document, "name")).unwrap_or_else(|| path.to_owned()),
        event: event.to_owned(),
        reference: reference.map(str::to_owned),
        jobs,
        skips: skip_comments(source),
    })
}
