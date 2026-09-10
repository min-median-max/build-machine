//! macOS bundle handling.
#![cfg(target_os = "macos")]

use crate::stream;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// The single `.app` a Tauri build produced, and the executable inside it.
pub fn locate_app(output: &Path) -> Result<(PathBuf, PathBuf)> {
    let directory = output.join("bundle/macos");
    let mut applications: Vec<PathBuf> = std::fs::read_dir(&directory)
        .with_context(|| format!("앱 번들 출력이 없어요: {}", directory.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .collect();
    if applications.len() != 1 {
        bail!("Expected exactly one macOS app bundle.");
    }
    let application = applications.remove(0);
    let plist = application.join("Contents/Info.plist");
    let value: plist::Value = plist::from_file(&plist)
        .with_context(|| format!("Info.plist를 읽지 못했어요: {}", plist.display()))?;
    let name = value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleExecutable"))
        .and_then(|value| value.as_string())
        .context("Info.plist에 CFBundleExecutable이 없어요.")?;
    let executable = application.join("Contents/MacOS").join(name);
    Ok((application, executable))
}

/// A universal application must actually contain both architectures.
pub fn architectures(executable: &Path, environment: &[(String, String)]) -> Result<String> {
    let arguments = vec!["-archs".to_owned(), executable.to_string_lossy().into_owned()];
    let output = stream::capture("lipo", &arguments, environment)?;
    let architectures = output.trim().to_owned();
    if !architectures.contains("arm64") || !architectures.contains("x86_64") {
        bail!("The macOS universal application is missing a required architecture.");
    }
    Ok(architectures)
}
