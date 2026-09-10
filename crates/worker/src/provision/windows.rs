//! Windows provisioning.
//!
//! Everything here was previously reached through .NET wrappers in PowerShell:
//! the registry, Authenticode verification, the administrator check and the
//! environment broadcast are direct Win32 calls now.

use super::{check_version, expected_versions, fetch, Diagnosis, Tools};
use crate::stream;
use crate::win32;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The WebView2 Evergreen Runtime client id, read from three hives in order.
const WEBVIEW2_CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const WEBVIEW2_INSTALLER: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

fn vswhere() -> PathBuf {
    let program_files =
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_owned());
    PathBuf::from(program_files).join("Microsoft Visual Studio").join("Installer").join("vswhere.exe")
}

/// The installed MSVC version, but only when every declared component is
/// present and the instance is complete. Absence is what triggers an install.
pub fn msvc_version(tools: &Tools) -> Option<String> {
    let path = vswhere();
    if !path.is_file() {
        return None;
    }
    let mut arguments = vec![
        "-products".to_owned(),
        "*".to_owned(),
        "-version".to_owned(),
        "[17.0,18.0)".to_owned(),
        "-requires".to_owned(),
    ];
    arguments.extend(tools.machine.msvc.components.iter().cloned());
    arguments.extend(["-format".to_owned(), "json".to_owned(), "-utf8".to_owned()]);
    let raw = stream::capture(&path.to_string_lossy(), &arguments, &tools.environment).ok()?;
    let instances: Vec<serde_json::Value> = serde_json::from_str(&raw).ok()?;
    instances
        .into_iter()
        .find(|instance| {
            instance.get("isComplete").and_then(|value| value.as_bool()).unwrap_or(false)
                && instance.get("isLaunchable").and_then(|value| value.as_bool()).unwrap_or(false)
        })
        .and_then(|instance| {
            instance.get("installationVersion").and_then(|value| value.as_str()).map(str::to_owned)
        })
}

pub fn webview2_version() -> Option<String> {
    for (hive, base) in [
        (win32::HKLM, "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients"),
        (win32::HKLM, "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients"),
        (win32::HKCU, "Software\\Microsoft\\EdgeUpdate\\Clients"),
    ] {
        let key = format!("{base}\\{WEBVIEW2_CLIENT}");
        if let Some(value) = win32::read_string(hive, &key, "pv") {
            if !value.is_empty() && value != "0.0.0.0" {
                return Some(value);
            }
        }
    }
    None
}

pub fn doctor(tools: &Tools) -> Result<Diagnosis> {
    let mut diagnosis = Diagnosis {
        platform: tools.platform,
        os_version: win32::product_name(),
        architecture: win32::machine_architecture().unwrap_or_else(|| std::env::consts::ARCH.to_owned()),
        tools: BTreeMap::new(),
        missing: Vec::new(),
        issues: Vec::new(),
        ready: false,
    };
    for (name, program, arguments, expected) in expected_versions(&tools.machine) {
        // pnpm and npm are `.cmd` shims, which CreateProcess cannot run.
        let program = match program {
            "pnpm" => "pnpm.cmd",
            other => other,
        };
        check_version(tools, &mut diagnosis, name, program, &arguments, &expected);
    }
    match msvc_version(tools) {
        Some(version) => {
            diagnosis.tools.insert("msvc".to_owned(), Some(version));
        }
        None => {
            diagnosis.tools.insert("msvc".to_owned(), None);
            diagnosis.missing.push("msvc-components".to_owned());
            diagnosis.issues.push("MSVC: required compiler or SDK components are missing".to_owned());
        }
    }
    match webview2_version() {
        Some(version) => {
            diagnosis.tools.insert("webview2".to_owned(), Some(version));
        }
        None => {
            diagnosis.tools.insert("webview2".to_owned(), None);
            diagnosis.missing.push("webview2".to_owned());
            diagnosis.issues.push("WebView2 runtime is missing".to_owned());
        }
    }
    diagnosis.missing.sort();
    diagnosis.missing.dedup();
    diagnosis.ready = diagnosis.missing.is_empty();
    Ok(diagnosis)
}

/// The Microsoft bootstrappers are not version-pinned, so trust comes from the
/// signature rather than a checksum.
fn verify_microsoft_signature(path: &Path) -> Result<()> {
    if !win32::verify_authenticode(path) {
        bail!("{} signature verification failed.", path.display());
    }
    let subject = win32::signer_subject(path).unwrap_or_default();
    if !subject.contains("Microsoft Corporation") {
        bail!("{} was not signed by Microsoft Corporation.", path.display());
    }
    Ok(())
}

pub fn setup_machine(tools: &Tools) -> Result<()> {
    if win32::machine_architecture().as_deref() != Some("ARM64") {
        bail!("This machine definition requires Windows ARM64.");
    }
    let msvc = msvc_version(tools);
    let webview2 = webview2_version();
    if msvc.is_some() && webview2.is_some() {
        println!("OK: machine-wide prerequisites present. No installation.");
        return Ok(());
    }
    // Elevation is only required to install. A run that has nothing to do
    // succeeds as the desktop user, which is what makes repeating it cheap.
    if !win32::is_administrator() {
        bail!("Run machine setup with administrator rights.");
    }
    let downloads = PathBuf::from(&tools.machine.windows_root).join("downloads");

    match msvc {
        Some(version) => println!("OK: MSVC {version}, required components present. No installation."),
        None => {
            let installer = downloads.join("vs_BuildTools.exe");
            fetch(&tools.machine.msvc.installer_url, &installer)?;
            verify_microsoft_signature(&installer)?;
            let mut arguments = vec![
                "--quiet".to_owned(),
                "--wait".to_owned(),
                "--norestart".to_owned(),
                "--installPath".to_owned(),
                tools.machine.msvc.install_path.clone(),
                "--addProductLang".to_owned(),
                "en-US".to_owned(),
            ];
            for component in &tools.machine.msvc.components {
                arguments.push("--add".to_owned());
                arguments.push(component.clone());
            }
            let finished = stream::run(&installer.to_string_lossy(), &arguments, None, &tools.environment, None)?;
            // 3010 means the install succeeded and wants a reboot.
            if finished.code != 0 && finished.code != 3010 {
                bail!("MSVC installer failed: {}.", finished.code);
            }
            let version = msvc_version(tools)
                .context("MSVC installation ended without the required components.")?;
            println!("Installed: MSVC {version}. Installer exit code {}.", finished.code);
        }
    }

    match webview2 {
        Some(version) => println!("OK: WebView2 {version}. No installation."),
        None => {
            let installer = downloads.join("MicrosoftEdgeWebview2Setup.exe");
            fetch(WEBVIEW2_INSTALLER, &installer)?;
            verify_microsoft_signature(&installer)?;
            let arguments = vec!["/silent".to_owned(), "/install".to_owned()];
            let finished = stream::run(&installer.to_string_lossy(), &arguments, None, &tools.environment, None)?;
            let version = webview2_version();
            if finished.code != 0 || version.is_none() {
                bail!("WebView2 installation failed: {}.", finished.code);
            }
            println!("Installed: WebView2 {}.", version.unwrap_or_default());
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, staging: &Path) -> Result<()> {
    std::fs::create_dir_all(staging)?;
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    zip.extract(staging)?;
    Ok(())
}

fn staging_for(tools: &Tools, name: &str) -> Result<PathBuf> {
    let staging = tools.root.join(format!(".extract-{name}"));
    if staging.exists() {
        bail!("An incomplete extraction exists at {}. Inspect it before retrying.", staging.display());
    }
    Ok(staging)
}

pub fn setup_user(tools: &Tools) -> Result<()> {
    std::fs::create_dir_all(&tools.root)?;
    let machine = &tools.machine;

    let node = tools.node_directory();
    if node.join("node.exe").exists() {
        println!("OK: declared Node.js already installed. No installation.");
    } else {
        let archive = tools.download(&machine.node.url, &machine.node.sha256, &format!("node-{}.zip", machine.node.version))?;
        let staging = staging_for(tools, "node")?;
        extract_zip(&archive, &staging)?;
        let inner = staging.join(node.file_name().unwrap());
        tools.install_extracted(&inner, &node)?;
        std::fs::remove_dir_all(&staging).ok();
    }

    let pnpm = tools.pnpm_directory();
    if pnpm.join("pnpm.cmd").exists() {
        println!("OK: declared pnpm already installed. No installation.");
    } else {
        let arguments = vec![
            "install".to_owned(),
            "--global".to_owned(),
            format!("pnpm@{}", machine.pnpm.version),
            "--prefix".to_owned(),
            pnpm.to_string_lossy().into_owned(),
        ];
        stream::checked(&node.join("npm.cmd").to_string_lossy(), &arguments, None, &tools.environment)?;
    }

    let rustup = tools.cargo_bin().join("rustup.exe");
    let wanted = tools.toolchain();
    if !rustup.exists() {
        let installer = tools.download(
            &machine.rust.installer_url,
            &machine.rust.installer_sha256,
            "rustup-init.exe",
        )?;
        let arguments = vec![
            "-y".to_owned(),
            "--profile".to_owned(),
            "minimal".to_owned(),
            "--default-host".to_owned(),
            machine.rust.host.clone(),
            "--default-toolchain".to_owned(),
            machine.rust.version.clone(),
            "--no-modify-path".to_owned(),
        ];
        stream::checked(&installer.to_string_lossy(), &arguments, None, &tools.environment)?;
    } else {
        let installed = stream::capture(
            &rustup.to_string_lossy(),
            &["toolchain".to_owned(), "list".to_owned()],
            &tools.environment,
        )?;
        if installed.lines().any(|line| line.trim_end().starts_with(&wanted)) {
            println!("OK: Rust {wanted} already installed. No installation.");
        } else {
            let arguments = vec![
                "toolchain".to_owned(),
                "install".to_owned(),
                wanted.clone(),
                "--profile".to_owned(),
                "minimal".to_owned(),
            ];
            stream::checked(&rustup.to_string_lossy(), &arguments, None, &tools.environment)?;
        }
    }

    let go = tools.go_directory();
    if go.join("go").join("bin").join("go.exe").exists() {
        println!("OK: declared Go already installed. No installation.");
    } else {
        let archive = tools.download(&machine.go.url, &machine.go.sha256, &format!("go-{}.zip", machine.go.version))?;
        let staging = staging_for(tools, "go")?;
        extract_zip(&archive, &staging)?;
        tools.install_extracted(&staging, &go)?;
    }

    let git = tools.git_directory();
    if git.join("cmd").join("git.exe").exists() {
        println!("OK: declared Git already installed. No installation.");
    } else {
        let installer = tools.download(&machine.git.url, &machine.git.sha256, &format!("git-{}.exe", machine.git.version))?;
        let arguments = vec![
            "/CURRENTUSER".to_owned(),
            "/VERYSILENT".to_owned(),
            "/SUPPRESSMSGBOXES".to_owned(),
            "/NORESTART".to_owned(),
            "/NOCANCEL".to_owned(),
            "/SP-".to_owned(),
            format!("/DIR={}", git.display()),
        ];
        let finished = stream::run(&installer.to_string_lossy(), &arguments, None, &tools.environment, None)?;
        if finished.code != 0 {
            bail!("Git installer failed: {}.", finished.code);
        }
    }

    update_user_path(tools)?;
    let diagnosis = tools.doctor()?;
    println!("{}", serde_json::to_string_pretty(&diagnosis)?);
    if !diagnosis.ready {
        bail!("Windows tool diagnosis failed: {}", diagnosis.missing.join(", "));
    }
    Ok(())
}

/// Append the managed directories to the user's PATH exactly once.
///
/// Nothing is written when the merged value is unchanged, so repeating setup
/// does not add a duplicate entry or rewrite the registry.
fn update_user_path(tools: &Tools) -> Result<()> {
    let managed = vec![
        tools.node_directory(),
        tools.pnpm_directory(),
        tools.go_directory().join("go").join("bin"),
        tools.git_directory().join("cmd"),
        tools.cargo_bin(),
        super::home_directory()?.join("go").join("bin"),
    ];
    let existing = win32::read_string(win32::HKCU, "Environment", "Path").unwrap_or_default();
    let mut parts: Vec<String> =
        existing.split(';').filter(|value| !value.is_empty()).map(str::to_owned).collect();
    for path in managed {
        let text = path.to_string_lossy().into_owned();
        if !parts.iter().any(|part| part.eq_ignore_ascii_case(&text)) {
            parts.push(text);
        }
    }
    let updated = parts.join(";");
    if updated == existing {
        println!("OK: user PATH already contains the required entries. No change.");
        return Ok(());
    }
    win32::write_user_path(&updated)?;
    println!("Updated user PATH.");
    Ok(())
}
