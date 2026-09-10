//! Building a project on the selected operating system's local disk.
//!
//! A successful result is reused only when its source, recipe, worker code,
//! machine configuration and executable checksum all still match. Anything
//! else is a fresh build.

use crate::provision::{workspace, Tools};
use crate::stream;
use anyhow::{bail, Context, Result};
use build_machine_core::request::{Framework, WorkRequest};
use build_machine_core::source::sha256_file;
use build_machine_core::Platform;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub project: String,
    pub project_key: String,
    pub build_id: String,
    pub revision: String,
    pub dirty: bool,
    pub source_sha256: String,
    pub platform: Platform,
    pub built_at: String,
    pub command: String,
    pub executable: String,
    pub executable_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub architectures: String,
    pub artifacts: Vec<build_machine_core::report::Artifact>,
    pub log: String,
}

/// What makes one build distinguishable from another. Changing the worker
/// binary changes this, so a rebuilt worker never reuses an older result.
fn build_id(request: &WorkRequest, tools: &Tools) -> Result<String> {
    let worker = std::env::current_exe().ok().and_then(|path| sha256_file(&path).ok()).unwrap_or_default();
    let recipe = serde_json::json!({
        "framework": request.framework,
        "command": request.command,
        "artifact": request.artifact,
        "target": request.target,
        "bundle": request.bundle,
    });
    let mut hasher = Sha256::new();
    hasher.update(request.snapshot.source_hash.as_bytes());
    hasher.update(worker.as_bytes());
    hasher.update(serde_json::to_vec(&recipe)?);
    hasher.update(serde_json::to_vec(&tools.machine)?);
    Ok(hex::encode(hasher.finalize()))
}

pub fn project_root(request: &WorkRequest) -> Result<PathBuf> {
    Ok(workspace()?.join(request.project_key()?))
}

/// Unpack the archive once, refusing to reuse a partial extraction.
pub fn extract_source(archive: &Path, expected: &str, directory: &Path) -> Result<PathBuf> {
    let source = directory.join("source");
    let marker = directory.join("source-ready");
    if sha256_file(archive)? != expected {
        bail!("Source snapshot checksum mismatch.");
    }
    std::fs::create_dir_all(directory)?;
    if marker.exists() {
        return Ok(source);
    }
    if source.exists() {
        bail!("Incomplete source extraction at {}. Inspect it before retrying.", source.display());
    }
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let root = source.clone();
    std::fs::create_dir_all(&root)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("Archive path escapes the extraction directory.");
        };
        let destination = root.join(relative);
        if !destination.starts_with(&root) {
            bail!("Archive path escapes the extraction directory.");
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&destination)?;
        std::io::copy(&mut entry, &mut output)?;
        drop(output);
        // An executable source script must stay executable, or the recipe that
        // runs it fails on a permission the snapshot recorded correctly.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    std::fs::write(&marker, expected)?;
    Ok(source)
}

/// Remove signing credentials a developer shell may carry. Local rehearsal
/// never uses production signing, and the original stores are untouched.
fn build_environment(tools: &Tools) -> Vec<(String, String)> {
    tools
        .environment
        .iter()
        .filter(|(key, _)| {
            !key.starts_with("APPLE_")
                && !matches!(
                    key.as_str(),
                    "TAURI_SIGNING_PRIVATE_KEY" | "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" | "GITHUB_TOKEN"
                )
        })
        .cloned()
        .collect()
}

fn package_manager(source: &Path) -> Result<(&'static str, Vec<String>, Vec<String>)> {
    if source.join("pnpm-lock.yaml").is_file() {
        Ok((
            "pnpm",
            vec!["install".to_owned(), "--frozen-lockfile".to_owned()],
            vec!["exec".to_owned(), "tauri".to_owned(), "build".to_owned()],
        ))
    } else if source.join("package-lock.json").is_file() {
        Ok((
            "npm",
            vec!["ci".to_owned()],
            vec!["exec".to_owned(), "--".to_owned(), "tauri".to_owned(), "build".to_owned()],
        ))
    } else {
        bail!("Use a custom recipe for a project without an npm or pnpm lockfile.")
    }
}

fn executable_name(program: &str) -> String {
    if cfg!(windows) {
        format!("{program}.cmd")
    } else {
        program.to_owned()
    }
}

/// Find the single produced executable, or use the one the recipe named.
fn locate_executable(source: &Path, output: &Path, artifact: Option<&str>) -> Result<PathBuf> {
    if let Some(artifact) = artifact {
        let path = source.join(artifact);
        let resolved = path.canonicalize().unwrap_or(path);
        if !resolved.starts_with(source) {
            bail!("The executable must be inside the project build directory.");
        }
        return Ok(resolved);
    }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(output).with_context(|| format!("빌드 출력이 없어요: {}", output.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if cfg!(windows) {
            if name.ends_with(".exe") {
                candidates.push(path);
            }
        } else if !name.contains('.') && is_executable(&path) {
            candidates.push(path);
        }
    }
    if candidates.len() != 1 {
        bail!("Specify --artifact when the build produces zero or multiple candidate executables.");
    }
    Ok(candidates.remove(0))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|value| value.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// What a Tauri build produced on this platform.
struct Produced {
    executable: PathBuf,
    app: Option<PathBuf>,
    architectures: String,
}

/// The bundle a development build asks for, and the package extension a
/// release rehearsal collects. A worker is built for one platform, so these
/// are compile-time facts rather than runtime branches.
#[cfg(target_os = "macos")]
const DEVELOPMENT_BUNDLE: &str = "app";
#[cfg(target_os = "macos")]
const PACKAGE_EXTENSION: &str = "dmg";
#[cfg(target_os = "linux")]
const DEVELOPMENT_BUNDLE: &str = "deb";
#[cfg(target_os = "linux")]
const PACKAGE_EXTENSION: &str = "deb";
#[cfg(target_os = "windows")]
const DEVELOPMENT_BUNDLE: &str = "none";
#[cfg(target_os = "windows")]
const PACKAGE_EXTENSION: &str = "exe";

#[cfg(target_os = "macos")]
fn tauri_output(
    _source: &Path,
    output: &Path,
    _artifact: Option<&str>,
    environment: &[(String, String)],
    _target: &str,
) -> Result<Produced> {
    let (app, executable) = crate::macos::locate_app(output)?;
    let architectures = crate::macos::architectures(&executable, environment)?;
    Ok(Produced { executable, app: Some(app), architectures })
}

#[cfg(not(target_os = "macos"))]
fn tauri_output(
    source: &Path,
    output: &Path,
    artifact: Option<&str>,
    _environment: &[(String, String)],
    target: &str,
) -> Result<Produced> {
    let executable = locate_executable(source, output, artifact)?;
    Ok(Produced { executable, app: None, architectures: target.to_owned() })
}

pub fn build(request: &WorkRequest, tools: &Tools, release: bool) -> Result<Receipt> {
    let root = project_root(request)?;
    let id = build_id(request, tools)?;
    let directory = root.join(&id[..24]);
    let receipt_path = directory.join("result.json");
    let latest = root.join("latest.json");

    if !release && receipt_path.is_file() {
        if let Ok(previous) = serde_json::from_slice::<Receipt>(&std::fs::read(&receipt_path)?) {
            let executable = PathBuf::from(&previous.executable);
            if executable.exists() && sha256_file(&executable)? == previous.executable_sha256 {
                build_machine_core::report::write_atomic(&latest, &previous)?;
                println!("REUSED BUILD: {}", previous.executable);
                return Ok(previous);
            }
        }
    }

    let source = extract_source(Path::new(&request.archive), &request.snapshot.source_hash, &directory)?;
    let environment = build_environment(tools);
    let framework = request.framework.unwrap_or(Framework::Tauri);
    let target = request.target.clone();
    let mut commands: Vec<String> = Vec::new();
    let (executable, app, artifacts, architectures);

    match framework {
        Framework::Tauri => {
            if !source.join("src-tauri/Cargo.lock").is_file() {
                bail!("Tauri builds require src-tauri/Cargo.lock.");
            }
            let (manager, install, base) = package_manager(&source)?;
            let program = executable_name(manager);
            stream::checked(&program, &install, Some(&source), &environment)?;
            commands.push(format!("{program} {}", install.join(" ")));
            let bundle = if release {
                tools.profile()?.bundle.clone().unwrap_or_else(|| DEVELOPMENT_BUNDLE.to_owned())
            } else {
                DEVELOPMENT_BUNDLE.to_owned()
            };
            let mut arguments = base.clone();
            arguments.extend([
                "--ci".to_owned(),
                "--no-sign".to_owned(),
                "--target".to_owned(),
                target.clone(),
                "--bundles".to_owned(),
                bundle,
                "--".to_owned(),
                "--locked".to_owned(),
            ]);
            stream::checked(&program, &arguments, Some(&source), &environment)?;
            commands.push(format!("{program} {}", arguments.join(" ")));
            let output = source.join(format!("src-tauri/target/{target}/release"));
            let produced = tauri_output(&source, &output, request.artifact.as_deref(), &environment, &target)?;
            executable = produced.executable;
            app = produced.app;
            architectures = produced.architectures;
            artifacts = collect_artifacts(&output.join("bundle"), PACKAGE_EXTENSION)?;
        }
        Framework::Custom => {
            let command = request.command.clone().context("Custom builds require a command.")?;
            let artifact = request.artifact.clone().context("Custom builds require an artifact.")?;
            let (shell, flags) = shell_for();
            let mut arguments = flags;
            arguments.push(command.clone());
            stream::checked(shell, &arguments, Some(&source), &environment)?;
            commands.push(command);
            executable = locate_executable(&source, &source, Some(&artifact))?;
            app = None;
            architectures = target.clone();
            artifacts = vec![artifact_of(&executable)?];
        }
        Framework::Wails2 => {
            bail!("Automatic recipes currently cover Tauri; supply a custom command for Wails.")
        }
    }

    if !executable.is_file() {
        bail!("The build did not produce the expected executable.");
    }
    let receipt = Receipt {
        project: request.snapshot.project.clone(),
        project_key: request.snapshot.project_key.clone(),
        build_id: id,
        revision: request.snapshot.revision.clone(),
        dirty: request.snapshot.dirty,
        source_sha256: request.snapshot.source_hash.clone(),
        platform: tools.platform,
        built_at: build_machine_core::now(),
        command: commands.join(" && "),
        executable_sha256: sha256_file(&executable)?,
        executable: executable.to_string_lossy().into_owned(),
        app: app.map(|path| path.to_string_lossy().into_owned()),
        architectures,
        artifacts,
        log: directory.join("build.log").to_string_lossy().into_owned(),
    };
    build_machine_core::report::write_atomic(&receipt_path, &receipt)?;
    build_machine_core::report::write_atomic(&latest, &receipt)?;
    println!("BUILT: {}", receipt.executable);
    Ok(receipt)
}

pub fn shell_for() -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        ("cmd.exe", vec!["/d".to_owned(), "/s".to_owned(), "/c".to_owned()])
    } else {
        ("/bin/sh", vec!["-eu".to_owned(), "-c".to_owned()])
    }
}

fn artifact_of(path: &Path) -> Result<build_machine_core::report::Artifact> {
    Ok(build_machine_core::report::Artifact {
        path: path.to_string_lossy().into_owned(),
        sha256: sha256_file(path)?,
        size: std::fs::metadata(path)?.len(),
    })
}

pub fn collect_artifacts(directory: &Path, extension: &str) -> Result<Vec<build_machine_core::report::Artifact>> {
    let mut artifacts = Vec::new();
    if !directory.exists() {
        return Ok(artifacts);
    }
    for entry in walkdir::WalkDir::new(directory).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some(extension)
        {
            artifacts.push(artifact_of(entry.path())?);
        }
    }
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(artifacts)
}
