//! Reaching a worker.
//!
//! macOS runs its worker directly; Windows and Linux run theirs inside a
//! Parallels virtual machine. Only the way the command is issued differs, so
//! that is the only thing this abstracts.

use crate::oplog::{OperationLog, Stream};
use anyhow::{bail, Context, Result};
use build_machine_core::config::Machine;
use build_machine_core::Platform;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where a worker binary and the files it reads live, in that worker's own
/// namespace.
pub struct Placement {
    pub worker: String,
    pub config: String,
}

pub trait Transport {
    /// Check the environment can accept work before anything is sent to it.
    fn prepare(&self) -> Result<()>;
    /// Translate a path under the controller root into the worker's namespace.
    fn locate(&self, path: &Path) -> Result<String>;
    fn placement(&self) -> Result<Placement>;
    /// Run the worker as the signed-in desktop user.
    fn invoke(&self, arguments: &[String], log: &OperationLog) -> Result<String>;
    /// Run the worker with the rights system-wide installation needs.
    ///
    /// Machine-wide prerequisites — MSVC, WebView2, apt packages — cannot be
    /// installed by the desktop user, while builds and launches must run as
    /// that user to reach their session. Only this step is elevated.
    fn invoke_elevated(&self, arguments: &[String], log: &OperationLog) -> Result<String>;
}

/// The macOS worker, which runs on this machine.
pub struct Local {
    pub root: PathBuf,
    pub worker: PathBuf,
}

impl Transport for Local {
    fn prepare(&self) -> Result<()> {
        if !self.worker.is_file() {
            bail!("macOS 워커를 찾지 못했어요: {}", self.worker.display());
        }
        Ok(())
    }

    fn locate(&self, path: &Path) -> Result<String> {
        Ok(path.to_string_lossy().into_owned())
    }

    fn placement(&self) -> Result<Placement> {
        Ok(Placement {
            worker: self.worker.to_string_lossy().into_owned(),
            config: self.root.join("machine.json").to_string_lossy().into_owned(),
        })
    }

    fn invoke(&self, arguments: &[String], log: &OperationLog) -> Result<String> {
        let placement = self.placement()?;
        let mut command = Command::new(&placement.worker);
        command.args(arguments).arg("--config").arg(&placement.config);
        stream_command(command, log)
    }

    /// The macOS host has no separate elevated channel: Apple's own installer
    /// asks for authorization when the worker opens it.
    fn invoke_elevated(&self, arguments: &[String], log: &OperationLog) -> Result<String> {
        self.invoke(arguments, log)
    }
}

/// A worker inside a Parallels virtual machine, reached through `prlctl exec`.
pub struct Parallels {
    pub root: PathBuf,
    pub platform: Platform,
    pub vm: String,
    pub share: String,
    pub worker_source: PathBuf,
    pub prlctl: PathBuf,
}

impl Parallels {
    pub fn new(root: PathBuf, machine: &Machine, platform: Platform, worker_source: PathBuf) -> Result<Parallels> {
        let profile = machine.profile(platform)?;
        let vm = profile.vm.clone().with_context(|| format!("machine.json에 {platform}의 vm이 없어요."))?;
        Ok(Parallels { root, platform, vm, share: machine.share.clone(), worker_source, prlctl: prlctl_path()? })
    }

    /// The share as the guest sees it.
    fn share_root(&self) -> String {
        match self.platform {
            Platform::Windows => format!("\\\\Mac\\{}", self.share),
            _ => format!("/media/psf/{}", self.share),
        }
    }

    fn guest_path(&self, relative: &str) -> String {
        match self.platform {
            Platform::Windows => format!("{}\\{}", self.share_root(), relative.replace('/', "\\")),
            _ => format!("{}/{}", self.share_root(), relative),
        }
    }

    /// Without `--current-user`, `prlctl exec` runs as the guest's own
    /// privileged account: SYSTEM on Windows, root on Linux.
    fn exec(&self, arguments: &[String], log: &OperationLog, as_user: bool) -> Result<String> {
        let placement = self.placement()?;
        let mut command = Command::new(&self.prlctl);
        command.arg("exec").arg(&self.vm);
        if as_user {
            command.arg("--current-user");
        }
        command.arg(&placement.worker).args(arguments).arg("--config").arg(&placement.config);
        stream_command(command, log)
    }

    fn cli(&self, arguments: &[&str]) -> Result<String> {
        let output = Command::new(&self.prlctl)
            .args(arguments)
            .output()
            .context("Parallels prlctl을 실행하지 못했어요.")?;
        if !output.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stdout).trim().to_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Publish the read-only share the guest reads its worker and sources from.
    fn ensure_share(&self) -> Result<()> {
        let raw = self.cli(&["list", "-i", "--json", &self.vm])?;
        let listing: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
        let info = listing.first().context("Parallels 응답이 비어 있어요.")?;
        if info["State"].as_str() != Some("running") {
            bail!("Start the {} VM and sign in to its desktop before running a build.", self.platform);
        }
        if info["GuestTools"]["state"].as_str() != Some("installed") {
            bail!("Install Parallels Tools in the {} VM before using the build machine.", self.platform);
        }
        let folders = &info["Host Shared Folders"];
        let existing = &folders[&self.share];
        let root = self.root.to_string_lossy().into_owned();
        if existing.is_object() {
            let path = existing["path"].as_str().unwrap_or_default();
            if Path::new(path).canonicalize().ok() != Path::new(&root).canonicalize().ok() {
                bail!("The named build-machine share already belongs to another directory.");
            }
            if existing["mode"].as_str() != Some("ro") || existing["enabled"].as_bool() != Some(true) {
                self.cli(&["set", &self.vm, "--shf-host-set", &self.share, "--mode", "ro", "--enable"])?;
            }
        } else {
            self.cli(&["set", &self.vm, "--shf-host-add", &self.share, "--path", &root, "--mode", "ro"])?;
        }
        if folders["enabled"].as_bool() != Some(true) {
            self.cli(&["set", &self.vm, "--shf-host", "on"])?;
        }
        Ok(())
    }
}

impl Transport for Parallels {
    fn prepare(&self) -> Result<()> {
        if !self.worker_source.is_file() {
            bail!("{} 워커를 찾지 못했어요: {}", self.platform, self.worker_source.display());
        }
        self.ensure_share()
    }

    fn locate(&self, path: &Path) -> Result<String> {
        let relative = path
            .strip_prefix(&self.root)
            .with_context(|| format!("공유 폴더 밖의 경로예요: {}", path.display()))?;
        Ok(self.guest_path(&relative.to_string_lossy()))
    }

    fn placement(&self) -> Result<Placement> {
        Ok(Placement {
            worker: self.locate(&self.worker_source)?,
            config: self.guest_path("machine.json"),
        })
    }

    fn invoke(&self, arguments: &[String], log: &OperationLog) -> Result<String> {
        self.exec(arguments, log, true)
    }

    fn invoke_elevated(&self, arguments: &[String], log: &OperationLog) -> Result<String> {
        self.exec(arguments, log, false)
    }
}

pub fn prlctl_path() -> Result<PathBuf> {
    let bundled = PathBuf::from("/Applications/Parallels Desktop.app/Contents/MacOS/prlctl");
    for candidate in ["/usr/local/bin/prlctl", "/opt/homebrew/bin/prlctl"] {
        if Path::new(candidate).is_file() {
            return Ok(PathBuf::from(candidate));
        }
    }
    if bundled.is_file() {
        return Ok(bundled);
    }
    bail!("Parallels prlctl is missing. Install and activate Parallels Desktop with CLI support first.")
}

/// Run a command, mirroring its output to the operation log and this process.
fn stream_command(mut command: Command, log: &OperationLog) -> Result<String> {
    let display = format!("{:?}", command).replace('"', "");
    log.command(&display);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().context("워커를 시작하지 못했어요.")?;
    let stdout = child.stdout.take().context("출력 스트림이 없어요.")?;
    let stderr = child.stderr.take().context("표준 오류 스트림이 없어요.")?;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let error_capture = captured.clone();
    let error_log = log.clone();
    let error_thread = std::thread::spawn(move || pump(stderr, &error_capture, &error_log, Stream::Stderr));
    pump(stdout, &captured, log, Stream::Stdout);
    let status = child.wait()?;
    let _ = error_thread.join();
    let output = captured.lock().unwrap().clone();
    log.exit_code(status.code().unwrap_or(-1));
    if !status.success() {
        let tail: String = output.chars().rev().take(2500).collect::<Vec<_>>().into_iter().rev().collect();
        bail!("Worker failed with exit code {}.\n{tail}", status.code().unwrap_or(-1));
    }
    Ok(output)
}

fn pump<R: std::io::Read>(
    reader: R,
    captured: &std::sync::Arc<std::sync::Mutex<String>>,
    log: &OperationLog,
    stream: Stream,
) {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&buffer).into_owned();
                log.raw(stream, &text);
                captured.lock().unwrap().push_str(&text);
            }
        }
    }
}
