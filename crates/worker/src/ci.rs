//! Replaying the supported part of a repository workflow.
//!
//! Every external GitHub service is replaced by a local adapter and recorded as
//! a limit. A local success never establishes production signing, notarization
//! or release publication.

use crate::build::{collect_artifacts, extract_source, project_root, shell_for};
use crate::provision::Tools;
use crate::stream;
use anyhow::{bail, Result};
use build_machine_core::report::{Artifact, Outcome, PlatformResult, Stage, Step as ReportStep};
use build_machine_core::request::WorkRequest;
use build_machine_core::workflow::{Adapter, Step, STAGE_ORDER};
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

/// Whether a step's `if` condition is true locally.
///
/// Only conditions whose value is knowable here are accepted. An expression
/// this machine cannot resolve fails the run rather than being guessed at.
fn condition_enabled(step: &Step) -> Result<(bool, Option<String>)> {
    let Some(condition) = step.condition.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((true, None));
    };
    if condition.contains("secrets.") || condition.contains("SIGNING_CONFIGURED") || condition.contains("github.token")
    {
        return Ok((false, Some("A GitHub secret or token condition is false in the local runner.".to_owned())));
    }
    match condition {
        "false" | "'false'" | "0" => Ok((false, None)),
        "true" | "'true'" | "1" | "always()" => Ok((true, None)),
        other => bail!("Unsupported workflow condition: {other}"),
    }
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
    let working = source.join(relative);
    let resolved = working.canonicalize().unwrap_or(working);
    if !resolved.starts_with(source) {
        bail!("Workflow working-directory must be inside the source snapshot.");
    }
    Ok(resolved)
}

fn tauri_action(step: &Step, source: &Path, request: &WorkRequest, environment: &[(String, String)]) -> Result<Vec<Artifact>> {
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
    let bundle = request.bundle.clone().unwrap_or_else(|| "none".to_owned());
    let mut arguments = base;
    arguments.extend([
        "--ci".to_owned(),
        "--no-sign".to_owned(),
        "--target".to_owned(),
        request.target.clone(),
        "--bundles".to_owned(),
        bundle,
        "--".to_owned(),
        "--locked".to_owned(),
    ]);
    if let Some(extra) = step.with.get("args") {
        arguments.extend(extra.split_whitespace().map(str::to_owned));
    }
    stream::checked(&program, &arguments, Some(source), environment)?;
    let output = source.join(format!("src-tauri/target/{}/release/bundle", request.target));
    let mut artifacts = Vec::new();
    for extension in ["dmg", "deb", "exe", "msi"] {
        artifacts.extend(collect_artifacts(&output, extension)?);
    }
    Ok(artifacts)
}

pub fn replay(request: &WorkRequest, tools: &Tools) -> Result<PlatformResult> {
    tools.setup_system()?;
    tools.setup_user()?;
    let root = project_root(request)?;
    let signature = request.workflow_signature.clone().unwrap_or_else(|| request.snapshot.source_hash.clone());
    let directory = root.join(format!("ci-{}", &signature[..signature.len().min(24)]));
    let source = extract_source(Path::new(&request.archive), &request.snapshot.source_hash, &directory)?;

    let base: Vec<(String, String)> = tools
        .environment
        .iter()
        .filter(|(key, _)| {
            !key.starts_with("APPLE_")
                && !matches!(
                    key.as_str(),
                    "TAURI_SIGNING_PRIVATE_KEY" | "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" | "GITHUB_TOKEN"
                )
        })
        .cloned()
        .collect();

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

    for stage_name in STAGE_ORDER {
        let Some(steps) = request.stages.get(stage_name).filter(|steps| !steps.is_empty()) else { continue };
        let mut stage = Stage {
            status: Outcome::Passed,
            started_at: build_machine_core::now(),
            finished_at: None,
            error: None,
            steps: Vec::new(),
        };
        for step in steps {
            let mut entry = ReportStep {
                index: step.index,
                name: step.name.clone(),
                adapter: step.adapter_name.clone(),
                status: Outcome::Passed,
                started_at: build_machine_core::now(),
                finished_at: None,
                command: None,
                exit_code: None,
                output: None,
                reason: step.reason.clone(),
                skipped: false,
                local_adapter: false,
                timeout_seconds: None,
            };
            let (enabled, limit) = condition_enabled(step)?;
            if let Some(limit) = limit {
                result.limits.push(limit);
            }
            if !enabled {
                entry.status = Outcome::PassedWithLimits;
                entry.skipped = true;
                entry.reason = Some("if condition evaluated false locally".to_owned());
                stage.status = Outcome::PassedWithLimits;
            } else {
                match step.adapter {
                    Adapter::Skip => {
                        entry.status = Outcome::PassedWithLimits;
                        stage.status = Outcome::PassedWithLimits;
                        result.limits.push(format!(
                            "{stage_name} skipped: {}",
                            step.reason.clone().unwrap_or_default()
                        ));
                    }
                    adapter if adapter.is_local_stand_in() => {
                        entry.local_adapter = true;
                    }
                    adapter if adapter.is_external_service() => {
                        entry.status = Outcome::PassedWithLimits;
                        entry.local_adapter = true;
                        stage.status = Outcome::PassedWithLimits;
                        result.limits.push(format!(
                            "{}: external GitHub service replaced by local artifact store",
                            step.name
                        ));
                    }
                    Adapter::TauriBuild => {
                        let (environment, limits) = step_environment(&base, step);
                        result.limits.extend(limits);
                        let artifacts = tauri_action(step, &source, request, &environment)?;
                        result.artifacts.extend(artifacts);
                    }
                    Adapter::Run => {
                        let (environment, limits) = step_environment(&base, step);
                        result.limits.extend(limits);
                        let command = step.run.clone().unwrap_or_default();
                        if command.trim().is_empty() {
                            bail!("Workflow step {} has an empty run command.", step.index);
                        }
                        let working = working_directory(&source, step)?;
                        let (shell, mut arguments) = shell_for();
                        arguments.push(command.clone());
                        entry.command = Some(command.clone());
                        println!("> {command}");
                        let timeout = timeout_for(stage_name);
                        let finished =
                            stream::run(shell, &arguments, Some(&working), &environment, Some(timeout))?;
                        entry.output = Some(finished.output.clone());
                        if finished.timed_out {
                            entry.status = Outcome::Timeout;
                            entry.timeout_seconds = Some(timeout.as_secs());
                            stage.status = Outcome::Failed;
                            stage.error = Some(format!("step {} timed out", step.index));
                        } else {
                            entry.exit_code = Some(finished.code);
                            if finished.code != 0 {
                                entry.status = Outcome::Failed;
                                stage.status = Outcome::Failed;
                                stage.error =
                                    Some(format!("step {} exited with {}", step.index, finished.code));
                            }
                        }
                    }
                    other => bail!("Unsupported workflow adapter: {}", other.as_str()),
                }
            }
            entry.finished_at = Some(build_machine_core::now());
            let failed = stage.status == Outcome::Failed;
            stage.steps.push(entry);
            if failed {
                break;
            }
        }
        stage.finished_at = Some(build_machine_core::now());
        let failed = stage.status == Outcome::Failed;
        let error = stage.error.clone();
        result.stages.insert(stage_name.to_owned(), stage);
        if failed {
            result.status = Outcome::Failed;
            result.success = false;
            result.error = error.or_else(|| Some(format!("{stage_name} stage failed")));
            result.finished_at = build_machine_core::now();
            return Ok(result);
        }
    }

    result.limits.sort();
    result.limits.dedup();
    // The seeded limit is always present, so a replay is never plain `passed`.
    result.status = Outcome::PassedWithLimits;
    result.success = true;
    result.finished_at = build_machine_core::now();
    Ok(result)
}
