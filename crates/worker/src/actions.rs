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
use build_machine_core::workflow::{PhpCoverage, Step};
use build_machine_core::Platform;
use std::path::Path;

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

/// The PHP version, extensions and coverage of a `setup-php` step.
pub fn php_plan(step: &Step, read: &dyn Fn(&str) -> Result<String>) -> Result<(String, Vec<String>)> {
    let plan = php_step(step, read)?;
    Ok((plan.version, plan.extensions))
}

/// The coverage drivers setup-php disables and the one it selects.
pub fn coverage_change(coverage: PhpCoverage) -> (Vec<&'static str>, Option<&'static str>) {
    match coverage {
        PhpCoverage::None => (vec!["xdebug", "pcov"], None),
        PhpCoverage::Xdebug => (vec!["pcov"], Some("xdebug")),
        PhpCoverage::Pcov => (vec!["xdebug"], Some("pcov")),
    }
}

/// Check `php -m` against the coverage: the disabled drivers are not loaded
/// and the selected one is. A failure names each driver that is wrong.
pub fn coverage_check(coverage: PhpCoverage, modules: &str) -> Result<()> {
    let loaded: Vec<String> = modules.lines().map(|line| line.trim().to_ascii_lowercase()).collect();
    let is_loaded = |name: &str| loaded.iter().any(|line| line == name);
    let (disabled, selected) = coverage_change(coverage);
    let still: Vec<&str> = disabled.into_iter().filter(|name| is_loaded(name)).collect();
    if !still.is_empty() {
        bail!("setup-php coverage가 끄는 드라이버가 아직 로드돼 있어요: {}", still.join(", "));
    }
    if let Some(selected) = selected.filter(|name| !is_loaded(name)) {
        bail!("setup-php coverage가 고른 드라이버 {selected}가 로드되지 않았어요.");
    }
    Ok(())
}

/// What a `setup-php` step declares.
pub struct PhpStep {
    pub version: String,
    pub extensions: Vec<String>,
    /// Its `coverage`, when it declares one.
    pub coverage: Option<PhpCoverage>,
}

pub fn php_step(step: &Step, read: &dyn Fn(&str) -> Result<String>) -> Result<PhpStep> {
    let coverage = step.with.get("coverage").map(|value| PhpCoverage::parse(value)).transpose()?;
    if let Some(tools) = step.with.get("tools") {
        if tools.trim() != "composer" {
            bail!("setup-php tools '{tools}'는 아직 지원하지 않아요. composer만 설치해요.");
        }
    }
    let version = declared_version(step, "php-version", "php-version-file", read)?
        .context("setup-php에는 php-version 또는 php-version-file이 필요해요.")?;
    let version = php_version(&version)?;
    let extensions = php_extensions(step.with.get("extensions"))?;
    Ok(PhpStep { version, extensions, coverage })
}

/// The archive of setup-php's cached build of a PHP release, as its
/// `php-ubuntu` install script names it.
pub fn php_build_name(version: &str, ubuntu: &str, arch: &str) -> Result<String> {
    let suffix = match arch {
        "aarch64" | "arm64" => "_arm64",
        "x86_64" => "",
        other => bail!("setup-php에는 {other}용 PHP 빌드가 없어요."),
    };
    Ok(format!("php_{version}-nts+ubuntu{ubuntu}{suffix}.tar.zst"))
}

/// A cached build and the checksum GitHub publishes for it.
#[derive(Debug, PartialEq, Eq)]
pub struct PhpBuild {
    pub url: String,
    pub sha256: String,
}

/// Find a build in the shivammathur/php-ubuntu release setup-php downloads
/// from. A build that is not there is the explicit failure a runner of this
/// image would hit too; it is never replaced by another release.
pub fn php_build(release: &serde_json::Value, name: &str) -> Result<PhpBuild> {
    let tag = release["tag_name"].as_str().unwrap_or_default();
    let asset = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"].as_str() == Some(name)))
        .with_context(|| {
            format!("setup-php의 PHP 빌드 저장소(shivammathur/php-ubuntu {tag})에 {name}이 없어요. 이 Ubuntu 버전과 아키텍처에는 이 PHP 릴리스를 설치할 수 없어요.")
        })?;
    let sha256 = asset["digest"]
        .as_str()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .with_context(|| format!("{name}에 GitHub가 공개한 sha256 체크섬이 없어요."))?;
    let url = asset["browser_download_url"].as_str().context("PHP 빌드의 내려받기 주소가 없어요.")?;
    Ok(PhpBuild { url: url.to_owned(), sha256: sha256.to_owned() })
}

/// What setup-php's php-ubuntu install script does with a build, as root:
/// unpack it over `/`, merge its packages into dpkg's status, and register
/// its programs as alternatives. `$1` is the archive and `$2` the release.
#[cfg(target_os = "linux")]
const PHP_BUILD_INSTALL: &str = r#"set -e
tar_file=$1
version=$2
cp /var/lib/dpkg/status /var/lib/dpkg/status-orig
rm -rf /var/lib/apt/lists/*ondrej*
tar -I zstd -xf "$tar_file" -C /
LC_ALL=C.UTF-8 python3 /usr/sbin/merge_status && rm -f /usr/sbin/merge_status
mv /var/lib/dpkg/status-orig /var/lib/dpkg/status
update-alternatives --force --install /usr/lib/cgi-bin/php php-cgi-bin /usr/lib/cgi-bin/php"$version" "${version/./}"
update-alternatives --force --install /usr/sbin/php-fpm php-fpm /usr/sbin/php-fpm"$version" "${version/./}"
update-alternatives --force --install /run/php/php-fpm.sock php-fpm.sock /run/php/php"$version"-fpm.sock "${version/./}"
for tool in phpize php-config phpdbg php-cgi php phar.phar phar; do
  update-alternatives --force --install /usr/bin/"$tool" "$tool" /usr/bin/"$tool$version" "${version/./}" \
    --slave /usr/share/man/man1/"$tool".1.gz "$tool".1.gz /usr/share/man/man1/"$tool$version".1.gz
done
systemctl daemon-reload 2>/dev/null || true
systemctl start php"$version"-fpm 2>/dev/null || true
if ! apt-get check 2>/dev/null; then
  apt --fix-broken install -y || (apt-get update && apt --fix-broken install -y)
fi
"#;

/// The libraries `ldd` reports as not found.
pub fn missing_libraries(ldd: &str) -> Vec<String> {
    ldd.lines()
        .filter_map(|line| line.trim().strip_suffix("=> not found").map(|name| name.trim().to_owned()))
        .collect()
}

/// The program an alternative currently selects, from `update-alternatives
/// --query`. `None` when it selects nothing.
pub fn alternative_value(query: &str) -> Option<String> {
    query
        .lines()
        .find_map(|line| line.strip_prefix("Value: "))
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
        .map(str::to_owned)
}

/// The programs setup-php switches between releases.
#[cfg(target_os = "linux")]
const PHP_TOOLS: [&str; 7] = ["php", "phar", "phar.phar", "php-cgi", "php-config", "phpize", "phpdbg"];

/// What each PHP program selects now.
#[cfg(target_os = "linux")]
fn php_selection(environment: &[(String, String)]) -> Vec<(&'static str, Option<String>)> {
    PHP_TOOLS
        .iter()
        .map(|tool| {
            let query = stream::capture("update-alternatives", &["--query".to_owned(), (*tool).to_owned()], environment);
            (*tool, query.ok().and_then(|text| alternative_value(&text)))
        })
        .collect()
}

/// Put the selection back as it was before this step: what was selected is
/// selected again, and an alternative this step registered where there was
/// none is removed.
#[cfg(target_os = "linux")]
fn restore_php_selection(previous: &[(&'static str, Option<String>)], version: &str, environment: &[(String, String)]) -> Result<()> {
    for (tool, selected) in previous {
        match selected {
            Some(program) => as_root("update-alternatives", &["--set", tool, program], environment)?,
            None => {
                let program = format!("/usr/bin/{tool}{version}");
                let registered = stream::capture("update-alternatives", &["--list".to_owned(), (*tool).to_owned()], environment)
                    .is_ok_and(|list| list.lines().any(|line| line.trim() == program));
                if registered {
                    as_root("update-alternatives", &["--remove", tool, &program], environment)?;
                }
            }
        }
    }
    Ok(())
}

/// Prove the release runs, with the declared extensions, before it becomes
/// `php`. A failure names the libraries the machine lacks.
#[cfg(target_os = "linux")]
fn verify_php(version: &str, extensions: &[String], environment: &[(String, String)]) -> Result<()> {
    let program = format!("/usr/bin/php{version}");
    let ldd = |path: &str| stream::capture("ldd", &[path.to_owned()], environment).map(|text| missing_libraries(&text)).unwrap_or_default();
    if let Err(error) = stream::capture(&program, &["-v".to_owned()], environment) {
        let missing = ldd(&program);
        if missing.is_empty() {
            bail!("PHP {version}가 실행되지 않아요: {error:#}");
        }
        bail!("PHP {version}가 실행되지 않아요. 이 머신에 없는 라이브러리: {}", missing.join(", "));
    }
    let modules = stream::capture(&program, &["-m".to_owned()], environment)?.to_ascii_lowercase();
    let loaded: Vec<&str> = modules.lines().map(str::trim).collect();
    let absent: Vec<&String> = extensions.iter().filter(|extension| !loaded.contains(&extension.as_str())).collect();
    if absent.is_empty() {
        return Ok(());
    }
    let directory = stream::capture(&format!("/usr/bin/php-config{version}"), &["--extension-dir".to_owned()], environment)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let mut details = Vec::new();
    for extension in &absent {
        let library = format!("{directory}/{extension}.so");
        let missing = if Path::new(&library).exists() { ldd(&library) } else { Vec::new() };
        details.push(if missing.is_empty() {
            format!("{extension} (not in this build; setup-php would install it from ondrej/php or PECL, which this adapter does not do)")
        } else {
            format!("{extension} (missing libraries: {})", missing.join(", "))
        });
    }
    bail!("PHP {version}에 선언한 확장이 없어요: {}", details.join("; "))
}

/// setup-php's coverage on Linux, as root: each disabled driver loses its
/// `conf.d` link in every SAPI of the release and its line in each `php.ini`
/// (`disable_extension_helper`); the selected driver is enabled in every
/// SAPI, and PCOV gets `pcov.enabled=1` as setup-php sets it. `$1` is the
/// release, `$2` the selected driver or empty, the rest the disabled ones.
#[cfg(target_os = "linux")]
const PHP_COVERAGE: &str = r#"set -e
version=$1
selected=$2
shift 2
for extension in "$@"; do
  find /etc/php/"$version" -name "*-$extension.ini" -not -path "*mods-available*" -delete
  for ini in /etc/php/"$version"/*/php.ini; do
    [ -f "$ini" ] && sed -Ei "/=(.*\/)?\"?$extension(.so)?\"?$/d" "$ini"
  done
done
if [ -n "$selected" ]; then
  phpenmod -v "$version" -s ALL "$selected"
  if [ "$selected" = pcov ]; then
    for ini in /etc/php/"$version"/*/php.ini; do
      [ -f "$ini" ] && { grep -q '^pcov.enabled=1' "$ini" || echo 'pcov.enabled=1' >> "$ini"; }
    done
  fi
fi
"#;

/// Run a command as root through `sudo`, which a runner's account may use
/// without a password; a password prompt fails the step instead of waiting.
#[cfg(target_os = "linux")]
fn as_root(program: &str, arguments: &[&str], environment: &[(String, String)]) -> Result<()> {
    let mut all = vec!["-n".to_owned(), "env".to_owned(), "DEBIAN_FRONTEND=noninteractive".to_owned(), program.to_owned()];
    all.extend(arguments.iter().map(|value| (*value).to_owned()));
    stream::checked("sudo", &all, None, environment)
        .with_context(|| "setup-php는 runner처럼 비밀번호 없는 sudo가 필요해요.".to_owned())?;
    Ok(())
}

/// `shivammathur/setup-php` on Ubuntu, as it runs on a hosted runner: a PHP
/// release the machine already has (`/usr/bin/php<v>` and `php-config<v>`) is
/// switched to; any other is installed from setup-php's cached build for this
/// Ubuntu version and architecture. The release then becomes `php` and its
/// tools on PATH, and Composer is installed.
#[cfg(target_os = "linux")]
pub fn setup_php(step: &Step, workspace: &Path, tools: &Tools, runner: &mut Runner) -> Result<Installed> {
    let PhpStep { version, extensions, coverage } = php_step(step, &read_in(workspace))?;
    let environment = runner_environment(tools, runner)?;
    // The install registers alternatives; what was selected before is kept
    // to put back if the release does not run.
    let previous = php_selection(&environment);
    let present = Path::new(&format!("/usr/bin/php{version}")).exists()
        && Path::new(&format!("/usr/bin/php-config{version}")).exists();
    let source = if present {
        format!("PHP {version} already on this machine")
    } else {
        let release_text = std::fs::read_to_string("/etc/os-release").context("/etc/os-release를 읽지 못했어요.")?;
        let ubuntu = release_text
            .lines()
            .find_map(|line| line.strip_prefix("VERSION_ID="))
            .map(|value| value.trim_matches('"').to_owned())
            .context("/etc/os-release에 VERSION_ID가 없어요.")?;
        let name = php_build_name(&version, &ubuntu, std::env::consts::ARCH)?;
        let release = fetch_json(
            tools,
            "https://api.github.com/repos/shivammathur/php-ubuntu/releases/latest",
            "php-ubuntu-release.json",
        )?;
        let build = php_build(&release, &name)?;
        let archive = tools.download(&build.url, &build.sha256, &name)?;
        let archive_text = archive.to_string_lossy().into_owned();
        let installed = as_root("bash", &["-c", PHP_BUILD_INSTALL, "php-build-install", &archive_text, &version], &environment);
        if let Err(error) = installed {
            restore_php_selection(&previous, &version, &environment)?;
            return Err(error);
        }
        format!("{name} of shivammathur/php-ubuntu {}, sha256 {}", release["tag_name"].as_str().unwrap_or_default(), build.sha256)
    };
    if let Err(error) = verify_php(&version, &extensions, &environment) {
        restore_php_selection(&previous, &version, &environment)
            .context("PHP 선택을 이전 상태로 되돌리지 못했어요")?;
        return Err(error.context("이전 PHP 선택은 그대로 두었어요"));
    }
    // setup-php's switch_version: the release's programs become the defaults.
    for tool in PHP_TOOLS {
        let program = format!("/usr/bin/{tool}{version}");
        if Path::new(&program).exists() {
            as_root("update-alternatives", &["--set", tool, &program], &environment)?;
        }
    }
    check_reports(
        tools,
        runner,
        "php",
        &["-r", "echo 'PHP ', PHP_MAJOR_VERSION, '.', PHP_MINOR_VERSION, PHP_EOL;"],
        &format!("PHP {version}"),
    )?;
    let coverage_summary = match coverage {
        Some(coverage) => {
            let (disabled, selected) = coverage_change(coverage);
            let mut arguments = vec!["-c", PHP_COVERAGE, "php-coverage", &version, selected.unwrap_or("")];
            arguments.extend(disabled.iter().copied());
            as_root("bash", &arguments, &environment)?;
            let modules = capture_in(tools, runner, &format!("php{version}"), &["-m"])?;
            coverage_check(coverage, &modules)?;
            format!(
                "; coverage {}: {} disabled{}",
                step.with.get("coverage").map(String::as_str).unwrap_or_default(),
                disabled.join(", "),
                selected.map(|name| format!(", {name} enabled")).unwrap_or_default()
            )
        }
        None => String::new(),
    };
    let composer = install_composer(tools)?;
    runner.add_path(composer.0.clone());
    Ok(Installed {
        summary: format!(
            "PHP {version} from {source}, extensions {}{coverage_summary}; Composer {}.",
            extensions.join(", "),
            composer.1
        ),
        limits: Vec::new(),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn setup_php(_step: &Step, _workspace: &Path, _tools: &Tools, _runner: &mut Runner) -> Result<Installed> {
    bail!("setup-php 어댑터는 아직 Linux 작업자에서만 설치해요.")
}

#[cfg(target_os = "linux")]
/// The latest stable Composer, as `setup-php` installs it, verified against
/// the checksum getcomposer.org publishes for that release.
fn install_composer(tools: &Tools) -> Result<(std::path::PathBuf, String)> {
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

/// The environment the steps after this one run under.
fn runner_environment(tools: &Tools, runner: &Runner) -> Result<Vec<(String, String)>> {
    let files = runner.begin_step(0)?;
    Ok(runner.environment(&tools.environment, &files))
}

fn capture_in(tools: &Tools, runner: &Runner, program: &str, arguments: &[&str]) -> Result<String> {
    // The step's own PATH decides which program runs, as it will for the
    // steps after this one.
    let environment = runner_environment(tools, runner)?;
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
