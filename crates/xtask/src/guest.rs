//! Building a guest worker inside its own virtual machine.
//!
//! This is a development path. A release builds each worker natively on the
//! matching GitHub runner, and the guest then needs no toolchain at all — it
//! only runs the binary the release shipped.
//!
//! `prlctl exec` does not preserve argument quoting: it joins what it is given
//! and the guest parses the result, so a `;` or a pipe would be split apart.
//! Every step here is therefore one plain command with plain arguments, which
//! also means no shell script has to be generated or left behind.

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

struct Guest {
    prlctl: PathBuf,
    vm: String,
    platform: Platform,
}

impl Guest {
    /// Run one command in the guest and return its output.
    fn run(&self, arguments: &[&str], echo: bool) -> Result<String> {
        if echo {
            println!("> prlctl exec {} {}", self.vm, arguments.join(" "));
        }
        let output = Command::new(&self.prlctl)
            .arg("exec")
            .arg(&self.vm)
            .arg("--current-user")
            .args(arguments)
            .output()
            .context("prlctl exec을 실행하지 못했어요.")?;
        if echo {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.status.success() {
            bail!(
                "Guest command failed with exit code {}: {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The signed-in user's home directory, which is where the build writes.
    fn home(&self) -> Result<String> {
        let raw = match self.platform {
            Platform::Windows => {
                self.run(&["powershell.exe", "-NoLogo", "-NoProfile", "-Command", "$env:USERPROFILE"], false)?
            }
            _ => self.run(&["printenv", "HOME"], false)?,
        };
        let home = raw.trim().to_owned();
        if home.is_empty() {
            bail!("게스트의 홈 디렉터리를 확인하지 못했어요.");
        }
        Ok(home)
    }
}

/// Compile the worker in the guest and bring the binary back.
///
/// Cargo reads the workspace straight from the read-only share and writes only
/// into the target directory, so nothing is copied and the guest needs no
/// writable path into the host. The binary is returned as base64 on the
/// guest's own output.
pub fn build_worker(root: &Path, machine: &Machine, platform: Platform) -> Result<PathBuf> {
    let profile = machine.profile(platform)?;
    let vm = profile.vm.clone().with_context(|| format!("machine.json에 {platform}의 vm이 없어요."))?;
    let guest = Guest { prlctl: prlctl_path()?, vm, platform };
    let share = share_root(platform, &machine.share);
    let home = guest.home()?;
    let target = &profile.target;

    let (separator, cargo, manifest, target_dir, produced) = match platform {
        Platform::Windows => (
            "\\",
            format!("{home}\\.cargo\\bin\\cargo.exe"),
            format!("{share}\\Cargo.toml"),
            format!("{home}\\.cache\\build-machine-worker"),
            "build-machine-worker.exe",
        ),
        _ => (
            "/",
            format!("{home}/.cargo/bin/cargo"),
            format!("{share}/Cargo.toml"),
            format!("{home}/.cache/build-machine-worker"),
            "build-machine-worker",
        ),
    };

    guest.run(
        &[
            &cargo,
            "build",
            "--release",
            "--locked",
            "-p",
            "build-machine-worker",
            "--target",
            target,
            "--manifest-path",
            &manifest,
            "--target-dir",
            &target_dir,
        ],
        true,
    )?;

    let output = [target_dir.as_str(), target, "release", produced].join(separator);
    let encoded = match platform {
        Platform::Windows => guest.run(
            &[
                "powershell.exe",
                "-NoLogo",
                "-NoProfile",
                "-Command",
                &format!("[Convert]::ToBase64String([IO.File]::ReadAllBytes('{output}'))"),
            ],
            false,
        )?,
        _ => guest.run(&["base64", &output], false)?,
    };

    let cleaned: String = encoded.chars().filter(|character| !character.is_whitespace()).collect();
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
    println!("Built in {}: {} bytes", guest.vm, bytes.len());
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
