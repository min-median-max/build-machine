pub mod controller;
pub mod preferences;

use controller::{JobOutcome, JobRequest, OutputLine};
use preferences::Preferences;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tauri::{ipc::Channel, Manager};

#[derive(Default)]
struct Jobs { busy: Arc<AtomicBool> }

struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard { fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); } }

fn preference_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("preferences.json"))
}

#[tauri::command]
fn load_preferences(app: tauri::AppHandle) -> Result<Preferences, String> {
    preferences::load(&preference_path(&app)?, controller::find_controller())
}

#[tauri::command]
fn save_preferences(app: tauri::AppHandle, preferences: Preferences) -> Result<(), String> {
    preferences::save(&preference_path(&app)?, &preferences)
}

#[tauri::command]
fn register_project(app: tauri::AppHandle, mut preferences: Preferences, project_path: String) -> Result<Preferences, String> {
    preferences::register(&mut preferences, &project_path)?;
    preferences::save(&preference_path(&app)?, &preferences)?;
    Ok(preferences)
}

#[tauri::command]
fn open_settings_folder(app: tauri::AppHandle) -> Result<(), String> {
    let path = preference_path(&app)?;
    let directory = path.parent().ok_or("설정 폴더가 없어요.")?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("/usr/bin/open").arg(directory).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("설정 폴더를 열지 못했어요.".into()) }
}

#[tauri::command]
async fn get_overview(controller_path: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || controller::overview(&controller_path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_job(request: JobRequest, on_output: Channel<OutputLine>, jobs: tauri::State<'_, Jobs>) -> Result<JobOutcome, String> {
    if jobs.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("이미 작업을 실행 중이에요.".into());
    }
    let guard = BusyGuard(jobs.busy.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        controller::execute(request, Arc::new(move |line| { let _ = on_output.send(line); }))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_log_folder(controller_path: String) -> Result<(), String> {
    controller::open_logs(&controller_path)
}

pub fn run() {
    let app = tauri::Builder::default()
        .manage(Jobs::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![load_preferences, save_preferences, register_project, open_settings_folder, get_overview, start_job, open_log_folder])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<Jobs>().busy.load(Ordering::SeqCst) { api.prevent_close(); }
            }
        })
        .build(tauri::generate_context!())
        .expect("Build Machine application could not start");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if handle.state::<Jobs>().busy.load(Ordering::SeqCst) { api.prevent_exit(); }
        }
    });
}
