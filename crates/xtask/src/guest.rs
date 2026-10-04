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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How long a guest has to answer a short query such as its home directory.
/// A guest whose tools do not answer `prlctl exec` stops the build here.
const QUERY_LIMIT: Duration = Duration::from_secs(60);
/// How long the guest may take to build the worker.
const BUILD_LIMIT: Duration = Duration::from_secs(60 * 60);

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
    /// The account the toolchain belongs to.
    user: Option<String>,
}

impl Guest {
    /// Run one command in the guest and return its output.
    ///
    /// Building needs the desktop user's toolchain but not their desktop
    /// session, so on a machine that has one this becomes the user directly.
    /// A machine sitting at its login screen has no session to attach to, and
    /// waiting for someone to sign in is not a build step.
    fn run(&self, arguments: &[&str], echo: bool, limit: Duration) -> Result<String> {
        if echo {
            println!("> prlctl exec {} {}", self.vm, arguments.join(" "));
        }
        let mut command = Command::new(&self.prlctl);
        command.arg("exec").arg(&self.vm);
        match (self.platform, &self.user) {
            (Platform::Windows, _) | (_, None) => {
                command.arg("--current-user");
            }
            (_, Some(user)) => {
                command.args(["runuser", "-u", user, "--"]);
            }
        }
        command.args(arguments);
        let output = bounded_output(command, limit).map_err(|error| {
            anyhow::anyhow!("{} {}: {error} {}", self.vm, arguments.join(" "), self.state())
        })?;
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

    /// The virtual machine's state and guest tools as `prlctl list -i` reports
    /// them, for an error about a guest that did not answer.
    fn state(&self) -> String {
        match Command::new(&self.prlctl).args(["list", "-i", &self.vm]).output() {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| line.starts_with("State:") || line.starts_with("GuestTools:"))
                    .collect();
                format!("({})", lines.join(", "))
            }
            Err(error) => format!("(prlctl list -i failed: {error})"),
        }
    }

    /// The signed-in user's home directory, which is where the build writes.
    fn home(&self) -> Result<String> {
        let raw = match self.platform {
            Platform::Windows => {
                self.run(&["powershell.exe", "-NoLogo", "-NoProfile", "-Command", "$env:USERPROFILE"], false, QUERY_LIMIT)?
            }
            _ => self.run(&["printenv", "HOME"], false, QUERY_LIMIT)?,
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
    let guest = Guest { prlctl: prlctl_path()?, vm, platform, user: profile.desktop_user.clone() };
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
        BUILD_LIMIT,
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
            QUERY_LIMIT,
        )?,
        _ => guest.run(&["base64", &output], false, QUERY_LIMIT)?,
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

/// Run `command` and collect its output, ending it when it has not finished
/// within `limit`. The output is read until both pipes close, which happens
/// when the process ends, so no timer decides when it is done.
fn bounded_output(mut command: Command, limit: Duration) -> Result<Output> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().with_context(|| format!("{command:?}을 실행하지 못했어요."))?;
    let (sender, receiver) = mpsc::channel();
    let mut readers = Vec::new();
    for (index, mut pipe) in [
        Box::new(child.stdout.take().context("stdout이 없어요.")?) as Box<dyn Read + Send>,
        Box::new(child.stderr.take().context("stderr가 없어요.")?) as Box<dyn Read + Send>,
    ]
    .into_iter()
    .enumerate()
    {
        let sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
            // 받는 쪽이 시간을 넘겨 끝났으면 보낼 곳이 없다. 그 경우 오류는 이미 보고됐다.
            let _ = sender.send((index, result));
        }));
    }
    drop(sender);
    let deadline = std::time::Instant::now() + limit;
    let mut collected: [Option<Vec<u8>>; 2] = [None, None];
    while collected.iter().any(Option::is_none) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok((index, result)) => collected[index] = Some(result.context("출력을 읽지 못했어요.")?),
            Err(_) => {
                child.kill().context("시간을 넘긴 명령을 끝내지 못했어요.")?;
                child.wait()?;
                bail!("{}초 안에 끝나지 않았어요", limit.as_secs_f64());
            }
        }
    }
    for reader in readers {
        reader.join().map_err(|_| anyhow::anyhow!("출력을 읽는 스레드가 실패했어요."))?;
    }
    let status = child.wait()?;
    let [Some(stdout), Some(stderr)] = collected else {
        bail!("명령의 출력을 모두 받지 못했어요.");
    };
    Ok(Output { status, stdout, stderr })
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

#[cfg(test)]
mod tests {
    use super::bounded_output;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// A guest that does not answer `prlctl exec` held `cargo xtask worker`
    /// for 25 minutes. The command is ended at its limit and the error names
    /// the limit.
    #[test]
    fn a_command_past_its_limit_is_ended_and_named() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_millis(300)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        assert!(error.to_string().contains("0.3"), "{error}");
    }

    #[test]
    fn a_command_within_its_limit_returns_its_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf out; printf err >&2; exit 3"]);
        let output = bounded_output(command, Duration::from_secs(10)).unwrap();
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
        assert_eq!(output.status.code(), Some(3));
    }
}
