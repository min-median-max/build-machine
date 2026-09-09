use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub controller_path: String,
    pub project_path: Option<String>,
    pub platforms: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action { Doctor, Setup, Build, Run }

impl Action {
    fn name(&self) -> &str {
        match self { Self::Doctor => "doctor", Self::Setup => "setup", Self::Build => "build", Self::Run => "run" }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub controller_path: String,
    pub project_path: Option<String>,
    pub platforms: Vec<String>,
    pub action: Action,
    pub launch: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOutcome {
    pub exit_code: i32,
    pub result: Option<Value>,
    pub result_path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutputLine { pub stream: String, pub line: String }

pub fn controller_root(value: &str) -> Result<PathBuf, String> {
    let root = Path::new(value).canonicalize().map_err(|_| "빌드 도구 폴더를 찾을 수 없어요. 폴더를 다시 선택해주세요.".to_string())?;
    if !root.join("build.py").is_file() || !root.join("machine.json").is_file() {
        return Err("선택한 폴더에 build.py와 machine.json이 없어요.".into());
    }
    Ok(root)
}

pub fn find_controller() -> String {
    if let Ok(executable) = std::env::current_exe() {
        for ancestor in executable.ancestors() {
            if ancestor.join("build.py").is_file() && ancestor.join("machine.json").is_file() {
                return ancestor.to_string_lossy().into_owned();
            }
        }
    }
    std::env::var_os("HOME").map(PathBuf::from).map(|home| home.join("Work/build-machine").to_string_lossy().into_owned()).unwrap_or_default()
}

fn process(command: &str) -> Command {
    let mut child = Command::new(command);
    let inherited = std::env::var("PATH").unwrap_or_default();
    child.env("PATH", format!("/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:{inherited}"));
    child
}

pub fn recent_projects(root: &Path) -> Vec<String> {
    let mut projects = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join(".state/projects")) {
        for entry in entries.flatten() {
            let receipt = entry.path().join("latest.json");
            if let Ok(data) = fs::read(&receipt) {
                if let Ok(value) = serde_json::from_slice::<Value>(&data) {
                    if let Some(project) = value.get("project").and_then(Value::as_str) {
                        if Path::new(project).is_dir() {
                            let modified = fs::metadata(&receipt).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                            projects.push((modified, project.to_owned()));
                        }
                    }
                }
            }
        }
    }
    projects.sort_by(|a, b| b.0.cmp(&a.0));
    let mut paths = Vec::new();
    for (_, path) in projects { if !paths.contains(&path) { paths.push(path); } }
    paths.truncate(8);
    paths
}

pub fn overview(value: &str) -> Result<Value, String> {
    let root = controller_root(value)?;
    let config: Value = serde_json::from_slice(&fs::read(root.join("machine.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let response = process("prlctl").args(["list", "-a", "--json"]).output();
    let (vms, warning) = match response {
        Ok(output) if output.status.success() => match serde_json::from_slice::<Vec<Value>>(&output.stdout) {
            Ok(vms) => (vms, None),
            Err(error) => (vec![], Some(format!("Parallels 응답을 읽지 못했어요: {error}"))),
        },
        Ok(output) => (vec![], Some(String::from_utf8_lossy(&output.stderr).trim().to_owned())),
        Err(error) => (vec![], Some(format!("Parallels에 연결하지 못했어요: {error}"))),
    };
    let mut environments = Vec::new();
    for (id, title) in [("windows", "Windows"), ("linux", "Ubuntu"), ("macos", "macOS")] {
        let profile = &config["platforms"][id];
        let target = profile["target"].as_str().ok_or_else(|| format!("machine.json에 {id}의 target 설정이 없어요."))?;
        let vm_name = profile["vm"].as_str();
        let status = if id == "macos" { "local" } else {
            vms.iter().find(|vm| vm["name"].as_str() == vm_name).and_then(|vm| vm["status"].as_str()).unwrap_or("unavailable")
        };
        environments.push(json!({"id":id,"title":title,"target":target,"vm":vm_name,"status":status}));
    }
    Ok(json!({"controllerPath":root,"environments":environments,"recentProjects":recent_projects(&root),"toolStatus":tool_status(&root, &config),"warning":warning}))
}

pub fn tool_status(root: &Path, config: &Value) -> Value {
    let status = fs::read(root.join(".state/tool-status.json")).ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok());
    match status {
        Some(status) if status.get("configuration") == Some(config) => {
            status.get("results").filter(|results| results.is_object()).cloned().unwrap_or_else(|| json!({}))
        },
        _ => json!({}),
    }
}

pub fn arguments(request: &JobRequest, root: &Path, result: &Path) -> Result<Vec<String>, String> {
    if request.platforms.is_empty() || request.platforms.iter().any(|os| !["windows", "linux", "macos"].contains(&os.as_str())) {
        return Err("실행할 운영체제를 하나 이상 선택해주세요.".into());
    }
    let mut args = vec!["-u".to_owned(), root.join("build.py").to_string_lossy().into_owned(), request.action.name().into()];
    if matches!(request.action, Action::Build | Action::Run) {
        let project = request.project_path.as_ref().filter(|path| Path::new(path).is_dir()).ok_or("프로젝트 폴더를 선택해주세요.")?;
        args.push(project.clone());
    }
    args.push("--os".into());
    args.extend(request.platforms.iter().cloned());
    args.extend(["--result-file".into(), result.to_string_lossy().into_owned()]);
    if request.launch && matches!(request.action, Action::Build) { args.push("--run".into()); }
    Ok(args)
}

fn stream(reader: impl Read, name: &str, emit: &Arc<dyn Fn(OutputLine) + Send + Sync>) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => emit(OutputLine { stream: name.into(), line: String::from_utf8_lossy(&bytes).trim_end_matches(['\r','\n']).into() }),
            Err(error) => { emit(OutputLine { stream: "stderr".into(), line: format!("로그 읽기 실패: {error}") }); break; }
        }
    }
}

pub fn execute(request: JobRequest, emit: Arc<dyn Fn(OutputLine) + Send + Sync>) -> Result<JobOutcome, String> {
    let root = controller_root(&request.controller_path)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let result_path = root.join(".state/gui").join(format!("{stamp}-{}.json", std::process::id()));
    let args = arguments(&request, &root, &result_path)?;
    let mut child = process("/usr/bin/python3").args(args).current_dir(&root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("빌드 명령을 시작하지 못했어요: {e}"))?;
    let stderr = child.stderr.take().ok_or("표준 오류 스트림이 없어요.")?;
    let stderr_emit = emit.clone();
    let error_thread = std::thread::spawn(move || stream(stderr, "stderr", &stderr_emit));
    stream(child.stdout.take().ok_or("출력 스트림이 없어요.")?, "stdout", &emit);
    let status = child.wait().map_err(|e| e.to_string())?;
    error_thread.join().map_err(|_| "오류 로그 처리에 실패했어요.")?;
    let result = if result_path.exists() {
        Some(serde_json::from_slice(&fs::read(&result_path).map_err(|e| e.to_string())?).map_err(|e| format!("결과 파일을 읽지 못했어요: {e}"))?)
    } else if status.success() {
        return Err(format!("명령은 종료됐지만 결과 파일이 없어서 성공을 확인할 수 없어요: {}", result_path.display()));
    } else { None };
    Ok(JobOutcome { exit_code: status.code().unwrap_or(-1), result, result_path: result_path.to_string_lossy().into_owned() })
}

pub fn open_logs(value: &str) -> Result<(), String> {
    let logs = controller_root(value)?.join(".state/logs");
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let status = process("/usr/bin/open").arg(logs).status().map_err(|e| e.to_string())?;
    if !status.success() { return Err("로그 폴더를 열지 못했어요.".into()); }
    Ok(())
}
