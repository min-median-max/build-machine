//! `actions/upload-pages-artifact` and `actions/deploy-pages`.
//!
//! The upload keeps the site as it was when the step ran, as the action's
//! archive does, for the rest of the replay. The deployment publishes nothing:
//! it records the files it would have deployed, with their sizes and SHA-256.

use anyhow::{bail, Context, Result};
use build_machine_core::report::Artifact;
use std::path::Path;

/// The action's archive leaves these out wherever they are.
const EXCLUDED: [&str; 2] = [".git", ".github"];

fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'));
    if !valid {
        bail!("pages 산출물 이름에는 영문자, 숫자, '-', '_', '.'만 쓸 수 있어요: {name}");
    }
    Ok(())
}

/// Every file of `directory` with links followed, as the action's archive
/// dereferences them, without `.git` and `.github`.
fn files(directory: &Path) -> Result<Vec<walkdir::DirEntry>> {
    let mut found = Vec::new();
    let walk = walkdir::WalkDir::new(directory)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !EXCLUDED.contains(&entry.file_name().to_string_lossy().as_ref()));
    for entry in walk {
        let entry = entry.with_context(|| format!("{}를 읽지 못했어요.", directory.display()))?;
        if entry.file_type().is_file() {
            found.push(entry);
        }
    }
    Ok(found)
}

/// Keep `path` under the workspace as the artifact `name` in `store`. A name
/// is uploaded once in a run, as on GitHub. Returns the number of files.
pub fn upload(workspace: &Path, path: &str, store: &Path, name: &str) -> Result<usize> {
    check_name(name)?;
    let source = crate::build::contained_in(workspace, &workspace.join(path))
        .filter(|source| source.is_dir())
        .with_context(|| format!("upload-pages-artifact의 path가 workspace 안의 폴더가 아니에요: {path}"))?;
    let destination = store.join(name);
    if destination.exists() {
        bail!("pages 산출물 {name}이 이 재현에서 이미 업로드됐어요.");
    }
    let partial = store.join(format!("{name}.partial"));
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    let found = files(&source)?;
    for entry in &found {
        let relative = entry.path().strip_prefix(&source)?;
        let target = partial.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(entry.path(), &target)
            .with_context(|| format!("{}를 pages 산출물에 담지 못했어요.", entry.path().display()))?;
    }
    std::fs::create_dir_all(&partial)?;
    std::fs::rename(&partial, &destination)?;
    Ok(found.len())
}

/// The files of the artifact `name` an earlier step uploaded, sorted by path.
pub fn deploy(store: &Path, name: &str) -> Result<Vec<Artifact>> {
    check_name(name)?;
    let directory = store.join(name);
    if !directory.is_dir() {
        bail!("deploy-pages가 배포할 pages 산출물 {name}이 이 재현에서 앞서 업로드되지 않았어요.");
    }
    let mut deployed = Vec::new();
    for entry in files(&directory)? {
        let relative = entry.path().strip_prefix(&directory)?;
        let path = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        deployed.push(Artifact {
            path,
            sha256: build_machine_core::source::sha256_file(entry.path())?,
            size: entry.metadata()?.len(),
        });
    }
    deployed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(deployed)
}
