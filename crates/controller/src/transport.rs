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
    pub desktop_user: Option<String>,
}

impl Parallels {
    pub fn new(root: PathBuf, machine: &Machine, platform: Platform, worker_source: PathBuf) -> Result<Parallels> {
        let profile = machine.profile(platform)?;
        let vm = profile.vm.clone().with_context(|| format!("machine.json에 {platform}의 vm이 없어요."))?;
        Ok(Parallels { root, platform, vm, share: machine.share.clone(), worker_source, prlctl: prlctl_path()?, desktop_user: profile.desktop_user.clone() })
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
        exec_with_retry(&mut || self.exec_command(arguments, as_user), log)
    }

    fn exec_command(&self, arguments: &[String], as_user: bool) -> Result<Command> {
        let placement = self.placement()?;
        let mut command = Command::new(&self.prlctl);
        command.arg("exec").arg(&self.vm);
        if as_user && self.platform == Platform::Linux {
            if let Some(user) = &self.desktop_user {
                let mut worker = vec![placement.worker];
                worker.extend_from_slice(arguments);
                worker.extend(["--config".to_owned(), placement.config]);
                let script = format!("exec {}", worker.iter().map(|value| shell_quote(value)).collect::<Vec<_>>().join(" "));
                // Parallels authenticates --current-user separately from the
                // desktop login. Its root channel lets runuser select the
                // declared Linux account without storing a password.
                command.arg(format!("runuser -l {} -c {}", shell_quote(user), shell_quote(&script)));
                return Ok(command);
            }
        }
        if as_user {
            command.arg("--current-user");
        }
        command.arg(&placement.worker).args(arguments).arg("--config").arg(&placement.config);
        Ok(command)
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::shell_quote;

    #[test]
    fn guest_shell_preserves_arguments_without_evaluating_them() {
        let values = ["", "path with spaces", "owner's file", "$(printf injected)", "a; printf injected", "line\nbreak"];
        let worker = format!("printf '%s\\0' {}", values.iter().map(|value| shell_quote(value)).collect::<Vec<_>>().join(" "));
        // runuser -c introduces a second shell interpretation, so verify
        // both layers, including metacharacters in workflow arguments.
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("sh -c {}", shell_quote(&worker))])
            .output().unwrap();
        assert!(output.status.success());
        let expected: Vec<u8> = values.iter().flat_map(|value| value.bytes().chain(std::iter::once(0))).collect();
        assert_eq!(output.stdout, expected);
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

/// A command that ran and exited unsuccessfully, with its output.
#[derive(Debug)]
pub struct CommandFailed {
    pub code: Option<i32>,
    /// Standard output and standard error, interleaved as they arrived.
    pub output: String,
    /// Standard output alone, where a worker writes its report.
    pub stdout: String,
}

impl std::fmt::Display for CommandFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tail: String = self.output.chars().rev().take(2500).collect::<Vec<_>>().into_iter().rev().collect();
        let shown = if tail.len() < self.output.len() { "the last 2500 characters of its output; the platform log holds all of it" } else { "its output" };
        write!(formatter, "Worker failed with exit code {} ({shown}).\n{tail}", self.code.unwrap_or(-1))
    }
}

impl std::error::Error for CommandFailed {}

/// How many times a guest command Parallels did not start is started.
pub const GUEST_EXEC_ATTEMPTS: u32 = 5;

/// `prlctl exec` gave up before the guest command started, every time.
#[derive(Debug)]
pub struct GuestExecNotStarted {
    pub attempts: u32,
    pub message: String,
}

impl std::fmt::Display for GuestExecNotStarted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Parallels did not start the guest command in {} attempts: {}. This is prlctl exec of Parallels 27 failing before the command runs, not a failure of the command.",
            self.attempts, self.message
        )
    }
}

impl std::error::Error for GuestExecNotStarted {}

/// Parallels 27's `prlctl exec` intermittently exits 255 with nothing but
/// `PrlJob_GetRetCode: Invalid argument` or `PrlJob_GetResult: Invalid
/// argument` before the guest command starts. That message, alone, is this
/// failure; any other output means the command ran.
pub fn exec_not_started(code: Option<i32>, output: &str) -> Option<String> {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let only_prljob = !lines.is_empty()
        && lines.iter().all(|line| line.starts_with("PrlJob_") && line.contains("Invalid argument"));
    (code == Some(255) && only_prljob).then(|| lines.join(" "))
}

/// Run a guest command, starting it again when Parallels did not start it.
///
/// Measured on 2026-10-04 against Parallels 27.0.2: 18 of 150 `prlctl exec`
/// calls failed that way and none of their commands had run, so starting one
/// again does not repeat its effect. A command that failed after it started
/// is never run again. Each attempt that did not start is logged.
pub fn exec_with_retry(build: &mut dyn FnMut() -> Result<Command>, log: &OperationLog) -> Result<String> {
    let mut message = String::new();
    for attempt in 1..=GUEST_EXEC_ATTEMPTS {
        match stream_command(build()?, log) {
            Ok(output) => return Ok(output),
            Err(error) => {
                let not_started =
                    error.downcast_ref::<CommandFailed>().and_then(|failed| exec_not_started(failed.code, &failed.output));
                let Some(not_started) = not_started else { return Err(error) };
                log.note(&format!(
                    "PARALLELS did not start the guest command (attempt {attempt}/{GUEST_EXEC_ATTEMPTS}): {not_started}"
                ));
                message = not_started;
            }
        }
    }
    Err(GuestExecNotStarted { attempts: GUEST_EXEC_ATTEMPTS, message }.into())
}

/// Run a command, mirroring its output to the operation log and this process.
fn stream_command(mut command: Command, log: &OperationLog) -> Result<String> {
    let display = format!("{:?}", command).replace('"', "");
    log.command(&display);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().context("워커를 시작하지 못했어요.")?;
    let stdout = child.stdout.take().context("출력 스트림이 없어요.")?;
    let stderr = child.stderr.take().context("표준 오류 스트림이 없어요.")?;
    // Both streams go to one interleaved capture for the error a person
    // reads; standard output is also kept alone, because the two streams
    // interleave by line and a worker's report must not take in a line of
    // its standard error.
    let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let error_capture = captured.clone();
    let error_log = log.clone();
    let error_thread = std::thread::spawn(move || pump(stderr, &error_capture, None, &error_log, Stream::Stderr));
    let mut standard_output = String::new();
    pump(stdout, &captured, Some(&mut standard_output), log, Stream::Stdout);
    let status = child.wait()?;
    let _ = error_thread.join();
    let output = captured.lock().unwrap().clone();
    log.exit_code(status.code().unwrap_or(-1));
    if !status.success() {
        return Err(CommandFailed { code: status.code(), output, stdout: standard_output }.into());
    }
    Ok(standard_output)
}

fn pump<R: std::io::Read>(
    reader: R,
    captured: &std::sync::Arc<std::sync::Mutex<String>>,
    mut alone: Option<&mut String>,
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
                if let Some(alone) = alone.as_deref_mut() {
                    alone.push_str(&text);
                }
            }
        }
    }
}
