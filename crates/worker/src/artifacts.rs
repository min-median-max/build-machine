//! `actions/upload-artifact` and `actions/download-artifact`.
//!
//! An artifact is kept in the store of the run that uploaded it,
//! `<project>/artifacts/<run id>/<name>`, for the steps after it and for a
//! later replay that names that run with `run-id`, as a workflow started by
//! `workflow_run` downloads the artifact of the run it follows. Nothing is
//! sent to GitHub. The files keep their places under the least common
//! ancestor of the uploaded paths, as the action lays them out.

use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};

fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'));
    if !valid {
        bail!("artifact 이름에는 영문자, 숫자, '-', '_', '.'만 쓸 수 있어요: {name}");
    }
    Ok(())
}

/// The files under `path`, a file or a folder, with links followed.
fn files_of(path: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(path).follow_links(true).sort_by_file_name() {
        let entry = entry.with_context(|| format!("{}를 읽지 못했어요.", path.display()))?;
        if entry.file_type().is_file() {
            found.push(entry.into_path());
        }
    }
    Ok(found)
}

/// Upload the newline-separated `paths` under `workspace` as the artifact
/// `name` of the run whose store is `store`. Returns the number of files; a
/// path that matches nothing adds none, and with no file at all no artifact
/// is kept, as the action's default `if-no-files-found: warn` does.
pub fn upload(workspace: &Path, paths: &str, store: &Path, name: &str) -> Result<usize> {
    check_name(name)?;
    let destination = store.join(name);
    if destination.exists() {
        bail!("artifact {name}이 이 run에서 이미 업로드됐어요.");
    }
    let mut roots = Vec::new();
    let mut files = Vec::new();
    for path in paths.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let relative = Path::new(path);
        let unsupported = path.contains(['*', '?', '[', '!'])
            || relative.is_absolute()
            || relative.components().any(|component| matches!(component, Component::ParentDir));
        if unsupported {
            bail!("upload-artifact의 path {path}는 재현이 지원하지 않는 형식이에요. workspace 안의 상대 경로만 쓸 수 있어요.");
        }
        let absolute = workspace.join(relative);
        if !absolute.exists() {
            continue;
        }
        let found = files_of(&absolute)?;
        // A folder is the root of its files; a file's root is its folder.
        roots.push(if absolute.is_dir() { absolute.clone() } else { absolute.parent().context("파일의 폴더가 없어요.")?.to_path_buf() });
        files.extend(found);
    }
    if files.is_empty() {
        return Ok(0);
    }
    let mut ancestor = roots.first().context("upload-artifact의 path가 비어 있어요.")?.clone();
    for root in &roots[1..] {
        while !root.starts_with(&ancestor) {
            ancestor = ancestor.parent().context("공통 상위 폴더가 없어요.")?.to_path_buf();
        }
    }
    let partial = store.join(format!("{name}.partial"));
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    for file in &files {
        let target = partial.join(file.strip_prefix(&ancestor)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &target).with_context(|| format!("{}를 artifact에 담지 못했어요.", file.display()))?;
    }
    std::fs::rename(&partial, &destination)?;
    Ok(files.len())
}

/// Download the artifact `name` from the run whose store is `store` into
/// `destination`. Returns the number of files.
pub fn download(store: &Path, name: &str, destination: &Path) -> Result<usize> {
    check_name(name)?;
    let source = store.join(name);
    if !source.is_dir() {
        bail!("artifact {name}이 그 run에 없어요: {}", store.display());
    }
    let files = files_of(&source)?;
    for file in &files {
        let target = destination.join(file.strip_prefix(&source)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &target).with_context(|| format!("{}를 받지 못했어요.", target.display()))?;
    }
    Ok(files.len())
}
