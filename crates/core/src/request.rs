//! What the controller asks a worker to do.
//!
//! The controller writes this document; the worker reads it. Both sides use
//! this one definition, so a field cannot drift between them.

use crate::source::Snapshot;
use crate::workflow::Step;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    Tauri,
    Wails2,
    Custom,
}

impl std::str::FromStr for Framework {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "tauri" => Framework::Tauri,
            "wails2" => Framework::Wails2,
            "custom" => Framework::Custom,
            other => anyhow::bail!("Unknown framework: {other}"),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkRequest {
    pub snapshot: Snapshot,
    /// Where the worker can read the source archive from, in its own namespace.
    pub archive: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<Framework>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// Present only for a workflow replay.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stages: BTreeMap<String, Vec<Step>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_signature: Option<String>,
}

impl WorkRequest {
    pub fn load(path: &Path) -> Result<WorkRequest> {
        let data = std::fs::read(path)
            .with_context(|| format!("작업 요청을 읽지 못했어요: {}", path.display()))?;
        serde_json::from_slice(&data)
            .with_context(|| format!("작업 요청 형식이 올바르지 않아요: {}", path.display()))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        crate::report::write_atomic(path, self)
    }

    /// The project directory name a worker uses. Validated because it becomes a
    /// path segment in the worker's own workspace.
    pub fn project_key(&self) -> Result<&str> {
        let key = self.snapshot.project_key.as_str();
        let valid = !key.is_empty()
            && key.chars().all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'));
        anyhow::ensure!(valid, "Invalid project key.");
        Ok(key)
    }
}
