//! Reading a repository workflow as a contract.
//!
//! Only the subset this machine can actually reproduce is accepted. Anything
//! else — an unknown action, a container, a matrix, an expression whose value
//! is not knowable locally — fails validation instead of being quietly
//! changed into something the machine can run.

use super::adapter::Adapter;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub index: u32,
    pub name: String,
    #[serde(skip)]
    pub adapter: Adapter,
    pub adapter_name: String,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub with: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub job_env: BTreeMap<String, String>,
    pub job_id: String,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub needs: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub steps: Vec<Step>,
}

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
        if (stage == "test" || stage == "smoke") && !reason.is_empty() {
            skips.insert(stage.to_owned(), reason.to_owned());
        }
    }
    skips
}

fn parse_steps(job_id: &str, job_env: &BTreeMap<String, String>, value: Option<&Yaml>) -> Result<Vec<Step>> {
    let Some(Yaml::Sequence(items)) = value else {
        bail!("각 job에는 하나 이상의 steps가 필요해요.");
    };
    if items.is_empty() {
        bail!("각 job에는 하나 이상의 steps가 필요해요.");
    }
    let mut steps = Vec::new();
    for (position, item) in items.iter().enumerate() {
        let index = position as u32 + 1;
        if as_map(item).is_none() {
            bail!("step {index}가 객체가 아니에요.");
        }
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
        steps.push(Step {
            index,
            name,
            adapter,
            adapter_name: adapter.as_str().to_owned(),
            action,
            action_ref,
            run,
            working_directory: text(get(item, "working-directory")),
            condition: text(get(item, "if")),
            reason: None,
            env: string_map(get(item, "env")),
            with: string_map(get(item, "with")),
            job_env: job_env.clone(),
            job_id: job_id.to_owned(),
        });
    }
    Ok(steps)
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
        if get(value, "runs-on").is_none() {
            bail!("job {id}에 runs-on이 없어요.");
        }
        for unsupported in ["container", "services", "uses", "strategy"] {
            if get(value, unsupported).is_some() {
                bail!("job {id}의 container/services/reusable/matrix는 아직 지원하지 않아요.");
            }
        }
        let needs = match get(value, "needs") {
            None => Vec::new(),
            Some(Yaml::String(value)) => vec![value.clone()],
            Some(Yaml::Sequence(values)) => values.iter().filter_map(|value| text(Some(value))).collect(),
            Some(_) => bail!("job {id}의 needs 형식이 올바르지 않아요."),
        };
        let env = string_map(get(value, "env"));
        let steps = parse_steps(&id, &env, get(value, "steps"))?;
        jobs.push(Job { id, needs, env, steps });
    }
    Ok(Workflow {
        path: path.to_owned(),
        name: text(get(&document, "name")).unwrap_or_else(|| path.to_owned()),
        event: event.to_owned(),
        reference: reference.map(str::to_owned),
        jobs: order_jobs(jobs)?,
        skips: skip_comments(source),
    })
}
