//! Checking the package a release rehearsal produced.
//!
//! Compiling, packaging, installing and launching are separate results. This
//! goes one step past "a file appeared": it opens the package and confirms it
//! contains what it claims to. It deliberately stops short of a system-level
//! installation, and says so in the record rather than implying more.

#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::stream;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use anyhow::Context;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub struct Checked {
    /// What was actually established, recorded in the receipt.
    pub description: String,
    /// Where the package's application landed, when opening it produced one.
    pub installed: Option<PathBuf>,
}

/// Mount the disk image read-only and copy the application out of it, so the
/// check exercises the thing a person would actually open.
#[cfg(target_os = "macos")]
pub fn check(package: &Path, directory: &Path, environment: &[(String, String)]) -> Result<Checked> {
    let mount = directory.join("mounted-package");
    std::fs::create_dir_all(&mount)?;
    let arguments = vec![
        "attach".to_owned(),
        "-readonly".to_owned(),
        "-nobrowse".to_owned(),
        "-mountpoint".to_owned(),
        mount.to_string_lossy().into_owned(),
        package.to_string_lossy().into_owned(),
    ];
    stream::checked("hdiutil", &arguments, None, environment)?;
    let outcome = (|| -> Result<PathBuf> {
        let mut applications: Vec<PathBuf> = std::fs::read_dir(&mount)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
            .collect();
        if applications.len() != 1 {
            bail!("The disk image does not contain exactly one application.");
        }
        let application = applications.remove(0);
        let installed = directory.join("installed").join(application.file_name().unwrap());
        std::fs::create_dir_all(installed.parent().unwrap())?;
        if installed.exists() {
            std::fs::remove_dir_all(&installed)?;
        }
        let copy = vec![
            application.to_string_lossy().into_owned(),
            installed.to_string_lossy().into_owned(),
        ];
        stream::checked("ditto", &copy, None, environment)?;
        Ok(installed)
    })();
    // The image is detached whether or not it held what was expected.
    let detach = vec!["detach".to_owned(), mount.to_string_lossy().into_owned()];
    let detached = stream::checked("hdiutil", &detach, None, environment);
    let installed = outcome?;
    detached?;
    Ok(Checked {
        description: "disk image mounted read-only; the application it carries was copied to an isolated directory"
            .to_owned(),
        installed: Some(installed),
    })
}

/// Read the package's own metadata. Extracting it would not exercise the
/// maintainer scripts, so this does not claim an installation happened.
#[cfg(target_os = "linux")]
pub fn check(package: &Path, _directory: &Path, environment: &[(String, String)]) -> Result<Checked> {
    let arguments = vec!["--info".to_owned(), package.to_string_lossy().into_owned()];
    stream::checked("dpkg-deb", &arguments, None, environment)
        .context("패키지 메타데이터를 읽지 못했어요.")?;
    let contents = vec!["--contents".to_owned(), package.to_string_lossy().into_owned()];
    let listing = stream::checked("dpkg-deb", &contents, None, environment)?;
    if !listing.contains("/usr/bin/") {
        bail!("The package does not carry an executable under /usr/bin.");
    }
    Ok(Checked {
        description: "package metadata and contents read; a system installation was not performed".to_owned(),
        installed: None,
    })
}

/// Confirm the installer is a real signed-or-unsigned executable image and
/// carries the payload, without running it. Running an installer would change
/// the machine, which a rehearsal must not do.
#[cfg(target_os = "windows")]
pub fn check(package: &Path, _directory: &Path, _environment: &[(String, String)]) -> Result<Checked> {
    let data = std::fs::read(package).with_context(|| format!("패키지를 읽지 못했어요: {}", package.display()))?;
    if data.len() < 0x40 || &data[..2] != b"MZ" {
        bail!("The installer is not a Windows executable image.");
    }
    let offset = u32::from_le_bytes([data[0x3c], data[0x3d], data[0x3e], data[0x3f]]) as usize;
    if data.len() < offset + 4 || &data[offset..offset + 4] != b"PE\0\0" {
        bail!("The installer is not a Windows executable image.");
    }
    Ok(Checked {
        description: "installer confirmed as a Windows executable image; it was not run, so no installation was performed"
            .to_owned(),
        installed: None,
    })
}
