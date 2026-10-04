//! Setup actions that install what the workflow declares.
//!
//! `actions/setup-node`, `actions/setup-go` and `shivammathur/setup-php` exist
//! to select a version. Standing in for one with whatever `machine.json`
//! installed would run the workflow on a release it did not ask for, so each
//! adapter reads the version the workflow names — directly or through a
//! version file — installs exactly that release, and puts it first on PATH for
//! the steps after it.
//!
//! Installation follows the machine's own provisioning: the archive is
//! verified against the checksum its publisher lists before it is used, each
//! release has its own directory under the managed root, and a release that is
//! already there is not installed again.

use crate::provision::Tools;
use crate::runner::Runner;
use crate::stream;
use anyhow::{bail, Context, Result};
use build_machine_core::workflow::Step;
use build_machine_core::Platform;
use std::path::{Path, PathBuf};

/// What a setup adapter did, for the step's report.
pub struct Installed {
    pub summary: String,
    pub limits: Vec<String>,
}

/// The version a step declares, through its version input or its version file.
///
/// Naming both is ambiguous, so it is refused rather than one silently winning.
pub fn declared_version(
    step: &Step,
    version_input: &str,
    file_input: &str,
    read: &dyn Fn(&str) -> Result<String>,
) -> Result<Option<String>> {
    let version = step.with.get(version_input).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    let file = step.with.get(file_input).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    match (version, file) {
        (Some(_), Some(_)) => bail!("{version_input}와 {file_input}를 함께 쓸 수 없어요."),
        (Some(version), None) => Ok(Some(version)),
        (None, Some(file)) => {
            let text = read(&file).with_context(|| format!("{file_input} {file}를 읽지 못했어요."))?;
            let version = if file.ends_with("go.mod") || file.ends_with("go.work") {
                go_mod_version(&text).with_context(|| format!("{file}에 go 또는 toolchain 지시어가 없어요."))?
            } else {
                text.lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with('#'))
                    .with_context(|| format!("{file}에 버전이 없어요."))?
                    .to_owned()
            };
            Ok(Some(version))
        }
        (None, None) => Ok(None),
    }
}

/// A `toolchain` directive names the exact release; otherwise the `go`
/// directive names the language version, as `actions/setup-go` reads it.
pub fn go_mod_version(text: &str) -> Option<String> {
    let directive = |name: &str| {
        text.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some(name)).then(|| words.next()).flatten().map(str::to_owned)
        })
    };
    directive("toolchain").map(|value| value.trim_start_matches("go").to_owned()).or_else(|| directive("go"))
}

/// Numeric parts of a dotted version, or `None` for anything else.
fn numeric(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|part| part.parse().ok()).collect()
}

/// A version spec of one to three numeric parts: `26`, `26.8` or `26.8.1`.
pub fn version_spec(text: &str) -> Result<Vec<u64>> {
    let trimmed = text.trim().trim_start_matches('v');
    match numeric(trimmed) {
        Some(parts) if (1..=3).contains(&parts.len()) => Ok(parts),
        _ => bail!("지원하지 않는 버전 지정이에요: {text}. 숫자로 된 버전(예: 22, 22.4, 22.4.1)만 받아요."),
    }
}

/// Whether a release matches a spec: an exact release, or the newest of a line.
fn matches(spec: &[u64], release: &[u64]) -> bool {
    release.len() >= spec.len() && release[..spec.len()] == *spec
}

/// The newest Node.js release that matches a spec, from nodejs.org's index.
pub fn resolve_node(spec: &str, index: &serde_json::Value) -> Result<String> {
    let wanted = version_spec(spec)?;
    let releases = index.as_array().context("Node.js 릴리스 목록 형식이 올바르지 않아요.")?;
    releases
        .iter()
        .filter_map(|release| release["version"].as_str())
        .filter_map(|version| {
            let bare = version.trim_start_matches('v');
            numeric(bare).filter(|parts| matches(&wanted, parts)).map(|parts| (parts, bare.to_owned()))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, version)| version)
        .with_context(|| format!("Node.js {spec}에 맞는 릴리스가 없어요."))
}

/// A Go release archive: its version, file name and published checksum.
#[derive(Debug, PartialEq, Eq)]
pub struct GoArchive {
    pub version: String,
    pub filename: String,
    pub sha256: String,
}

/// The newest stable Go release matching a spec, from go.dev's release list.
pub fn resolve_go(spec: &str, releases: &serde_json::Value, os: &str, arch: &str) -> Result<GoArchive> {
    let wanted = version_spec(spec)?;
    let releases = releases.as_array().context("Go 릴리스 목록 형식이 올바르지 않아요.")?;
    let (_, release) = releases
        .iter()
        .filter(|release| release["stable"].as_bool() == Some(true))
        .filter_map(|release| {
            let version = release["version"].as_str()?.strip_prefix("go")?;
            numeric(version).filter(|parts| matches(&wanted, parts)).map(|parts| (parts, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .with_context(|| format!("Go {spec}에 맞는 안정 릴리스가 없어요."))?;
    let version = release["version"].as_str().unwrap_or_default().trim_start_matches("go").to_owned();
    let file = release["files"]
        .as_array()
        .and_then(|files| {
            files.iter().find(|file| {
                file["os"].as_str() == Some(os)
                    && file["arch"].as_str() == Some(arch)
                    && file["kind"].as_str() == Some("archive")
            })
        })
        .with_context(|| format!("Go {version}에 {os}/{arch} 아카이브가 없어요."))?;
    Ok(GoArchive {
        version,
        filename: file["filename"].as_str().context("Go 아카이브 이름이 없어요.")?.to_owned(),
        sha256: file["sha256"].as_str().context("Go 아카이브 체크섬이 없어요.")?.to_owned(),
    })
}

/// A file's checksum from a `SHASUMS256.txt` listing.
pub fn listed_checksum(listing: &str, filename: &str) -> Result<String> {
    listing
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            let sum = words.next()?;
            (words.next()? == filename).then(|| sum.to_owned())
        })
        .with_context(|| format!("체크섬 목록에 {filename}이 없어요."))
}

/// A PHP version as `setup-php` takes it: `major.minor`.
pub fn php_version(text: &str) -> Result<String> {
    let trimmed = text.trim();
    match numeric(trimmed) {
        Some(parts) if parts.len() == 2 => Ok(trimmed.to_owned()),
        _ => bail!("PHP 버전은 major.minor 형식이어야 해요: {text}"),
    }
}

/// Extensions every Debian PHP build carries in `php<v>-cli` or `php<v>-common`.
const PHP_BUNDLED: [&str; 27] = [
    "calendar", "core", "ctype", "date", "exif", "ffi", "fileinfo", "filter", "ftp", "gettext", "hash", "iconv",
    "json", "libxml", "openssl", "pcntl", "pcre", "pdo", "phar", "posix", "random", "readline", "reflection",
    "sockets", "sodium", "spl", "tokenizer",
];

/// The declared extensions, normalised. Disabling one (`:name`) or all
/// (`none`) changes the image rather than adding to it, which this adapter
/// does not do.
pub fn php_extensions(list: Option<&String>) -> Result<Vec<String>> {
    let mut extensions = Vec::new();
    for name in list.map(String::as_str).unwrap_or_default().split(',') {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if name.starts_with(':') || name == "none" || name.contains(['@', '-', '/']) {
            bail!("지원하지 않는 PHP 확장 지정이에요: {name}");
        }
        extensions.push(name);
    }
    Ok(extensions)
}

/// The Debian packages that provide a PHP version and its extensions.
pub fn php_packages(version: &str, extensions: &[String]) -> Vec<String> {
    let mut packages = vec![format!("php{version}-cli")];
    for extension in extensions {
        if PHP_BUNDLED.contains(&extension.as_str()) {
            continue;
        }
        let package = match extension.as_str() {
            "pdo_mysql" | "mysqli" | "mysqlnd" => "mysql",
            "pdo_pgsql" | "pgsql" => "pgsql",
            "pdo_sqlite" | "sqlite3" => "sqlite3",
            other => other,
        };
        let package = format!("php{version}-{package}");
        if !packages.contains(&package) {
            packages.push(package);
        }
    }
    packages
}

fn read_in(workspace: &Path) -> impl Fn(&str) -> Result<String> + '_ {
    move |relative: &str| {
        let path = crate::build::contained_in(workspace, &workspace.join(relative))
            .context("버전 파일은 저장소 안에 있어야 해요.")?;
        Ok(std::fs::read_to_string(path)?)
    }
}

/// Fetch a JSON index the publisher serves, every time: it names the newest
/// release, which is the answer a version line asks for.
fn fetch_json(tools: &Tools, url: &str, name: &str) -> Result<serde_json::Value> {
    let destination = tools.root.join("downloads").join(name);
    crate::provision::fetch(url, &destination)?;
    let text = std::fs::read_to_string(&destination)?;
    serde_json::from_str(&text).with_context(|| format!("{url}의 응답을 읽지 못했어요."))
}

fn fetch_text(tools: &Tools, url: &str, name: &str) -> Result<String> {
    let destination = tools.root.join("downloads").join(name);
    crate::provision::fetch(url, &destination)?;
    Ok(std::fs::read_to_string(&destination)?)
}

fn archive_platform(tools: &Tools) -> Result<(&'static str, &'static str)> {
    let os = match tools.platform {
        Platform::Linux => "linux",
        Platform::Macos => "darwin",
        Platform::Windows => bail!("이 setup 어댑터는 아직 Windows 작업자에서 설치하지 못해요."),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => bail!("지원하지 않는 CPU 아키텍처예요: {other}"),
    };
    Ok((os, arch))
}

/// Unpack a verified archive into its own directory under the managed root.
fn install_archive(tools: &Tools, archive: &Path, name: &str, destination: &Path, inner: Option<&str>) -> Result<()> {
    #[cfg(unix)]
    {
        let staging = crate::provision::unix::staging_for(tools, name)?;
        crate::provision::unix::extract_tar(archive, &staging)?;
        let staged = match inner {
            Some(inner) => staging.join(inner),
            None => staging.clone(),
        };
        tools.install_extracted(&staged, destination)?;
        std::fs::remove_dir_all(&staging).ok();
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (tools, archive, name, destination, inner);
        bail!("이 setup 어댑터는 아직 Windows 작업자에서 설치하지 못해요.")
    }
}

fn cache_limit(step: &Step, action: &str) -> Vec<String> {
    match step.with.get("cache").map(String::as_str) {
        None | Some("false") => Vec::new(),
        Some(cache) => vec![format!("{action} cache '{cache}' is not restored or saved by the local runner.")],
    }
}

/// `actions/setup-node`.
pub fn setup_node(step: &Step, workspace: &Path, tools: &Tools, runner: &mut Runner) -> Result<Installed> {
    let limits = cache_limit(step, "setup-node");
    let Some(spec) = declared_version(step, "node-version", "node-version-file", &read_in(workspace))? else {
        return Ok(Installed {
            summary: format!("No version declared; Node.js {} of machine.json stays on PATH.", tools.machine.node.version),
            limits,
        });
    };
    let (os, arch) = archive_platform(tools)?;
    let parts = version_spec(&spec)?;
    let version = if parts.len() == 3 {
        parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
    } else {
        resolve_node(&spec, &fetch_json(tools, "https://nodejs.org/dist/index.json", "node-index.json")?)?
    };
    let name = format!("node-v{version}-{os}-{arch}");
    let directory = tools.root.join(&name);
    let summary = if directory.join("bin").join("node").exists() {
        format!("Node.js {version} (declared {spec}) already installed at {}.", directory.display())
    } else {
        let extension = if os == "darwin" { "tar.gz" } else { "tar.xz" };
        let filename = format!("{name}.{extension}");
        let listing = fetch_text(
            tools,
            &format!("https://nodejs.org/dist/v{version}/SHASUMS256.txt"),
            &format!("node-v{version}-SHASUMS256.txt"),
        )?;
        let sha256 = listed_checksum(&listing, &filename)?;
        let archive = tools.download(&format!("https://nodejs.org/dist/v{version}/{filename}"), &sha256, &filename)?;
        install_archive(tools, &archive, &name, &directory, Some(&name))?;
        format!("Installed Node.js {version} (declared {spec}), {filename} sha256 {sha256}.")
    };
    runner.add_path(directory.join("bin"));
    check_reports(tools, runner, "node", &["--version"], &format!("v{version}"))?;
    Ok(Installed { summary, limits })
}

/// `actions/setup-go`.
pub fn setup_go(step: &Step, workspace: &Path, tools: &Tools, runner: &mut Runner) -> Result<Installed> {
    let mut limits = cache_limit(step, "setup-go");
    // setup-go caches modules unless told otherwise.
    if !step.with.contains_key("cache") {
        limits.push("setup-go module cache is not restored or saved by the local runner.".to_owned());
    }
    let Some(spec) = declared_version(step, "go-version", "go-version-file", &read_in(workspace))? else {
        return Ok(Installed {
            summary: format!("No version declared; Go {} of machine.json stays on PATH.", tools.machine.go.version),
            limits,
        });
    };
    let (os, arch) = archive_platform(tools)?;
    let arch = if arch == "x64" { "amd64" } else { arch };
    let installed = |version: &str| tools.root.join(format!("go-{version}"));
    let parts = version_spec(&spec)?;
    // An exact release already installed needs no index; a release line always
    // asks go.dev which release is newest.
    let exact = (parts.len() == 3).then(|| spec.trim().to_owned());
    let (version, summary) = match exact.filter(|version| installed(version).join("go/bin/go").exists()) {
        Some(version) => {
            let summary = format!("Go {version} (declared {spec}) already installed.");
            (version, summary)
        }
        None => {
            let releases = fetch_json(tools, "https://go.dev/dl/?mode=json&include=all", "go-releases.json")?;
            let archive = resolve_go(&spec, &releases, os, arch)?;
            let directory = installed(&archive.version);
            let summary = if directory.join("go/bin/go").exists() {
                format!("Go {} (declared {spec}) already installed at {}.", archive.version, directory.display())
            } else {
                let downloaded = tools.download(
                    &format!("https://go.dev/dl/{}", archive.filename),
                    &archive.sha256,
                    &archive.filename,
                )?;
                install_archive(tools, &downloaded, &format!("go-{}", archive.version), &directory, None)?;
                format!("Installed Go {} (declared {spec}), {} sha256 {}.", archive.version, archive.filename, archive.sha256)
            };
            (archive.version, summary)
        }
    };
    runner.add_path(installed(&version).join("go").join("bin"));
    check_reports(tools, runner, "go", &["version"], &format!("go version go{version} "))?;
    Ok(Installed { summary, limits })
}

/// The PHP version and packages of a `setup-php` step, read from the source
/// archive. The system phase runs before the source is extracted, as root, and
/// must not leave root-owned files in the user's workspace.
pub fn php_plan(step: &Step, read: &dyn Fn(&str) -> Result<String>) -> Result<(String, Vec<String>, Vec<String>)> {
    if let Some(tools) = step.with.get("tools") {
        if tools.trim() != "composer" {
            bail!("setup-php tools '{tools}'는 아직 지원하지 않아요. composer만 설치해요.");
        }
    }
    if let Some(coverage) = step.with.get("coverage") {
        if coverage.trim() != "none" {
            bail!("setup-php coverage '{coverage}'는 아직 지원하지 않아요.");
        }
    }
    let version = declared_version(step, "php-version", "php-version-file", read)?
        .context("setup-php에는 php-version 또는 php-version-file이 필요해요.")?;
    let version = php_version(&version)?;
    let extensions = php_extensions(step.with.get("extensions"))?;
    let packages = php_packages(&version, &extensions);
    Ok((version, extensions, packages))
}

/// Install a `setup-php` step's packages. Runs as root, before the replay.
#[cfg(target_os = "linux")]
pub fn setup_php_system(step: &Step, read: &dyn Fn(&str) -> Result<String>, tools: &Tools) -> Result<()> {
    let (version, _, packages) = php_plan(step, read)?;
    let missing: Vec<String> = packages
        .iter()
        .filter(|package| {
            let arguments = vec!["-W".to_owned(), "-f=${Status}".to_owned(), (*package).clone()];
            !matches!(stream::capture("dpkg-query", &arguments, &tools.environment), Ok(status) if status.trim() == "install ok installed")
        })
        .cloned()
        .collect();
    let mut environment = tools.environment.clone();
    environment.push(("DEBIAN_FRONTEND".to_owned(), "noninteractive".to_owned()));
    if missing.is_empty() {
        println!("OK: PHP {version} packages present: {}. No installation.", packages.join(" "));
    } else {
        stream::checked("apt-get", &["update".to_owned()], None, &environment)?;
        let mut arguments = vec!["install".to_owned(), "-y".to_owned()];
        arguments.extend(missing);
        stream::checked("apt-get", &arguments, None, &environment)?;
    }
    // setup-php makes the version it installed the `php` on PATH.
    let program = format!("/usr/bin/php{version}");
    let current = stream::capture(
        "update-alternatives",
        &["--query".to_owned(), "php".to_owned()],
        &environment,
    )?;
    let selected = current.lines().find_map(|line| line.strip_prefix("Value: ")).map(str::trim);
    if selected == Some(program.as_str()) {
        println!("OK: php selects {program}. No change.");
    } else {
        stream::checked("update-alternatives", &["--set".to_owned(), "php".to_owned(), program], None, &environment)?;
    }
    Ok(())
}

/// `shivammathur/setup-php`, as the replaying user: confirm what the system
/// phase installed, and install Composer.
pub fn setup_php(step: &Step, workspace: &Path, tools: &Tools, runner: &mut Runner) -> Result<Installed> {
    if tools.platform != Platform::Linux {
        bail!("setup-php 어댑터는 아직 Linux 작업자에서만 설치해요.");
    }
    let (version, extensions, packages) = php_plan(step, &read_in(workspace))?;
    check_reports(
        tools,
        runner,
        "php",
        &["-r", "echo 'PHP ', PHP_MAJOR_VERSION, '.', PHP_MINOR_VERSION, PHP_EOL;"],
        &format!("PHP {version}"),
    )?;
    let modules = capture_in(tools, runner, "php", &["-m"])?.to_ascii_lowercase();
    let loaded: Vec<&str> = modules.lines().map(str::trim).collect();
    let absent: Vec<&String> = extensions.iter().filter(|extension| !loaded.contains(&extension.as_str())).collect();
    if !absent.is_empty() {
        bail!(
            "PHP {version}에 선언한 확장이 없어요: {}",
            absent.iter().map(|value| value.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    let composer = install_composer(tools)?;
    runner.add_path(composer.0.clone());
    Ok(Installed {
        summary: format!(
            "PHP {version} from {} with extensions {}; Composer {}.",
            packages.join(" "),
            extensions.join(", "),
            composer.1
        ),
        limits: Vec::new(),
    })
}

/// The latest stable Composer, as `setup-php` installs it, verified against
/// the checksum getcomposer.org publishes for that release.
fn install_composer(tools: &Tools) -> Result<(PathBuf, String)> {
    let versions = fetch_json(tools, "https://getcomposer.org/versions", "composer-versions.json")?;
    let version = versions["stable"][0]["version"].as_str().context("Composer 안정 릴리스를 찾지 못했어요.")?.to_owned();
    let directory = tools.root.join(format!("composer-{version}"));
    let program = directory.join("composer");
    if program.exists() {
        return Ok((directory, format!("{version} (already installed)")));
    }
    let listing = fetch_text(
        tools,
        &format!("https://getcomposer.org/download/{version}/composer.phar.sha256sum"),
        &format!("composer-{version}.sha256sum"),
    )?;
    let sha256 = listed_checksum(&listing, "composer.phar")?;
    let phar = tools.download(
        &format!("https://getcomposer.org/download/{version}/composer.phar"),
        &sha256,
        &format!("composer-{version}.phar"),
    )?;
    std::fs::create_dir_all(&directory)?;
    std::fs::copy(&phar, &program)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok((directory, format!("{version}, composer.phar sha256 {sha256}")))
}

fn capture_in(tools: &Tools, runner: &Runner, program: &str, arguments: &[&str]) -> Result<String> {
    // The step's own PATH decides which program runs, as it will for the
    // steps after this one.
    let files = runner.begin_step(0)?;
    let environment = runner.environment(&tools.environment, &files);
    let arguments: Vec<String> = arguments.iter().map(|value| (*value).to_owned()).collect();
    stream::capture(program, &arguments, &environment)
}

/// Prove the tool the next step will find is the one that was declared.
fn check_reports(
    tools: &Tools,
    runner: &Runner,
    program: &str,
    arguments: &[&str],
    expected: &str,
) -> Result<()> {
    let output = capture_in(tools, runner, program, arguments)?;
    let first = output.lines().next().unwrap_or_default().trim();
    // A prefix that ends in a space stops at the version; anything else must
    // match whole, so 26.8.1 is not satisfied by 26.8.10.
    let reported = if expected.ends_with(' ') { first.starts_with(expected) } else { first == expected };
    if !reported {
        bail!("{program}가 {first}를 보고했어요. 선언한 버전은 {}예요.", expected.trim());
    }
    Ok(())
}
