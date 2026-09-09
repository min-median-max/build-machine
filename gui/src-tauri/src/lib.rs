pub mod controller;

use controller::{JobOutcome, JobRequest, OutputLine, Preferences};
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
    let path = preference_path(&app)?;
    if path.is_file() {
        return serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string());
    }
    let root = controller::find_controller();
    let recent = controller::recent_projects(std::path::Path::new(&root));
    Ok(Preferences { controller_path: root, project_path: recent.first().cloned(), platforms: vec!["linux".into()] })
}

#[tauri::command]
fn save_preferences(app: tauri::AppHandle, preferences: Preferences) -> Result<(), String> {
    let path = preference_path(&app)?;
    fs::create_dir_all(path.parent().ok_or("설정 폴더가 없어요.")?).map_err(|e| e.to_string())?;
    let partial = path.with_extension("partial");
    fs::write(&partial, serde_json::to_vec_pretty(&preferences).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(partial, path).map_err(|e| e.to_string())
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
        .invoke_handler(tauri::generate_handler![load_preferences, save_preferences, get_overview, start_job, open_log_folder])
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
