use build_machine_desktop::preferences::{self, Page};
use std::{fs, process::Command};

#[test]
fn existing_selected_project_and_os_choices_are_preserved_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    fs::write(&path, r#"{"controllerPath":"/controller","projectPath":"/projects/airdata","platforms":["linux"]}"#).unwrap();
    let loaded = preferences::load(&path, "/default".into()).unwrap();
    assert_eq!(loaded.controller_path, "/controller");
    assert_eq!(loaded.projects.len(), 1);
    assert_eq!(loaded.projects[0].path, "/projects/airdata");
    assert_eq!(loaded.projects[0].platforms, ["linux"]);
    assert!(!loaded.projects[0].launch);
    assert_eq!(loaded.selected_project.as_deref(), Some("/projects/airdata"));
    assert_eq!(preferences::load(&path, "/default".into()).unwrap().projects.len(), 1);
}

#[test]
fn registration_deduplicates_folders_and_settings_survive_reload_independently() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("config/preferences.json");
    let project = directory.path().join("project with ' spaces");
    fs::create_dir(&project).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).arg(&project).status().unwrap().success());
    let mut settings = preferences::load(&settings_path, "/controller".into()).unwrap();
    assert!(settings.projects.is_empty());
    preferences::register(&mut settings, project.to_str().unwrap()).unwrap();
    preferences::register(&mut settings, project.join(".").to_str().unwrap()).unwrap();
    assert_eq!(settings.projects.len(), 1);
    settings.projects[0].platforms = vec!["linux".into()];
    settings.projects[0].launch = true;
    settings.environment_platforms = vec!["windows".into(), "macos".into()];
    settings.page = Page::Projects;
    preferences::save(&settings_path, &settings).unwrap();
    let loaded = preferences::load(&settings_path, "/unused".into()).unwrap();
    assert_eq!(loaded.projects[0].platforms, ["linux"]);
    assert!(loaded.projects[0].launch);
    assert_eq!(loaded.environment_platforms, ["windows", "macos"]);
    assert!(matches!(loaded.page, Page::Projects));
    assert!(!settings_path.with_extension("partial").exists());
}

#[test]
fn a_non_git_folder_is_not_registered() {
    let directory = tempfile::tempdir().unwrap();
    let mut settings = preferences::load(&directory.path().join("preferences.json"), "/controller".into()).unwrap();
    assert!(preferences::register(&mut settings, directory.path().to_str().unwrap()).unwrap_err().contains("Git"));
    assert!(settings.projects.is_empty());
}
