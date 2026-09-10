//! Staging the worker binaries the application bundle ships.
//!
//! The host worker is compiled here. A guest worker cannot be cross-linked
//! from macOS — the Windows linker lives inside Windows — so it is compiled in
//! its own virtual machine, which is also where a release runner would build
//! it natively.

use crate::guest;
use anyhow::{bail, Context, Result};
use build_machine_core::config::Machine;
use build_machine_core::Platform;
use std::path::{Path, PathBuf};

/// Where the controller looks for each worker.
pub fn staged_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "build-machine-worker-windows.exe",
        Platform::Linux => "build-machine-worker-linux",
        Platform::Macos => "build-machine-worker-macos",
    }
}

/// The cargo that invoked this task, so the toolchain stays the same one.
pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        let bundled = PathBuf::from(&home).join(".cargo/bin/cargo");
        if bundled.is_file() {
            bundled.to_string_lossy().into_owned()
        } else {
            "cargo".to_owned()
        }
    })
}

/// The triple this machine actually compiles for. `machine.json` names
/// `universal-apple-darwin` for macOS, which is a Tauri bundling target rather
/// than one rustc can build, so the real triple has to come from rustc.
fn host_triple() -> Result<String> {
    let output = std::process::Command::new("rustc").arg("-vV").output();
    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => {
            let home = std::env::var("HOME").unwrap_or_default();
            std::process::Command::new(PathBuf::from(home).join(".cargo/bin/rustc"))
                .arg("-vV")
                .output()
                .context("rustc를 실행하지 못했어요.")?
        }
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .context("rustc가 host 트리플을 알려주지 않았어요.")
}

/// Tauri stages a sidecar by target triple and fails the build when the file
/// is missing, which is what forces the workers to exist before the app.
pub fn sidecar_path(root: &Path) -> Result<PathBuf> {
    Ok(root.join("gui/src-tauri/binaries").join(format!("build-machine-worker-{}", host_triple()?)))
}

pub fn build(root: &Path, platforms: &[Platform]) -> Result<()> {
    let machine = Machine::load(&root.join("machine.json"))?;
    let workers = root.join("workers");
    std::fs::create_dir_all(&workers)?;
    let host = Platform::host()?;
    for platform in platforms {
        let produced = if *platform == host {
            build_here(root, &machine, *platform)?
        } else {
            guest::build_worker(root, &machine, *platform)?
        };
        let staged = workers.join(staged_name(*platform));
        std::fs::copy(&produced, &staged)
            .with_context(|| format!("워커를 배치하지 못했어요: {}", staged.display()))?;
        println!("Worker: {}", staged.display());
        if *platform == host {
            let sidecar = sidecar_path(root)?;
            std::fs::create_dir_all(sidecar.parent().unwrap())?;
            std::fs::copy(&produced, &sidecar)?;
            println!("Sidecar: {}", sidecar.display());
        }
    }
    Ok(())
}

fn build_target(root: &Path, target: &str) -> Result<PathBuf> {
    crate::run(
        &cargo(),
        &["build", "--release", "--locked", "-p", "build-machine-worker", "--target", target],
        root,
    )?;
    let produced = root.join("target").join(target).join("release").join(binary_name());
    if !produced.is_file() {
        bail!("워커 바이너리가 만들어지지 않았어요: {}", produced.display());
    }
    Ok(produced)
}

/// Build for this machine.
///
/// A macOS worker travels inside an application that runs on both
/// architectures, so it is built for each and joined, exactly as the release
/// runner does.
fn build_here(root: &Path, machine: &Machine, platform: Platform) -> Result<PathBuf> {
    if platform != Platform::Macos {
        return build_target(root, &machine.profile(platform)?.target);
    }
    let arm = build_target(root, "aarch64-apple-darwin")?;
    let intel = build_target(root, "x86_64-apple-darwin")?;
    let universal = root.join("target").join("universal-apple-darwin").join("release");
    std::fs::create_dir_all(&universal)?;
    let joined = universal.join(binary_name());
    crate::run(
        "lipo",
        &["-create", "-output", &joined.to_string_lossy(), &arm.to_string_lossy(), &intel.to_string_lossy()],
        root,
    )?;
    crate::run("lipo", &["-archs", &joined.to_string_lossy()], root)?;
    Ok(joined)
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "build-machine-worker.exe"
    } else {
        "build-machine-worker"
    }
}
