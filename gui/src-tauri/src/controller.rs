//! The desktop application's view of the controller.
//!
//! The controller is a library, so a job runs in this process rather than
//! through a command line and a result file. There is one definition of every
//! recorded value and one code path both front ends drive.

use build_machine_controller::oplog::Stream;
use build_machine_controller::{
    controller_root as resolve_root, find_controller as locate, matrix, seed_root, state_directory, Lock,
    Operation,
};
use build_machine_core::config::Machine;
use build_machine_core::report::{Action, ExecutionMode, RunReport};
use build_machine_core::request::Framework;
use build_machine_core::Platform;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub controller_path: String,
    pub project_path: Option<String>,
    pub platforms: Vec<String>,
    pub action: String,
    pub launch: bool,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default = "default_event")]
    pub event: String,
    #[serde(default)]
    pub ref_name: Option<String>,
    #[serde(default)]
    pub execution: String,
}

fn default_event() -> String {
    "workflow_dispatch".into()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOutcome {
    pub exit_code: i32,
    pub result: Option<RunReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutputLine {
    pub stream: String,
    pub line: String,
}

pub fn controller_root(value: &str) -> Result<PathBuf, String> {
    resolve_root(Path::new(value)).map_err(|error| format!("{error:#}"))
}

/// The payload a released application carries, if this is one.
fn bundled_payload() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let resources = exe.parent()?.parent()?.join("Resources");
    resources.join("machine.json").is_file().then_some(resources)
}

/// Where this application keeps the machine it drives.
///
/// A developer running from the workspace uses that workspace. A released
/// application places its bundled payload beside its settings, because the
/// bundle is read-only and a virtual machine has to be able to read the
/// workers and source archives from a directory this application can write.
pub fn find_controller(settings: &Path) -> String {
    if let Some(root) = locate() {
        return root.to_string_lossy().into_owned();
    }
    match bundled_payload().map(|payload| seed_root(&payload, &settings.join("machine"))) {
        Some(Ok(root)) => root.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// A bundled application does not inherit a login shell's PATH, and `prlctl`
/// lives in one of these directories.
fn process(command: &str) -> Command {
    let mut child = Command::new(command);
    let inherited = std::env::var("PATH").unwrap_or_default();
    child.env("PATH", format!("/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:{inherited}"));
    child
}

pub fn overview(value: &str) -> Result<Value, String> {
    let root = controller_root(value)?;
    let machine = Machine::load(&root.join("machine.json")).map_err(|error| format!("{error:#}"))?;
    let response = process("prlctl").args(["list", "-a", "--json"]).output();
    let (vms, warning): (Vec<Value>, Option<String>) = match response {
        Ok(output) if output.status.success() => match serde_json::from_slice::<Vec<Value>>(&output.stdout) {
            Ok(vms) => (vms, None),
            Err(error) => (vec![], Some(format!("Parallels 응답을 읽지 못했어요: {error}"))),
        },
        Ok(output) => (vec![], Some(String::from_utf8_lossy(&output.stderr).trim().to_owned())),
        Err(error) => (vec![], Some(format!("Parallels에 연결하지 못했어요: {error}"))),
    };
    let mut environments = Vec::new();
    for (platform, title) in
        [(Platform::Windows, "Windows"), (Platform::Linux, "Ubuntu"), (Platform::Macos, "macOS")]
    {
        let profile = machine.profile(platform).map_err(|error| format!("{error:#}"))?;
        let status = match &profile.vm {
            None => "local",
            Some(name) => vms
                .iter()
                .find(|vm| vm["name"].as_str() == Some(name.as_str()))
                .and_then(|vm| vm["status"].as_str())
                .unwrap_or("unavailable"),
        };
        environments.push(json!({
            "id": platform.as_str(), "title": title, "target": profile.target,
            "vm": profile.vm, "status": status
        }));
    }
    Ok(json!({
        "controllerPath": root,
        "environments": environments,
        "toolStatus": tool_status(&root, &machine),
        "warning": warning
    }))
}

/// The last diagnosis and setup results, dropped when the machine definition
/// they were measured against changes.
pub fn tool_status(root: &Path, machine: &Machine) -> Value {
    let path = state_directory(root).join("tool-status.json");
    let Some(status) = std::fs::read(path).ok().and_then(|data| serde_json::from_slice::<Value>(&data).ok())
    else {
        return json!({});
    };
    let configuration = serde_json::to_value(machine).unwrap_or(Value::Null);
    if status.get("configuration") != Some(&configuration) {
        return json!({});
    }
    status.get("results").filter(|results| results.is_object()).cloned().unwrap_or_else(|| json!({}))
}

fn parse_action(request: &JobRequest) -> Result<Action, String> {
    // A project with a workflow path replays that workflow instead of building.
    let replay = request.workflow.as_ref().is_some_and(|path| !path.trim().is_empty());
    Ok(match request.action.as_str() {
        "doctor" => Action::Doctor,
        "setup" => Action::Setup,
        "run" => Action::Run,
        "build" if replay => Action::Ci,
        "build" => Action::Build,
        other => return Err(format!("알 수 없는 작업이에요: {other}")),
    })
}

pub fn build_operation(request: &JobRequest, observer: build_machine_controller::oplog::Observer) -> Result<Operation, String> {
    let root = controller_root(&request.controller_path)?;
    let machine = Machine::load(&root.join("machine.json")).map_err(|error| format!("{error:#}"))?;
    let action = parse_action(request)?;
    let mut platforms = Vec::new();
    for name in &request.platforms {
        platforms.push(name.parse::<Platform>().map_err(|_| "실행할 운영체제를 하나 이상 선택해주세요.".to_string())?);
    }
    if platforms.is_empty() {
        return Err("실행할 운영체제를 하나 이상 선택해주세요.".into());
    }
    platforms.sort();
    platforms.dedup();
    let project = match action {
        Action::Doctor | Action::Setup => None,
        _ => {
            let path = request
                .project_path
                .as_ref()
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
                .ok_or("프로젝트 폴더를 선택해주세요.")?;
            Some(path)
        }
    };
    Ok(Operation {
        root,
        machine,
        action,
        platforms,
        execution: if request.execution == "parallel" { ExecutionMode::Parallel } else { ExecutionMode::Sequential },
        project,
        framework: None::<Framework>,
        command: None,
        artifact: None,
        // A replay has no launch step, so the option is not carried into one.
        launch: request.launch && action == Action::Build,
        workflow: request.workflow.clone().filter(|path| !path.trim().is_empty()),
        event: request.event.clone(),
        reference: request.ref_name.clone().filter(|value| !value.trim().is_empty()),
        result_file: None,
        observer: Some(observer),
    })
}

pub fn execute(
    request: JobRequest,
    emit: Arc<dyn Fn(OutputLine) + Send + Sync>,
) -> Result<JobOutcome, String> {
    let forward = emit.clone();
    let observer: build_machine_controller::oplog::Observer = Arc::new(move |stream, line| {
        forward(OutputLine {
            stream: match stream {
                Stream::Stderr => "stderr".to_owned(),
                Stream::Stdout => "stdout".to_owned(),
            },
            line: line.to_owned(),
        });
    });
    let operation = build_operation(&request, observer)?;
    let state = state_directory(&operation.root);
    let _lock = Lock::acquire(&state).map_err(|error| format!("{error:#}"))?;
    let report = matrix::execute(&operation).map_err(|error| format!("{error:#}"))?;
    matrix::record_tool_status(&operation.root, &operation.machine, &report)
        .map_err(|error| format!("{error:#}"))?;
    let exit_code = if report.succeeded() { 0 } else { 1 };
    Ok(JobOutcome { exit_code, result: Some(report) })
}

pub fn open_logs(value: &str) -> Result<(), String> {
    let logs = state_directory(&controller_root(value)?).join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let status = process("/usr/bin/open").arg(logs).status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("로그 폴더를 열지 못했어요.".into())
    }
}
