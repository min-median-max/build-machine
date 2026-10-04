//! What the controller asks a worker to do.
//!
//! The controller writes this document; the worker reads it. Both sides use
//! this one definition, so a field cannot drift between them.

use crate::source::Snapshot;
use crate::workflow::Job;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The identity of the request and report this build exchanges: the SHA-256
/// of this crate's sources, computed by `build.rs`. A controller and a worker
/// built from different sources have different identities, so neither
/// silently reads the other's documents.
pub const PROTOCOL: &str = env!("BUILD_MACHINE_PROTOCOL");

/// What to do when the other side has another protocol.
pub fn rebuild_hint() -> &'static str {
    "워커를 현재 소스로 다시 빌드해야 해요: cargo xtask worker --os <os>"
}

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
    /// The controller's `PROTOCOL`.
    pub protocol: String,
    pub snapshot: Snapshot,
    /// Where the worker can read the source archive from, in its own namespace.
    pub archive: String,
    /// Where the worker can read the replay's Git history from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    /// Where the worker can read each other repository's history from, by
    /// the key of `snapshot.repositories`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repositories: BTreeMap<String, String>,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<Framework>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// The jobs a workflow replay runs on this platform, in `needs` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<Job>,
    /// Stages the workflow documents as deliberately absent, with the reason.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skips: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_signature: Option<String>,
}

impl WorkRequest {
    /// Read a request, refusing one written for another protocol before its
    /// shape is read.
    pub fn load(path: &Path) -> Result<WorkRequest> {
        let data = std::fs::read(path)
            .with_context(|| format!("작업 요청을 읽지 못했어요: {}", path.display()))?;
        let document: serde_json::Value = serde_json::from_slice(&data)
            .with_context(|| format!("작업 요청 형식이 올바르지 않아요: {}", path.display()))?;
        let protocol = document.get("protocol").and_then(|value| value.as_str()).unwrap_or("없음");
        if protocol != PROTOCOL {
            anyhow::bail!(
                "작업 요청의 protocol {protocol}이 이 워커의 protocol {PROTOCOL}과 달라요. {}",
                rebuild_hint()
            );
        }
        serde_json::from_value(document)
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
