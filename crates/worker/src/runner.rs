//! What a GitHub runner gives each step besides its command.
//!
//! A step changes the environment of the steps after it through two files:
//! `GITHUB_ENV` sets variables and `GITHUB_PATH` puts directories in front of
//! PATH. A setup action does the same through its toolkit. Without them a step
//! that writes to `"$GITHUB_ENV"` fails on an unset variable, and an installed
//! tool is invisible to the next step, so both are part of the replay. A step
//! with an `id` also sets outputs through `GITHUB_OUTPUT`, which later steps
//! read as `steps.<id>.outputs.<name>`.

use anyhow::{bail, Context, Result};
use build_machine_core::workflow::Outputs;
use build_machine_core::Platform;
use std::path::{Path, PathBuf};

/// The runner state of one replay.
pub struct Runner {
    directory: PathBuf,
    workspace: PathBuf,
    platform: Platform,
    /// Variables steps have set, in the order they were set.
    variables: Vec<(String, String)>,
    /// Directories steps have added, the most recent first, as GitHub orders them.
    paths: Vec<PathBuf>,
    /// What each step with an `id` wrote to `GITHUB_OUTPUT`.
    outputs: Outputs,
    /// The job's own runner variables: `GITHUB_SHA`, `GITHUB_REF`,
    /// `GITHUB_JOB`, `RUNNER_TRACKING_ID` and the like.
    context: Vec<(String, String)>,
}

/// The files one step may write.
pub struct StepFiles {
    pub env: PathBuf,
    pub path: PathBuf,
    pub output: PathBuf,
    /// `GITHUB_STEP_SUMMARY`. GitHub renders it on the run page; a replay
    /// only provides it.
    pub summary: PathBuf,
}

impl Runner {
    /// Start a replay's runner state in `directory`, emptied first so nothing a
    /// previous replay set carries over.
    pub fn new(directory: &Path, workspace: &Path, platform: Platform) -> Result<Runner> {
        if directory.exists() {
            std::fs::remove_dir_all(directory)
                .with_context(|| format!("이전 runner 디렉터리를 지우지 못했어요: {}", directory.display()))?;
        }
        std::fs::create_dir_all(directory.join("temp"))?;
        Ok(Runner {
            directory: directory.to_path_buf(),
            workspace: workspace.to_path_buf(),
            platform,
            variables: Vec::new(),
            paths: Vec::new(),
            outputs: Outputs::new(),
            context: Vec::new(),
        })
    }

    /// Set a runner variable every step of the job sees.
    pub fn set(&mut self, key: &str, value: &str) {
        self.context.retain(|(existing, _)| existing != key);
        self.context.push((key.to_owned(), value.to_owned()));
    }

    /// Put a directory in front of PATH for every later step.
    pub fn add_path(&mut self, path: PathBuf) {
        self.paths.retain(|existing| existing != &path);
        self.paths.insert(0, path);
    }

    /// Empty files for one step to write into.
    pub fn begin_step(&self, position: u32) -> Result<StepFiles> {
        let files = StepFiles {
            env: self.directory.join(format!("env-{position}")),
            path: self.directory.join(format!("path-{position}")),
            output: self.directory.join(format!("output-{position}")),
            summary: self.directory.join(format!("summary-{position}")),
        };
        std::fs::write(&files.summary, "")?;
        std::fs::write(&files.env, "")?;
        std::fs::write(&files.path, "")?;
        std::fs::write(&files.output, "")?;
        Ok(files)
    }

    /// The outputs the steps so far have set, by step id.
    pub fn outputs(&self) -> &Outputs {
        &self.outputs
    }

    /// Apply what a step wrote. A malformed file fails the step that wrote it.
    /// Outputs are kept for a step with an `id`, the only way to read them.
    pub fn finish_step(&mut self, files: &StepFiles, id: Option<&str>) -> Result<()> {
        let env = std::fs::read_to_string(&files.env).unwrap_or_default();
        let path = std::fs::read_to_string(&files.path).unwrap_or_default();
        let output = std::fs::read_to_string(&files.output).unwrap_or_default();
        std::fs::remove_file(&files.env).ok();
        std::fs::remove_file(&files.path).ok();
        std::fs::remove_file(&files.output).ok();
        std::fs::remove_file(&files.summary).ok();
        let outputs = parse_env_file(&output).context("GITHUB_OUTPUT를 읽지 못했어요")?;
        if let Some(id) = id {
            self.outputs.entry(id.to_owned()).or_default().extend(outputs);
        }
        for (key, value) in parse_env_file(&env).context("GITHUB_ENV를 읽지 못했어요")? {
            self.variables.retain(|(existing, _)| existing != &key);
            self.variables.push((key, value));
        }
        for line in path.lines().map(str::trim).filter(|line| !line.is_empty()) {
            self.add_path(PathBuf::from(line));
        }
        Ok(())
    }

    /// The environment a step runs under: the machine's own, the runner's
    /// variables, what earlier steps set, and PATH with their directories first.
    pub fn environment(&self, base: &[(String, String)], files: &StepFiles) -> Vec<(String, String)> {
        let mut environment: Vec<(String, String)> = base.to_vec();
        let set = |environment: &mut Vec<(String, String)>, key: &str, value: String| {
            environment.retain(|(existing, _)| !existing.eq_ignore_ascii_case(key));
            environment.push((key.to_owned(), value));
        };
        for (key, value) in &self.variables {
            set(&mut environment, key, value.clone());
        }
        let runner: [(&str, String); 10] = [
            ("GITHUB_ACTIONS", "true".to_owned()),
            ("CI", "true".to_owned()),
            ("GITHUB_WORKSPACE", self.workspace.to_string_lossy().into_owned()),
            ("RUNNER_TEMP", self.directory.join("temp").to_string_lossy().into_owned()),
            ("RUNNER_OS", runner_os(self.platform).to_owned()),
            ("RUNNER_ARCH", runner_arch().to_owned()),
            ("GITHUB_ENV", files.env.to_string_lossy().into_owned()),
            ("GITHUB_PATH", files.path.to_string_lossy().into_owned()),
            ("GITHUB_OUTPUT", files.output.to_string_lossy().into_owned()),
            ("GITHUB_STEP_SUMMARY", files.summary.to_string_lossy().into_owned()),
        ];
        for (key, value) in runner {
            set(&mut environment, key, value);
        }
        for (key, value) in &self.context {
            set(&mut environment, key, value.clone());
        }
        let separator = if cfg!(windows) { ";" } else { ":" };
        let inherited = environment
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let mut parts: Vec<String> = self.paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if !inherited.is_empty() {
            parts.push(inherited);
        }
        set(&mut environment, "PATH", parts.join(separator));
        environment
    }
}

fn runner_os(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "Linux",
        Platform::Macos => "macOS",
        Platform::Windows => "Windows",
    }
}

fn runner_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "ARM64",
        "x86_64" => "X64",
        other => other,
    }
}

/// Read a `GITHUB_ENV` file: `NAME=value` lines and `NAME<<DELIMITER` blocks.
pub fn parse_env_file(text: &str) -> Result<Vec<(String, String)>> {
    let mut variables = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let heredoc = line.find("<<").filter(|at| line.find('=').is_none_or(|equals| *at < equals));
        if let Some(at) = heredoc {
            let name = &line[..at];
            let delimiter = &line[at + 2..];
            check_name(name)?;
            if delimiter.is_empty() {
                bail!("GITHUB_ENV의 {name}에 구분자가 없어요.");
            }
            let mut value = Vec::new();
            loop {
                match lines.next() {
                    Some(next) if next == delimiter => break,
                    Some(next) => value.push(next),
                    None => bail!("GITHUB_ENV의 {name}이 구분자 {delimiter}로 끝나지 않았어요."),
                }
            }
            variables.push((name.to_owned(), value.join("\n")));
        } else {
            let (name, value) =
                line.split_once('=').with_context(|| format!("GITHUB_ENV 줄의 형식이 올바르지 않아요: {line}"))?;
            check_name(name)?;
            variables.push((name.to_owned(), value.to_owned()));
        }
    }
    Ok(variables)
}

fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && name.chars().all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        bail!("GITHUB_ENV의 변수 이름이 올바르지 않아요: {name}");
    }
    Ok(())
}

/// Terminate what a job left running, as a runner does at the end of a job.
///
/// GitHub's runner gives every step `RUNNER_TRACKING_ID` and, when the job
/// ends, terminates each process that still carries it ("Cleaning up orphan
/// processes"). A server a step started in the background would otherwise
/// outlive the job and hold its ports in the next replay. A process that
/// replaced its environment, as `sudo` does, no longer carries the variable
/// and is left running there too.
#[cfg(target_os = "linux")]
pub fn terminate_orphans(tracking: &str) -> Vec<String> {
    let wanted = format!("RUNNER_TRACKING_ID={tracking}");
    let mut terminated = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else { return terminated };
    let own = std::process::id();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == own {
            continue;
        }
        let Ok(environment) = std::fs::read(entry.path().join("environ")) else { continue };
        if !environment.split(|byte| *byte == 0).any(|item| item == wanted.as_bytes()) {
            continue;
        }
        let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default().trim().to_owned();
        if unsafe { kill(pid as i32, 9) } == 0 {
            terminated.push(format!("{pid} ({name})"));
        }
    }
    terminated
}

#[cfg(target_os = "linux")]
extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}
