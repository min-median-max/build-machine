//! The controller: what the CLI and the desktop application are both built on.
//!
//! It prepares one source snapshot, drives the selected environments, records
//! a single run report and bounds what it keeps. It runs only on macOS, which
//! is where the virtual machines and the desktop application live.

pub mod matrix;
pub mod oplog;
pub mod snapshot;
pub mod transport;

use anyhow::{bail, Context, Result};
use build_machine_core::config::Machine;
use build_machine_core::report::{Action, ExecutionMode};
use build_machine_core::Platform;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Everything a run needs to know before it starts.
pub struct Operation {
    pub root: PathBuf,
    pub machine: Machine,
    pub action: Action,
    pub platforms: Vec<Platform>,
    pub execution: ExecutionMode,
    pub project: Option<PathBuf>,
    pub framework: Option<build_machine_core::request::Framework>,
    pub command: Option<String>,
    pub artifact: Option<String>,
    pub launch: bool,
    pub workflow: Option<String>,
    pub event: String,
    pub reference: Option<String>,
    pub result_file: Option<PathBuf>,
    /// Where each produced line goes, besides the log file. The command line
    /// prints them; the desktop application streams them to its log panel.
    pub observer: Option<oplog::Observer>,
}

/// An observer that prints to this process, for the command line.
pub fn printing_observer() -> oplog::Observer {
    Arc::new(|stream, line| match stream {
        oplog::Stream::Stderr => eprintln!("{line}"),
        oplog::Stream::Stdout => println!("{line}"),
    })
}

/// The controller directory: the one that holds `machine.json`.
pub fn controller_root(value: &Path) -> Result<PathBuf> {
    let root = value
        .canonicalize()
        .with_context(|| format!("빌드 도구 폴더를 찾을 수 없어요: {}", value.display()))?;
    if !root.join("machine.json").is_file() {
        bail!("선택한 폴더에 machine.json이 없어요.");
    }
    Ok(root)
}

/// Find the controller directory from the running executable, so the desktop
/// application and the command line agree on where state lives.
pub fn find_controller() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    for ancestor in exe.ancestors() {
        if ancestor.join("machine.json").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Work/build-machine"))
}

pub fn state_directory(root: &Path) -> PathBuf {
    root.join(".state")
}

/// Serialize guest work and shared transfer state across every front end.
pub struct Lock {
    _file: std::fs::File,
}

impl Lock {
    pub fn acquire(state: &Path) -> Result<Lock> {
        std::fs::create_dir_all(state)?;
        let file = std::fs::File::create(state.join("machine.lock"))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let result = unsafe { flock(file.as_raw_fd(), 2 | 4) };
            if result != 0 {
                bail!("Another build-machine command is running. Wait for its result.");
            }
        }
        Ok(Lock { _file: file })
    }
}

#[cfg(unix)]
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
