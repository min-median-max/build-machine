//! Launching the last successful build in the signed-in desktop.
//!
//! Repeating this reuses the running process rather than starting a second
//! copy. A process and window check verifies the launch; it does not certify
//! that the application's features work.

use crate::build::Receipt;
use crate::provision::Tools;
use anyhow::{bail, Context, Result};
use build_machine_core::source::sha256_file;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(target_os = "macos"))]
use std::process::Stdio;
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launched {
    pub executable: String,
    pub process_id: u32,
    pub reused_process: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responding: Option<bool>,
    pub verification: String,
}

pub fn latest_receipt(root: &Path) -> Result<Receipt> {
    let path = root.join("latest.json");
    let data = std::fs::read(&path)
        .context("No successful build exists for this project. Run build first.")?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn launch(receipt: &Receipt, tools: &Tools) -> Result<Launched> {
    let executable = PathBuf::from(&receipt.executable);
    if !executable.exists() || sha256_file(&executable)? != receipt.executable_sha256 {
        bail!("Executable is missing or differs from the build receipt.");
    }
    let existing = running_process(&executable);
    if existing.is_none() {
        start(receipt, &executable, tools)?;
    }
    let mut process_id = existing;
    for _ in 0..20 {
        if process_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
        process_id = running_process(&executable);
    }
    let Some(process_id) = process_id else {
        bail!("No running process was found for the built application.");
    };
    std::thread::sleep(Duration::from_secs(3));
    if running_process(&executable) != Some(process_id) {
        bail!("The application exited during its startup check. Inspect launch.log beside the executable.");
    }
    let (window_title, responding) = window_state(process_id);
    Ok(Launched {
        executable: receipt.executable.clone(),
        process_id,
        reused_process: existing.is_some(),
        window_title,
        responding,
        verification: "process running; visible rendering requires the recorded platform screen check"
            .to_owned(),
    })
}

#[cfg(target_os = "macos")]
fn start(receipt: &Receipt, _executable: &Path, tools: &Tools) -> Result<()> {
    let app = receipt.app.clone().context("macOS launch requires the built application bundle.")?;
    crate::stream::checked("open", &["-n".to_owned(), app], None, &tools.environment)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn start(_receipt: &Receipt, executable: &Path, tools: &Tools) -> Result<()> {
    let environment = desktop_environment(tools);
    let has_display = environment
        .iter()
        .any(|(key, value)| (key == "DISPLAY" || key == "WAYLAND_DISPLAY") && !value.is_empty());
    if !has_display {
        bail!("No Linux desktop display is available to the signed-in user.");
    }
    let log = executable.parent().unwrap_or(Path::new(".")).join("launch.log");
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&log)?;
    let errors = file.try_clone()?;
    let mut command = Command::new(executable);
    command.stdin(Stdio::null()).stdout(Stdio::from(file)).stderr(Stdio::from(errors));
    for (key, value) in &environment {
        command.env(key, value);
    }
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
    command.spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
extern "C" {
    fn setsid() -> i32;
}

/// The launch reads only the desktop connection settings from the signed-in
/// user's own session, not that session's whole environment.
#[cfg(target_os = "linux")]
fn desktop_environment(tools: &Tools) -> Vec<(String, String)> {
    const ALLOWED: [&str; 6] = [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_SESSION_TYPE",
    ];
    let mut environment = tools.environment.clone();
    if let Ok(output) = crate::stream::capture(
        "systemctl",
        &["--user".to_owned(), "show-environment".to_owned()],
        &tools.environment,
    ) {
        for line in output.lines() {
            if let Some((key, value)) = line.split_once('=') {
                if ALLOWED.contains(&key) {
                    environment.retain(|(existing, _)| existing != key);
                    environment.push((key.to_owned(), value.to_owned()));
                }
            }
        }
    }
    environment
}

/// Launch the application so it outlives this worker.
///
/// The controller reaches the guest through `prlctl exec`, which does not
/// return until the process tree it started has emptied. A launched
/// application is meant to keep running, so it must not be in that tree.
/// Creating it detached is not enough — it stays in the job object the remote
/// execution placed it in — so the shell is asked to start it, exactly as a
/// person double-clicking it would. The shell already runs outside that job.
///
/// Linux reaches the same place with `setsid`.
#[cfg(target_os = "windows")]
fn start(_receipt: &Receipt, executable: &Path, tools: &Tools) -> Result<()> {
    let mut command = Command::new("explorer.exe");
    command.arg(executable).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for (key, value) in &tools.environment {
        command.env(key, value);
    }
    // `explorer.exe` hands off and exits, so its own status says nothing about
    // the application. The process check below is what establishes the launch.
    let _ = command.status();
    Ok(())
}

/// Match on the full executable path so a similarly named process elsewhere is
/// never mistaken for this build.
#[cfg(target_os = "linux")]
fn running_process(executable: &Path) -> Option<u32> {
    let wanted = executable.canonicalize().ok()?;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(pid) = name.parse::<u32>() else { continue };
        if std::fs::read_link(entry.path().join("exe")).is_ok_and(|path| path == wanted) {
            return Some(pid);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn running_process(executable: &Path) -> Option<u32> {
    let wanted = executable.canonicalize().ok()?;
    let output = Command::new("ps").args(["-axww", "-o", "pid=,comm="]).output().ok()?;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        let Some((pid, image)) = line.split_once(char::is_whitespace) else { continue };
        if Path::new(image.trim()).canonicalize().is_ok_and(|path| path == wanted) {
            return pid.parse().ok();
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn running_process(executable: &Path) -> Option<u32> {
    crate::win32::process_with_image(executable)
}

#[cfg(target_os = "windows")]
fn window_state(pid: u32) -> (Option<String>, Option<bool>) {
    match crate::win32::main_window(pid) {
        Some((handle, title)) => (Some(title), Some(crate::win32::window_responding(handle))),
        None => (None, None),
    }
}

#[cfg(not(target_os = "windows"))]
fn window_state(_pid: u32) -> (Option<String>, Option<bool>) {
    (None, None)
}
