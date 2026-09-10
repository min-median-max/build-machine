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

/// The sidecar name Tauri stages for the host, which must carry the target
/// triple so the bundler can find it.
pub fn sidecar_path(root: &Path, target: &str) -> PathBuf {
    root.join("gui/src-tauri/binaries").join(format!("build-machine-worker-{target}"))
}

pub fn build(root: &Path, platforms: &[Platform]) -> Result<()> {
    let machine = Machine::load(&root.join("machine.json"))?;
    let workers = root.join("workers");
    std::fs::create_dir_all(&workers)?;
    for platform in platforms {
        let profile = machine.profile(*platform)?;
        let produced = if *platform == Platform::host()? {
            build_here(root, &profile.target)?
        } else {
            guest::build_worker(root, &machine, *platform)?
        };
        let staged = workers.join(staged_name(*platform));
        std::fs::copy(&produced, &staged)
            .with_context(|| format!("워커를 배치하지 못했어요: {}", staged.display()))?;
        println!("Worker: {}", staged.display());
        if *platform == Platform::host()? {
            let sidecar = sidecar_path(root, &profile.target);
            std::fs::create_dir_all(sidecar.parent().unwrap())?;
            std::fs::copy(&produced, &sidecar)?;
            println!("Sidecar: {}", sidecar.display());
        }
    }
    Ok(())
}

fn build_here(root: &Path, target: &str) -> Result<PathBuf> {
    crate::run(
        "cargo",
        &["build", "--release", "--locked", "-p", "build-machine-worker", "--target", target],
        root,
    )?;
    let produced = root.join("target").join(target).join("release").join(binary_name());
    if !produced.is_file() {
        bail!("워커 바이너리가 만들어지지 않았어요: {}", produced.display());
    }
    Ok(produced)
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "build-machine-worker.exe"
    } else {
        "build-machine-worker"
    }
}
