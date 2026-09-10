//! Preparing the one source snapshot a run uses.
//!
//! Every selected environment builds the same bytes. The archive is stored by
//! content, so an unchanged project reuses the archive it already produced.

use crate::Operation;
use anyhow::{Context, Result};
use build_machine_core::request::Framework;
use build_machine_core::source::{self, Snapshot};
use build_machine_core::workflow;
use std::path::{Path, PathBuf};

fn transfer_directory(state: &Path, key: &str) -> PathBuf {
    state.join("projects").join(key)
}

/// Archive the project once and keep it under its own content hash.
fn archive_project(state: &Path, project: &Path, reference: Option<&str>) -> Result<(Snapshot, PathBuf)> {
    let root = source::repository_root(project)?;
    let key = source::project_key(&root);
    let transfer = transfer_directory(state, &key);
    std::fs::create_dir_all(&transfer)?;
    let pending = transfer.join("source.pending.zip");
    let (revision, dirty, hash, count, mode) = source::make_archive(&root, &pending, reference)?;
    let archive = transfer.join(format!("{hash}.zip"));
    if archive.exists() {
        std::fs::remove_file(&pending)?;
    } else {
        std::fs::rename(&pending, &archive)?;
    }
    let snapshot = Snapshot {
        revision,
        dirty,
        source_hash: hash,
        file_count: count,
        source_mode: mode,
        project_key: key,
        project: root.to_string_lossy().into_owned(),
        archive: archive.to_string_lossy().into_owned(),
        workflow_path: None,
        event: None,
        requested_ref: None,
        stage_counts: Default::default(),
        framework: None,
        command: None,
        artifact: None,
    };
    Ok((snapshot, archive))
}

/// Detect the recipe when the caller did not name one.
fn detect_framework(project: &Path, requested: Option<Framework>) -> Result<Framework> {
    if let Some(framework) = requested {
        return Ok(framework);
    }
    if project.join("src-tauri/tauri.conf.json").is_file() {
        Ok(Framework::Tauri)
    } else if project.join("wails.json").is_file() {
        Ok(Framework::Wails2)
    } else {
        anyhow::bail!("Use --framework custom --command COMMAND --artifact RELATIVE_PATH for this project.")
    }
}

pub fn for_build(operation: &Operation, state: &Path) -> Result<Snapshot> {
    let project = operation.project.clone().context("프로젝트 폴더가 필요해요.")?;
    let (mut snapshot, _archive) = archive_project(state, &project, None)?;
    let framework = detect_framework(Path::new(&snapshot.project), operation.framework)?;
    if framework == Framework::Custom && (operation.command.is_none() || operation.artifact.is_none()) {
        anyhow::bail!("Custom builds require --command and --artifact.");
    }
    snapshot.framework = Some(format!("{framework:?}").to_lowercase());
    snapshot.command = operation.command.clone();
    snapshot.artifact = operation.artifact.clone();
    Ok(snapshot)
}

/// A run action only needs to name the project, not archive it.
pub fn for_run(operation: &Operation) -> Result<Snapshot> {
    let project = operation.project.clone().context("프로젝트 폴더가 필요해요.")?;
    let root = source::repository_root(&project)?;
    Ok(Snapshot {
        revision: String::new(),
        dirty: false,
        source_hash: String::new(),
        file_count: 0,
        source_mode: source::SourceMode::Local,
        project_key: source::project_key(&root),
        project: root.to_string_lossy().into_owned(),
        archive: String::new(),
        workflow_path: None,
        event: None,
        requested_ref: None,
        stage_counts: Default::default(),
        framework: None,
        command: None,
        artifact: None,
    })
}

pub struct Replay {
    pub snapshot: Snapshot,
    pub workflow: workflow::Workflow,
}

/// Read the workflow that will be replayed, from the working tree or from an
/// immutable ref, and archive the matching source.
pub fn for_replay(operation: &Operation, state: &Path) -> Result<Replay> {
    let project = operation.project.clone().context("프로젝트 폴더가 필요해요.")?;
    let root = source::repository_root(&project)?;
    let event = operation.event.clone();
    let reference = operation.reference.as_deref();
    let selected = match reference {
        Some(reference) => {
            let relative = match &operation.workflow {
                Some(path) => PathBuf::from(path),
                None => {
                    let current = workflow::discover(&root, None, &event, None)?;
                    PathBuf::from(&current.path)
                        .strip_prefix(&root)
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|_| PathBuf::from(&current.path))
                }
            };
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .arg("show")
                .arg(format!("{reference}:{}", relative.to_string_lossy().replace('\\', "/")))
                .output()
                .context("git show를 실행하지 못했어요.")?;
            if !output.status.success() {
                anyhow::bail!("고정 ref에서 workflow를 읽지 못했어요: {}", relative.display());
            }
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            workflow::load_text(&relative.to_string_lossy(), &text, &event, Some(reference))?
        }
        None => workflow::discover(&root, operation.workflow.as_deref(), &event, None)?,
    };
    let (mut snapshot, _archive) = archive_project(state, &root, reference)?;
    snapshot.workflow_path = Some(selected.path.clone());
    snapshot.event = Some(event);
    snapshot.requested_ref = operation.reference.clone();
    snapshot.stage_counts = workflow::stage_counts(&selected);
    Ok(Replay { snapshot, workflow: selected })
}
