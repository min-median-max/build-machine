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
        event_payload: None,
        pull_request: None,
        requested_ref: None,
        stage_counts: Default::default(),
        framework: None,
        command: None,
        artifact: None,
        history: None,
        history_sha256: None,
        checkout_ref: None,
        repositories: Default::default(),
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
        event_payload: None,
        pull_request: None,
        requested_ref: None,
        stage_counts: Default::default(),
        framework: None,
        command: None,
        artifact: None,
        history: None,
        history_sha256: None,
        checkout_ref: None,
        repositories: Default::default(),
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
            // The workflow and every reusable workflow it calls come from the ref.
            let show = |path: &str| -> Result<String> {
                let output = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .arg("show")
                    .arg(format!("{reference}:{}", path.replace('\\', "/")))
                    .output()
                    .context("git show를 실행하지 못했어요.")?;
                if !output.status.success() {
                    anyhow::bail!("고정 ref {reference}에서 workflow를 읽지 못했어요: {path}");
                }
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            };
            let path = relative.to_string_lossy().into_owned();
            let text = show(&path)?;
            workflow::load_text(&path, &text, &event, Some(reference), &show)?
        }
        None => workflow::discover(&root, operation.workflow.as_deref(), &event, None)?,
    };
    let (mut snapshot, archive) = archive_project(state, &root, reference)?;
    // actions/checkout fetches from the repository's history; the bundle is
    // replaced on every replay, as branches and tags move.
    let bundle = archive.with_file_name("history.bundle");
    let history = source::make_history(&root, reference, &bundle)?;
    snapshot.history = Some(bundle.to_string_lossy().into_owned());
    snapshot.history_sha256 = Some(history.sha256);
    snapshot.checkout_ref = history.reference;
    // The workflow's github.ref is the ref the checkout takes.
    let github = workflow::github_context(
        &selected.jobs,
        &event,
        &snapshot.revision,
        snapshot.checkout_ref.as_deref(),
        operation.event_payload.as_ref(),
    )?;
    // Each other repository a checkout step names is fetched from its local
    // clone's committed history, bundled the same way beside the project's.
    for (name, clone) in workflow::checkout_repositories(&selected, &operation.machine.repositories, &github)? {
        let bundle = archive.with_file_name("repositories").join(format!("{name}.bundle"));
        let history = source::make_history(&clone, None, &bundle)
            .with_context(|| format!("{name}의 Git 기록을 묶지 못했어요: {}", clone.display()))?;
        snapshot.repositories.insert(
            name,
            source::RepositoryHistory { bundle: bundle.to_string_lossy().into_owned(), sha256: history.sha256 },
        );
    }
    snapshot.workflow_path = Some(selected.path.clone());
    snapshot.event = Some(event);
    snapshot.event_payload = operation.event_payload.clone();
    snapshot.pull_request = operation.pull_request.clone();
    snapshot.requested_ref = operation.reference.clone();
    snapshot.stage_counts = workflow::stage_counts(&selected);
    Ok(Replay { snapshot, workflow: selected })
}
