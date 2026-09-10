//! `machine.json`: the declared tool versions, VM names and retention policy.

use crate::Platform;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Machine {
    pub vm: String,
    pub share: String,
    #[serde(rename = "windowsRoot")]
    pub windows_root: String,
    pub architecture: String,
    pub node: Pinned,
    pub pnpm: Version,
    pub rust: Rust,
    pub go: Pinned,
    pub git: Pinned,
    pub msvc: Msvc,
    #[serde(default)]
    pub retention: Retention,
    pub platforms: BTreeMap<String, Profile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Version {
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pinned {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rust {
    pub version: String,
    pub host: String,
    #[serde(rename = "installerUrl")]
    pub installer_url: String,
    #[serde(rename = "installerSha256")]
    pub installer_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Msvc {
    #[serde(rename = "installPath")]
    pub install_path: String,
    #[serde(rename = "installerUrl")]
    pub installer_url: String,
    pub components: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Retention {
    #[serde(rename = "maxRunsPerProject")]
    pub max_runs_per_project: u32,
    #[serde(rename = "maxBytes")]
    pub max_bytes: u64,
    pub days: i64,
}

impl Default for Retention {
    fn default() -> Self {
        Self { max_runs_per_project: 20, max_bytes: 20 * 1024 * 1024 * 1024, days: 30 }
    }
}

/// A per-platform profile. `vm` is absent for macOS, which runs on the host.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    #[serde(default)]
    pub vm: Option<String>,
    pub runner: String,
    pub target: String,
    #[serde(default)]
    pub bundle: Option<String>,
    #[serde(default, rename = "nodeUrl")]
    pub node_url: Option<String>,
    #[serde(default, rename = "nodeSha256")]
    pub node_sha256: Option<String>,
    #[serde(default, rename = "goUrl")]
    pub go_url: Option<String>,
    #[serde(default, rename = "goSha256")]
    pub go_sha256: Option<String>,
    #[serde(default, rename = "rustupUrl")]
    pub rustup_url: Option<String>,
    #[serde(default, rename = "rustupSha256")]
    pub rustup_sha256: Option<String>,
    #[serde(default)]
    pub packages: Vec<String>,
}

impl Machine {
    pub fn load(path: &Path) -> Result<Machine> {
        let data = std::fs::read(path)
            .with_context(|| format!("machine.json을 읽지 못했어요: {}", path.display()))?;
        serde_json::from_slice(&data)
            .with_context(|| format!("machine.json 형식이 올바르지 않아요: {}", path.display()))
    }

    pub fn profile(&self, platform: Platform) -> Result<&Profile> {
        self.platforms
            .get(platform.as_str())
            .with_context(|| format!("machine.json에 {platform}의 설정이 없어요."))
    }
}
