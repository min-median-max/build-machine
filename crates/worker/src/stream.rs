//! Running a child process and showing its output while it runs.
//!
//! A build step's output is the only progress signal there is. Buffering a
//! multi-minute compile until it exits hides that from the terminal and from
//! the desktop app's log panel alike, so every child is streamed line by line
//! and captured at the same time.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Finished {
    pub code: i32,
    /// Interleaved stdout and stderr, in the order they happened.
    pub output: String,
    pub timed_out: bool,
}

impl Finished {
    pub fn success(&self) -> bool {
        self.code == 0 && !self.timed_out
    }
}

/// Kill the child's whole process group.
///
/// A step runs through a shell, so its real work is a grandchild holding the
/// same pipe. Killing only the shell leaves the read blocked until that
/// grandchild finishes, which defeats the timeout entirely.
#[cfg(unix)]
fn kill_tree(pid: u32) {
    unsafe {
        let group = getpgid(pid as i32);
        if group > 0 {
            killpg(group, 9);
        } else {
            kill(pid as i32, 9);
        }
    }
}

#[cfg(unix)]
extern "C" {
    fn getpgid(pid: i32) -> i32;
    fn killpg(group: i32, signal: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn setsid() -> i32;
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Its own session, so the whole tree can be signalled as one group.
    unsafe {
        command.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn detach(_command: &mut Command) {}

/// Run a command, echoing its interleaved output and returning it as well.
pub fn run(
    program: &str,
    arguments: &[String],
    working: Option<&Path>,
    environment: &[(String, String)],
    timeout: Option<Duration>,
) -> Result<Finished> {
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(working) = working {
        command.current_dir(working);
    }
    for (key, value) in environment {
        command.env(key, value);
    }
    detach(&mut command);
    let mut child =
        command.spawn().with_context(|| format!("명령을 시작하지 못했어요: {program}"))?;
    let pid = child.id();

    let stdout = child.stdout.take().context("표준 출력 스트림이 없어요.")?;
    let stderr = child.stderr.take().context("표준 오류 스트림이 없어요.")?;
    let captured = Arc::new(Mutex::new(String::new()));
    let error_capture = captured.clone();
    let error_thread = std::thread::spawn(move || pump(stderr, &error_capture));

    let done = Arc::new(AtomicBool::new(false));
    let fired = Arc::new(AtomicBool::new(false));
    let timer = timeout.map(|limit| {
        let (done, fired) = (done.clone(), fired.clone());
        std::thread::spawn(move || {
            let deadline = Instant::now() + limit;
            while Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !done.load(Ordering::SeqCst) {
                fired.store(true, Ordering::SeqCst);
                kill_tree(pid);
            }
        })
    });

    pump(stdout, &captured);
    let status = child.wait()?;
    done.store(true, Ordering::SeqCst);
    let _ = error_thread.join();
    if let Some(timer) = timer {
        let _ = timer.join();
    }
    let output = captured.lock().unwrap().clone();
    Ok(Finished { code: status.code().unwrap_or(-1), output, timed_out: fired.load(Ordering::SeqCst) })
}

fn pump<R: std::io::Read>(reader: R, captured: &Arc<Mutex<String>>) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&buffer).into_owned();
                print!("{text}");
                let _ = std::io::stdout().flush();
                captured.lock().unwrap().push_str(&text);
            }
        }
    }
}

/// Run a command that must succeed, echoing the command line first.
pub fn checked(
    program: &str,
    arguments: &[String],
    working: Option<&Path>,
    environment: &[(String, String)],
) -> Result<String> {
    println!("> {program} {}", arguments.join(" "));
    let finished = run(program, arguments, working, environment, None)?;
    if !finished.success() {
        anyhow::bail!("{program} failed with exit code {}.", finished.code);
    }
    Ok(finished.output)
}

/// Capture a command's output without echoing it, for machine-readable
/// introspection whose payload would bury the operation output.
pub fn capture(program: &str, arguments: &[String], environment: &[(String, String)]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::null());
    for (key, value) in environment {
        command.env(key, value);
    }
    let output = command
        .output()
        .with_context(|| format!("명령을 실행하지 못했어요: {program}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{program} failed with exit code {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
