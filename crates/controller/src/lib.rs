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
///
/// This finds the workspace a developer is running from. A released
/// application carries its payload instead and calls [`seed_root`].
pub fn find_controller() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|ancestor| ancestor.join("machine.json").is_file()).map(Path::to_path_buf)
}

/// Prepare a writable controller root from a read-only payload.
///
/// A released application ships its machine definition and its worker binaries
/// inside the bundle, which cannot be written to and cannot be shared into a
/// virtual machine. They are placed once into a directory that can be both, and
/// a payload that is already there is not replaced — the run records and source
/// archives beside it belong to the person using it.
pub fn seed_root(payload: &Path, root: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let definition = root.join("machine.json");
    if !definition.is_file() {
        std::fs::copy(payload.join("machine.json"), &definition)
            .context("머신 정의를 배치하지 못했어요.")?;
    }
    let workers = root.join("workers");
    std::fs::create_dir_all(&workers)?;
    for entry in std::fs::read_dir(payload.join("workers")).context("번들에 워커가 없어요.")? {
        let entry = entry?;
        let destination = workers.join(entry.file_name());
        // A worker is replaced whenever the bundle carries a different one, so
        // updating the application updates what the guests run.
        let same = std::fs::metadata(&destination)
            .ok()
            .zip(entry.metadata().ok())
            .is_some_and(|(there, here)| there.len() == here.len());
        if !same {
            std::fs::copy(entry.path(), &destination)?;
            set_executable(&destination)?;
        }
    }
    Ok(root.to_path_buf())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
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
