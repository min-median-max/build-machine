//! Building a guest worker inside its own virtual machine.
//!
//! This is a development path. A release builds each worker natively on the
//! matching GitHub runner, and the guest then needs no toolchain at all — it
//! only runs the binary the release shipped.

use anyhow::{bail, Context, Result};
use base64::Engine;
use build_machine_controller::transport::prlctl_path;
use build_machine_core::config::Machine;
use build_machine_core::Platform;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The guest's own view of the share.
fn share_root(platform: Platform, share: &str) -> String {
    match platform {
        Platform::Windows => format!("\\\\Mac\\{share}"),
        _ => format!("/media/psf/{share}"),
    }
}

/// Where the guest builds. The share is read only, so the build cannot happen
/// inside it.
fn workspace(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "C:\\BuildMachine\\worker-build",
        _ => "/tmp/build-machine-worker",
    }
}

fn shell(platform: Platform, script: &str) -> Vec<String> {
    match platform {
        Platform::Windows => vec![
            "powershell.exe".to_owned(),
            "-NoLogo".to_owned(),
            "-NoProfile".to_owned(),
            "-NonInteractive".to_owned(),
            "-Command".to_owned(),
            script.to_owned(),
        ],
        _ => vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
    }
}

fn exec(prlctl: &Path, vm: &str, platform: Platform, script: &str, quiet: bool) -> Result<Vec<u8>> {
    if !quiet {
        println!("> prlctl exec {vm} …");
    }
    let output = Command::new(prlctl)
        .arg("exec")
        .arg(vm)
        .arg("--current-user")
        .args(shell(platform, script))
        .output()
        .context("prlctl exec을 실행하지 못했어요.")?;
    if !quiet {
        print!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        bail!(
            "Guest command failed with exit code {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// Compile the worker in the guest and bring the binary back.
///
/// The share the guest reads is mounted read only on purpose, so the guest
/// cannot write its build product back through it. The binary is returned as
/// base64 on the guest's own output instead, which needs no second mount and
/// no writable path into the host.
pub fn build_worker(root: &Path, machine: &Machine, platform: Platform) -> Result<PathBuf> {
    let profile = machine.profile(platform)?;
    let vm = profile.vm.clone().with_context(|| format!("machine.json에 {platform}의 vm이 없어요."))?;
    let prlctl = prlctl_path()?;
    let share = share_root(platform, &machine.share);
    let workspace = workspace(platform);
    let target = &profile.target;

    let (build_script, encode_script) = match platform {
        Platform::Windows => (
            format!(
                "$ErrorActionPreference='Stop'; \
                 if (Test-Path '{workspace}') {{ Remove-Item -Recurse -Force '{workspace}' }}; \
                 Copy-Item -Recurse '{share}' '{workspace}'; \
                 Set-Location '{workspace}'; \
                 cargo build --release --locked -p build-machine-worker --target {target}"
            ),
            format!(
                "[Convert]::ToBase64String([IO.File]::ReadAllBytes('{workspace}\\target\\{target}\\release\\build-machine-worker.exe'))"
            ),
        ),
        _ => (
            format!(
                "set -eu; rm -rf {workspace}; mkdir -p {workspace}; \
                 cp -a {share}/. {workspace}/; cd {workspace}; \
                 cargo build --release --locked -p build-machine-worker --target {target}"
            ),
            format!("base64 < {workspace}/target/{target}/release/build-machine-worker"),
        ),
    };

    exec(&prlctl, &vm, platform, &build_script, false)?;
    let encoded = exec(&prlctl, &vm, platform, &encode_script, true)?;
    let cleaned: String = String::from_utf8_lossy(&encoded).chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .context("게스트가 보낸 워커 바이너리를 읽지 못했어요.")?;
    if bytes.is_empty() {
        bail!("게스트가 빈 워커 바이너리를 보냈어요.");
    }

    let landing = root.join(".state/worker-build");
    std::fs::create_dir_all(&landing)?;
    let destination = landing.join(crate::workers::staged_name(platform));
    std::fs::write(&destination, &bytes)
        .with_context(|| format!("워커를 저장하지 못했어요: {}", destination.display()))?;
    set_executable(&destination)?;
    Ok(destination)
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
