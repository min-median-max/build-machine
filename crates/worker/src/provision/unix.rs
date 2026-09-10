//! macOS and Linux provisioning.
//!
//! System packages are the platform's own responsibility (Apple Command Line
//! Tools, apt). Everything the machine declares a version for is installed
//! privately under the managed root so the system's own Node.js and Go
//! installations are preserved.

use super::{check_version, expected_versions, Diagnosis, Tools};
use crate::stream;
use anyhow::{bail, Context, Result};
use build_machine_core::Platform;
use std::collections::BTreeMap;
use std::path::Path;

fn missing_system_packages(tools: &Tools) -> Result<Vec<String>> {
    if tools.platform == Platform::Macos {
        for arguments in [vec!["--find", "clang"], vec!["--show-sdk-path"]] {
            let arguments: Vec<String> = arguments.into_iter().map(str::to_owned).collect();
            if stream::capture("xcrun", &arguments, &tools.environment).is_err() {
                return Ok(vec!["Apple Command Line Tools".to_owned()]);
            }
        }
        return Ok(Vec::new());
    }
    let profile = tools.profile()?;
    let mut missing = Vec::new();
    for package in ["git", "python3"].iter().map(|value| (*value).to_owned()).chain(profile.packages.iter().cloned()) {
        let arguments = vec!["-W".to_owned(), "-f=${Status}".to_owned(), package.clone()];
        match stream::capture("dpkg-query", &arguments, &tools.environment) {
            Ok(status) if status.trim() == "install ok installed" => {}
            _ => missing.push(package),
        }
    }
    Ok(missing)
}

fn os_version(tools: &Tools) -> Option<String> {
    if tools.platform == Platform::Macos {
        stream::capture("sw_vers", &["-productVersion".to_owned()], &tools.environment)
            .ok()
            .map(|value| value.trim().to_owned())
    } else {
        std::fs::read_to_string("/etc/os-release").ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                .map(|value| value.trim_matches('"').to_owned())
        })
    }
}

pub fn doctor(tools: &Tools) -> Result<Diagnosis> {
    let mut diagnosis = Diagnosis {
        platform: tools.platform,
        os_version: os_version(tools),
        architecture: std::env::consts::ARCH.to_owned(),
        tools: BTreeMap::new(),
        missing: missing_system_packages(tools)?,
        issues: Vec::new(),
        ready: false,
    };
    for (name, program, arguments, expected) in expected_versions(&tools.machine) {
        check_version(tools, &mut diagnosis, name, program, &arguments, &expected);
    }
    diagnosis.missing.sort();
    diagnosis.missing.dedup();
    diagnosis.ready = diagnosis.missing.is_empty();
    Ok(diagnosis)
}

pub fn setup_system(tools: &Tools) -> Result<()> {
    let missing = missing_system_packages(tools)?;
    if missing.is_empty() {
        println!("OK: native system dependencies present. No installation.");
        return Ok(());
    }
    if tools.platform == Platform::Macos {
        stream::checked("xcode-select", &["--install".to_owned()], None, &tools.environment).ok();
        bail!("Apple Command Line Tools installation was opened. Finish that system installer, then rerun the same command.");
    }
    if unsafe { geteuid() } != 0 {
        bail!("Linux system packages require root. The controller runs setup-system as root.");
    }
    let mut environment = tools.environment.clone();
    environment.push(("DEBIAN_FRONTEND".to_owned(), "noninteractive".to_owned()));
    stream::checked("apt-get", &["update".to_owned()], None, &environment)?;
    let mut arguments = vec!["install".to_owned(), "-y".to_owned()];
    arguments.extend(missing);
    stream::checked("apt-get", &arguments, None, &environment)?;
    Ok(())
}

extern "C" {
    fn geteuid() -> u32;
}

/// Unpack a tarball into a staging directory beside the managed root.
fn extract_tar(archive: &Path, staging: &Path) -> Result<()> {
    std::fs::create_dir_all(staging)?;
    let arguments = vec![
        "-xf".to_owned(),
        archive.to_string_lossy().into_owned(),
        "-C".to_owned(),
        staging.to_string_lossy().into_owned(),
    ];
    stream::checked("tar", &arguments, None, &[])?;
    Ok(())
}

fn staging_for(tools: &Tools, name: &str) -> Result<std::path::PathBuf> {
    let staging = tools.root.join(format!(".extract-{name}"));
    if staging.exists() {
        bail!("An incomplete extraction exists at {}. Inspect it before retrying.", staging.display());
    }
    Ok(staging)
}

pub fn setup_user(tools: &Tools) -> Result<()> {
    let profile = tools.profile()?.clone();
    std::fs::create_dir_all(&tools.root)?;

    let node = tools.node_directory();
    if node.join("bin").join("node").exists() {
        println!("OK: declared Node.js already installed. No installation.");
    } else {
        let url = profile.node_url.clone().context("machine.json에 nodeUrl이 없어요.")?;
        let sha = profile.node_sha256.clone().context("machine.json에 nodeSha256이 없어요.")?;
        let name = url.rsplit('/').next().unwrap_or("node-archive").to_owned();
        let archive = tools.download(&url, &sha, &name)?;
        let staging = staging_for(tools, "node")?;
        extract_tar(&archive, &staging)?;
        let inner = staging.join(node.file_name().unwrap());
        tools.install_extracted(&inner, &node)?;
        std::fs::remove_dir_all(&staging).ok();
    }

    let pnpm = tools.pnpm_directory();
    if pnpm.join("bin").join("pnpm").exists() {
        println!("OK: declared pnpm already installed. No installation.");
    } else {
        let arguments = vec![
            "install".to_owned(),
            "--global".to_owned(),
            format!("pnpm@{}", tools.machine.pnpm.version),
            "--prefix".to_owned(),
            pnpm.to_string_lossy().into_owned(),
        ];
        stream::checked(&node.join("bin").join("npm").to_string_lossy(), &arguments, None, &tools.environment)?;
    }

    let rustup = tools.cargo_bin().join("rustup");
    if !rustup.exists() {
        let url = profile.rustup_url.clone().context("machine.json에 rustupUrl이 없어요.")?;
        let sha = profile.rustup_sha256.clone().context("machine.json에 rustupSha256이 없어요.")?;
        let installer = tools.download(&url, &sha, &format!("rustup-init-{}", tools.platform))?;
        set_executable(&installer)?;
        let arguments = vec![
            "-y".to_owned(),
            "--profile".to_owned(),
            "minimal".to_owned(),
            "--default-toolchain".to_owned(),
            tools.machine.rust.version.clone(),
            "--no-modify-path".to_owned(),
        ];
        stream::checked(&installer.to_string_lossy(), &arguments, None, &tools.environment)?;
    } else {
        let installed = stream::capture(
            &rustup.to_string_lossy(),
            &["toolchain".to_owned(), "list".to_owned()],
            &tools.environment,
        )?;
        let wanted = format!("{}-", tools.machine.rust.version);
        if installed.lines().any(|line| line.starts_with(&wanted)) {
            println!("OK: declared Rust already installed. No installation.");
        } else {
            let arguments = vec![
                "toolchain".to_owned(),
                "install".to_owned(),
                tools.machine.rust.version.clone(),
                "--profile".to_owned(),
                "minimal".to_owned(),
            ];
            stream::checked(&rustup.to_string_lossy(), &arguments, None, &tools.environment)?;
        }
    }

    let targets: Vec<String> = if tools.platform == Platform::Macos {
        vec!["aarch64-apple-darwin".to_owned(), "x86_64-apple-darwin".to_owned()]
    } else {
        vec![profile.target.clone()]
    };
    let installed = stream::capture(
        &rustup.to_string_lossy(),
        &[
            "target".to_owned(),
            "list".to_owned(),
            "--installed".to_owned(),
            "--toolchain".to_owned(),
            tools.machine.rust.version.clone(),
        ],
        &tools.environment,
    )?;
    for target in targets {
        if !installed.lines().any(|line| line.trim() == target) {
            let arguments = vec![
                "target".to_owned(),
                "add".to_owned(),
                target,
                "--toolchain".to_owned(),
                tools.machine.rust.version.clone(),
            ];
            stream::checked(&rustup.to_string_lossy(), &arguments, None, &tools.environment)?;
        }
    }

    let go = tools.go_directory();
    if go.join("go").join("bin").join("go").exists() {
        println!("OK: declared Go already installed. No installation.");
    } else {
        let url = profile.go_url.clone().context("machine.json에 goUrl이 없어요.")?;
        let sha = profile.go_sha256.clone().context("machine.json에 goSha256이 없어요.")?;
        let name = url.rsplit('/').next().unwrap_or("go-archive").to_owned();
        let archive = tools.download(&url, &sha, &name)?;
        let staging = staging_for(tools, "go")?;
        extract_tar(&archive, &staging)?;
        tools.install_extracted(&staging, &go)?;
    }

    let diagnosis = tools.doctor()?;
    println!("{}", serde_json::to_string_pretty(&diagnosis)?);
    if !diagnosis.ready {
        bail!("Native tool diagnosis failed: {}", diagnosis.missing.join(", "));
    }
    Ok(())
}

fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}
