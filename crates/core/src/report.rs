//! The run report: the single record of what an operation did.
//!
//! Defined once. The CLI, the desktop bridge and the dashboard all read this
//! type rather than indexing an untyped document, so a field cannot mean one
//! thing on the producing side and another on the consuming side.

use crate::source::Snapshot;
use crate::Platform;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Doctor,
    Setup,
    Build,
    Run,
    Release,
    Ci,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Doctor => "doctor",
            Action::Setup => "setup",
            Action::Build => "build",
            Action::Run => "run",
            Action::Release => "release",
            Action::Ci => "ci",
        }
    }

    /// Whether a record of this action belongs in the build history.
    pub fn is_build_history(&self) -> bool {
        matches!(self, Action::Build | Action::Ci)
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Success,
    PassedWithLimits,
    Failure,
}

/// A single platform's outcome. `passed_with_limits` means the work completed
/// but something was replaced by a local adapter or skipped on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    PassedWithLimits,
    Failed,
    Timeout,
    /// A workflow step that did not run: its `if` was false, or an earlier step
    /// failed and its `if` does not run after a failure.
    Skipped,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        matches!(self, Outcome::Passed | Outcome::PassedWithLimits)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    #[default]
    Sequential,
    Parallel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub run_id: String,
    pub action: Action,
    pub project: Option<String>,
    pub platforms: Vec<Platform>,
    pub execution_mode: ExecutionMode,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    pub source: Option<Snapshot>,
    pub results: BTreeMap<Platform, PlatformResult>,
    pub log: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformResult {
    pub success: bool,
    pub status: Outcome,
    pub finished_at: String,
    pub attempts: u32,
    pub log: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stages: BTreeMap<String, Stage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<String>,
    /// What `actions/deploy-pages` would have deployed. Nothing is deployed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deployments: Vec<Deployment>,
    /// The virtual disks compacted after the work, measured before and after.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disk: Vec<DiskCompaction>,
}

/// One virtual disk image compacted with its VM stopped. Allocated bytes are
/// what the image takes on the host; the apparent size is its file length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskCompaction {
    pub image: String,
    pub allocated_before: u64,
    pub allocated_after: u64,
    pub apparent_before: u64,
    pub apparent_after: u64,
}

/// A deployment a replay recorded and did not make: the job, its
/// environment, the artifact and that artifact's files.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub job: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<crate::workflow::Environment>,
    pub artifact: String,
    pub files: Vec<Artifact>,
}

impl PlatformResult {
    pub fn passed(finished_at: String, log: String) -> Self {
        Self {
            success: true,
            status: Outcome::Passed,
            finished_at,
            attempts: 1,
            log,
            error: None,
            stages: BTreeMap::new(),
            artifacts: Vec::new(),
            limits: Vec::new(),
            executable: None,
            signing: None,
            deployments: Vec::new(),
            disk: Vec::new(),
        }
    }

    pub fn failed(finished_at: String, log: String, error: String) -> Self {
        Self { success: false, status: Outcome::Failed, error: Some(error), ..Self::passed(finished_at, log) }
    }

    /// The workflow steps that reached execution. A step its `if` skipped,
    /// one a secret condition skipped and a skip marker did not.
    pub fn executed_steps(&self) -> usize {
        self.stages
            .values()
            .flat_map(|stage| &stage.steps)
            .filter(|step| step.adapter != "skip" && step.status != Outcome::Skipped && !step.skipped)
            .count()
    }

    /// A replay that executed no workflow step did not replay the workflow,
    /// so a success it reports is turned into a failure that says so.
    pub fn require_executed_steps(&mut self) {
        if self.success && self.executed_steps() == 0 {
            self.success = false;
            self.status = Outcome::Failed;
            self.error = Some(
                "이 플랫폼에서 실행된 workflow 단계가 없어요. 이 플랫폼의 job이 없거나 모든 단계가 건너뛰어졌어요.".to_owned(),
            );
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub status: Outcome,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub steps: Vec<Step>,
}

/// One workflow step. `output` is the step's interleaved stdout and stderr,
/// the order a reader needs to diagnose it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub index: u32,
    pub name: String,
    pub adapter: String,
    pub status: Outcome,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_adapter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

impl RunReport {
    /// Derive the overall status from the platform results. A run succeeds only
    /// when every requested platform reported success.
    pub fn settle(&mut self, finished_at: String) {
        let complete = self.results.len() == self.platforms.len();
        let all_passed = self.platforms.iter().all(|platform| {
            self.results.get(platform).map(|result| result.success).unwrap_or(false)
        });
        let success = self.error.is_none() && complete && all_passed;
        let limited = success
            && self.results.values().any(|result| result.status == Outcome::PassedWithLimits);
        self.status = if limited {
            RunStatus::PassedWithLimits
        } else if success {
            RunStatus::Success
        } else {
            RunStatus::Failure
        };
        self.finished_at = Some(finished_at);
    }

    pub fn succeeded(&self) -> bool {
        matches!(self.status, RunStatus::Success | RunStatus::PassedWithLimits)
    }
}

/// Write a document so a reader never observes a partial file.
pub fn write_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("폴더를 만들지 못했어요: {}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    let partial = path.with_extension(format!(
        "{}.partial",
        path.extension().and_then(|value| value.to_str()).unwrap_or("json")
    ));
    std::fs::write(&partial, text)
        .with_context(|| format!("파일을 쓰지 못했어요: {}", partial.display()))?;
    std::fs::rename(&partial, path)
        .with_context(|| format!("파일을 바꾸지 못했어요: {}", path.display()))?;
    Ok(())
}
