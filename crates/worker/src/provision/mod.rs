//! Checking and installing the declared build prerequisites.
//!
//! Installation is repeatable: a tool that already satisfies `machine.json` is
//! never reinstalled and never added to PATH twice. This is repeatable
//! provisioning, not a claim of bit-for-bit reproducible compiler output.

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

use anyhow::{bail, Context, Result};
use build_machine_core::config::{Machine, Profile};
use build_machine_core::Platform;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub platform: Platform,
    pub os_version: Option<String>,
    pub architecture: String,
    pub tools: BTreeMap<String, Option<String>>,
    pub missing: Vec<String>,
    pub issues: Vec<String>,
    pub ready: bool,
}

/// The managed toolchain for this machine.
pub struct Tools {
    pub machine: Machine,
    pub platform: Platform,
    pub root: PathBuf,
    pub environment: Vec<(String, String)>,
}

impl Tools {
    pub fn new(machine: Machine) -> Result<Tools> {
        let platform = Platform::host()?;
        let home = home_directory()?;
        let root = managed_root(&home, platform);
        let mut tools = Tools { machine, platform, root, environment: Vec::new() };
        tools.environment = tools.build_environment(&home)?;
        Ok(tools)
    }

    pub fn profile(&self) -> Result<&Profile> {
        self.machine.profile(self.platform)
    }

    /// The PATH a build runs under: managed directories first, then the
    /// system's own, so a declared version wins over whatever else is present.
    fn build_environment(&self, home: &Path) -> Result<Vec<(String, String)>> {
        let mut environment: Vec<(String, String)> = std::env::vars().collect();
        let managed = self.managed_paths(home);
        let separator = if cfg!(windows) { ";" } else { ":" };
        let inherited = std::env::var("PATH").unwrap_or_default();
        let joined = managed
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(separator);
        let path = if inherited.is_empty() { joined } else { format!("{joined}{separator}{inherited}") };
        environment.retain(|(key, _)| !key.eq_ignore_ascii_case("PATH"));
        environment.push(("PATH".to_owned(), path));
        environment.push(("RUSTUP_TOOLCHAIN".to_owned(), self.toolchain()));
        environment.push(("CI".to_owned(), "true".to_owned()));
        environment.push(("NO_COLOR".to_owned(), "1".to_owned()));
        Ok(environment)
    }

    pub fn toolchain(&self) -> String {
        if self.platform == Platform::Windows {
            format!("{}-{}", self.machine.rust.version, self.machine.rust.host)
        } else {
            self.machine.rust.version.clone()
        }
    }

    pub fn node_directory(&self) -> PathBuf {
        let version = &self.machine.node.version;
        let suffix = match self.platform {
            Platform::Macos => "darwin-arm64",
            Platform::Linux => "linux-arm64",
            Platform::Windows => "win-arm64",
        };
        self.root.join(format!("node-v{version}-{suffix}"))
    }

    pub fn pnpm_directory(&self) -> PathBuf {
        self.root.join(format!("pnpm-{}", self.machine.pnpm.version))
    }

    pub fn go_directory(&self) -> PathBuf {
        self.root.join(format!("go-{}", self.machine.go.version))
    }

    pub fn git_directory(&self) -> PathBuf {
        self.root.join(format!("git-{}", self.machine.git.version))
    }

    pub fn cargo_bin(&self) -> PathBuf {
        home_directory().map(|home| home.join(".cargo/bin")).unwrap_or_default()
    }

    fn managed_paths(&self, home: &Path) -> Vec<PathBuf> {
        let mut paths = vec![
            if self.platform == Platform::Windows { self.node_directory() } else { self.node_directory().join("bin") },
            self.pnpm_directory().join("bin"),
            self.go_directory().join("go/bin"),
            self.cargo_bin(),
            home.join("go/bin"),
        ];
        if self.platform == Platform::Windows {
            paths[1] = self.pnpm_directory();
            paths.push(self.git_directory().join("cmd"));
        }
        paths
    }

    /// Download a file and prove it is the declared one before using it.
    pub fn download(&self, url: &str, expected: &str, name: &str) -> Result<PathBuf> {
        let directory = self.root.join("downloads");
        std::fs::create_dir_all(&directory)?;
        let destination = directory.join(name);
        if destination.exists()
            && build_machine_core::source::sha256_file(&destination)?.eq_ignore_ascii_case(expected)
        {
            return Ok(destination);
        }
        let partial = destination.with_extension("partial");
        fetch(url, &partial)?;
        let actual = build_machine_core::source::sha256_file(&partial)?;
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("Checksum mismatch: {url}");
        }
        std::fs::rename(&partial, &destination)?;
        Ok(destination)
    }

    /// Move an extracted tree into place. A leftover staging directory is a
    /// sign of an interrupted install and is left for a person to inspect.
    pub fn install_extracted(&self, staged: &Path, destination: &Path) -> Result<()> {
        if destination.exists() {
            bail!("Incomplete managed installation at {}; inspect before replacement.", destination.display());
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(staged, destination)
            .with_context(|| format!("설치 위치로 옮기지 못했어요: {}", destination.display()))?;
        Ok(())
    }

    pub fn doctor(&self) -> Result<Diagnosis> {
        #[cfg(unix)]
        {
            unix::doctor(self)
        }
        #[cfg(windows)]
        {
            windows::doctor(self)
        }
    }

    pub fn setup_system(&self) -> Result<()> {
        #[cfg(unix)]
        {
            unix::setup_system(self)
        }
        #[cfg(windows)]
        {
            windows::setup_machine(self)
        }
    }

    pub fn setup_user(&self) -> Result<()> {
        #[cfg(unix)]
        {
            unix::setup_user(self)
        }
        #[cfg(windows)]
        {
            windows::setup_user(self)
        }
    }
}

/// Download a file with the platform's own client.
///
/// Every machine this runs on already has `curl`: macOS and Windows ship it,
/// and `machine.json` declares it among the Linux packages `setup-system`
/// installs before any download happens. Carrying a TLS stack in the worker
/// would add a C toolchain dependency for no behaviour the system client is
/// missing.
pub fn fetch(url: &str, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    println!("Downloading {url}");
    let arguments = vec![
        "--fail".to_owned(),
        "--location".to_owned(),
        "--silent".to_owned(),
        "--show-error".to_owned(),
        "--output".to_owned(),
        destination.to_string_lossy().into_owned(),
        url.to_owned(),
    ];
    crate::stream::checked("curl", &arguments, None, &[])
        .with_context(|| format!("내려받지 못했어요: {url}"))?;
    Ok(())
}

pub fn home_directory() -> Result<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key).map(PathBuf::from).context("홈 디렉터리를 찾지 못했어요.")
}

fn managed_root(home: &Path, platform: Platform) -> PathBuf {
    match platform {
        Platform::Windows => home.join("AppData/Local/WindowsBuildMachine"),
        _ => home.join(".local/share/build-machine"),
    }
}

/// Where build receipts and running records live for this machine.
pub fn workspace() -> Result<PathBuf> {
    let home = home_directory()?;
    Ok(match Platform::host()? {
        Platform::Windows => PathBuf::from("C:\\BuildMachine\\projects"),
        _ => home.join(".local/state/build-machine"),
    })
}

/// Compare a reported version against what `machine.json` declares.
pub fn check_version(
    tools: &Tools,
    diagnosis: &mut Diagnosis,
    name: &str,
    program: &str,
    arguments: &[&str],
    expected: &str,
) {
    let arguments: Vec<String> = arguments.iter().map(|value| (*value).to_owned()).collect();
    match crate::stream::capture(program, &arguments, &tools.environment) {
        Ok(output) => {
            let found = output.split('\n').next().unwrap_or_default().trim().to_owned();
            diagnosis.tools.insert(name.to_owned(), Some(found.clone()));
            if !found.starts_with(expected) {
                diagnosis.missing.push(name.to_owned());
                diagnosis.issues.push(format!("{name}: expected {}; found {found}", expected.trim()));
            }
        }
        Err(_) => {
            diagnosis.tools.insert(name.to_owned(), None);
            diagnosis.missing.push(name.to_owned());
            diagnosis.issues.push(format!("{name}: expected {}; executable not found", expected.trim()));
        }
    }
}

/// The declared version table both platforms check.
pub fn expected_versions(machine: &Machine) -> Vec<(&'static str, &'static str, Vec<&'static str>, String)> {
    vec![
        ("node", "node", vec!["--version"], format!("v{}", machine.node.version)),
        ("pnpm", "pnpm", vec!["--version"], machine.pnpm.version.clone()),
        ("rust", "rustc", vec!["--version"], format!("rustc {} ", machine.rust.version)),
        ("go", "go", vec!["version"], format!("go version go{} ", machine.go.version)),
        ("git", "git", vec!["--version"], "git version ".to_owned()),
    ]
}
