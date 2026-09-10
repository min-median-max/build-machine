//! Grouping steps into stages, and the gates a workflow must satisfy.

use super::adapter::{stage_for_shell, Adapter, STAGE_ORDER};
use super::parse::{Step, Workflow};
use crate::Platform;
use anyhow::{bail, Result};
use std::collections::BTreeMap;

/// The stage a step belongs to. An action is decided by its adapter; only a
/// shell step is read from its own text.
pub fn stage_of(step: &Step) -> &'static str {
    if let Some(stage) = step.adapter.stage() {
        return stage;
    }
    stage_for_shell(&step.name, step.run.as_deref().unwrap_or_default())
}

/// Group every step by stage, inserting a marker for a stage the workflow
/// documents as deliberately absent.
pub fn stages(workflow: &Workflow) -> BTreeMap<String, Vec<Step>> {
    stages_for(workflow, None)
}

/// The stages to run on one operating system.
///
/// A three-OS release workflow has a job per operating system, and `runs-on`
/// says which is which. Replaying every job everywhere would run the Windows
/// job on Linux, so a platform takes only the jobs written for it — plus any
/// job whose runner this machine does not recognise, which constrains nothing.
pub fn stages_for(workflow: &Workflow, platform: Option<Platform>) -> BTreeMap<String, Vec<Step>> {
    let mut grouped: BTreeMap<String, Vec<Step>> =
        STAGE_ORDER.iter().map(|stage| ((*stage).to_owned(), Vec::new())).collect();
    for job in &workflow.jobs {
        if let (Some(platform), Some(declared)) = (platform, super::platform_for_runner(&job.runs_on)) {
            if declared != platform {
                continue;
            }
        }
        for step in &job.steps {
            grouped.entry(stage_of(step).to_owned()).or_default().push(step.clone());
        }
    }
    for stage in ["test", "smoke"] {
        if grouped.get(stage).is_some_and(|steps| steps.is_empty()) {
            if let Some(reason) = workflow.skips.get(stage) {
                let mut marker = Step {
                    index: 0,
                    name: format!("skip {stage}"),
                    adapter: Adapter::Skip,
                    action: None,
                    action_ref: None,
                    run: None,
                    working_directory: None,
                    condition: None,
                    reason: Some(reason.clone()),
                    env: BTreeMap::new(),
                    with: BTreeMap::new(),
                    job_env: BTreeMap::new(),
                    job_id: String::new(),
                };
                marker.reason = Some(reason.clone());
                grouped.insert(stage.to_owned(), vec![marker]);
            }
        }
    }
    grouped
}

/// A workflow must build, and must either test and smoke or say why it does not.
///
/// Each operating system the workflow has a job for is checked on its own. A
/// workflow that tests on macOS and not on Linux would otherwise pass while
/// the Linux rehearsal quietly ran no tests at all.
pub fn check_gates(workflow: &Workflow) -> Result<()> {
    let declared = super::declared_platforms(workflow);
    if declared.is_empty() {
        return check_gates_for(workflow, None);
    }
    for platform in declared {
        check_gates_for(workflow, Some(platform))?;
    }
    Ok(())
}

fn check_gates_for(workflow: &Workflow, platform: Option<Platform>) -> Result<()> {
    let grouped = stages_for(workflow, platform);
    let where_ = platform.map(|value| format!("{value}에 ")).unwrap_or_default();
    if grouped.get("build").is_none_or(|steps| steps.is_empty()) {
        bail!("{where_}build 단계가 없어요. 지원하는 build action 또는 build 명령을 workflow에 추가해야 해요.");
    }
    for stage in ["test", "smoke"] {
        let present = grouped.get(stage).is_some_and(|steps| !steps.is_empty());
        if !present && !workflow.skips.contains_key(stage) {
            bail!("{where_}{stage} 단계가 없어요. 워크플로에 '# build-machine: skip {stage} reason=...' 주석을 추가해야 해요.");
        }
    }
    Ok(())
}

/// How many steps each stage holds, for the run record.
pub fn stage_counts(workflow: &Workflow) -> BTreeMap<String, usize> {
    stages(workflow).into_iter().map(|(stage, steps)| (stage, steps.len())).collect()
}
