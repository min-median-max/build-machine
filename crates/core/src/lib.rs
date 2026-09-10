//! Domain types shared by the controller and the native worker.
//!
//! This crate owns the machine configuration, the run report schema, source
//! snapshots, the workflow contract and the retention policy. It knows nothing
//! about processes or virtual machines, so both front ends agree on one
//! definition of every value that crosses between them.

pub mod config;
pub mod report;
pub mod request;
pub mod retention;
pub mod source;
pub mod workflow;

use chrono::{DateTime, SecondsFormat, Utc};

/// The timestamp format every recorded field uses.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub fn format_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// The three environments this machine drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Windows, Platform::Linux, Platform::Macos];

    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::Macos => "macos",
        }
    }

    /// The platform this binary was compiled for.
    pub fn host() -> anyhow::Result<Platform> {
        if cfg!(target_os = "macos") {
            Ok(Platform::Macos)
        } else if cfg!(target_os = "linux") {
            Ok(Platform::Linux)
        } else if cfg!(target_os = "windows") {
            Ok(Platform::Windows)
        } else {
            anyhow::bail!("This worker supports macOS, Linux and Windows only.")
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "windows" => Ok(Platform::Windows),
            "linux" => Ok(Platform::Linux),
            "macos" => Ok(Platform::Macos),
            other => anyhow::bail!("Unknown platform: {other}"),
        }
    }
}
