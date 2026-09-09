use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Page { Dashboard, Environment, Projects }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub path: String,
    pub platforms: Vec<String>,
    pub launch: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub controller_path: String,
    pub environment_platforms: Vec<String>,
    pub projects: Vec<Project>,
    pub selected_project: Option<String>,
    pub page: Page,
}

pub fn all_platforms() -> Vec<String> {
    ["windows", "linux", "macos"].map(String::from).to_vec()
}

pub fn save(path: &Path, preferences: &Preferences) -> Result<(), String> {
    for platforms in std::iter::once(&preferences.environment_platforms).chain(preferences.projects.iter().map(|project| &project.platforms)) {
        if platforms.iter().any(|name| !["windows", "linux", "macos"].contains(&name.as_str())) {
            return Err("설정에 지원하지 않는 운영체제가 있어요.".into());
        }
    }
    fs::create_dir_all(path.parent().ok_or("설정 폴더가 없어요.")?).map_err(|e| e.to_string())?;
    let partial = path.with_extension("partial");
    fs::write(&partial, serde_json::to_vec_pretty(preferences).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(partial, path).map_err(|e| e.to_string())
}

pub fn load(path: &Path, controller_path: String) -> Result<Preferences, String> {
    let mut preferences = Preferences { controller_path, environment_platforms: all_platforms(),
        projects: vec![], selected_project: None, page: Page::Dashboard };
    if path.is_file() {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if value.get("projects").is_some() {
            return serde_json::from_value(value).map_err(|e| e.to_string());
        }
        // Preserve the explicitly selected project from the existing initial GUI.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct InitialPreferences { controller_path: String, project_path: Option<String>, platforms: Vec<String> }
        let previous: InitialPreferences = serde_json::from_value(value).map_err(|e| e.to_string())?;
        preferences.controller_path = previous.controller_path;
        preferences.environment_platforms = previous.platforms.clone();
        if let Some(project) = previous.project_path {
            preferences.projects.push(Project { path: project.clone(), platforms: previous.platforms, launch: false });
            preferences.selected_project = Some(project);
        }
    }
    save(path, &preferences)?;
    Ok(preferences)
}

pub fn register(preferences: &mut Preferences, project_path: &str) -> Result<(), String> {
    let path = Path::new(project_path).canonicalize().map_err(|_| "프로젝트 폴더를 찾을 수 없어요.".to_string())?;
    let response = Command::new("/usr/bin/git").args(["-C"]).arg(&path).args(["rev-parse", "--is-inside-work-tree"]).output().map_err(|e| e.to_string())?;
    if !response.status.success() || String::from_utf8_lossy(&response.stdout).trim() != "true" {
        return Err("Git 프로젝트 폴더를 선택해주세요.".into());
    }
    let path = path.to_string_lossy().into_owned();
    if !preferences.projects.iter().any(|project| project.path == path) {
        preferences.projects.push(Project { path: path.clone(), platforms: all_platforms(), launch: false });
    }
    preferences.selected_project = Some(path);
    preferences.page = Page::Projects;
    Ok(())
}
