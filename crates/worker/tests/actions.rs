//! The setup actions install the release the workflow declares.
//!
//! orm's CI names Go 1.27, Node.js from `.node-version` and PHP from
//! `.php-version`. Standing in for those actions with whatever `machine.json`
//! installed would run the checks on releases the workflow did not ask for.

use build_machine_core::workflow::{Adapter, Step};
use build_machine_worker::actions::{
    alternative_value, declared_version, go_mod_version, listed_checksum, missing_libraries, php_build, php_build_name,
    php_extensions, php_plan,
    php_version, resolve_go, resolve_node, version_spec, GoArchive,
};

fn step(adapter: Adapter, with: &[(&str, &str)]) -> Step {
    Step {
        index: 1,
        position: 1,
        name: "setup".to_owned(),
        adapter,
        with: with.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
        job_id: "test".to_owned(),
        ..Step::default()
    }
}

fn files(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> anyhow::Result<String> {
    move |path: &str| {
        entries
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| (*text).to_owned())
            .ok_or_else(|| anyhow::anyhow!("no such file: {path}"))
    }
}

#[test]
fn the_version_comes_from_the_input_or_the_version_file() {
    let read = files(&[(".node-version", "\n26.8.1\n"), ("go.mod", "module x\n\ngo 1.27\n\ntoolchain go1.27.1\n")]);
    let direct = step(Adapter::GoSetup, &[("go-version", "1.27")]);
    assert_eq!(declared_version(&direct, "go-version", "go-version-file", &read).unwrap().as_deref(), Some("1.27"));
    let file = step(Adapter::NodeSetup, &[("node-version-file", ".node-version")]);
    assert_eq!(declared_version(&file, "node-version", "node-version-file", &read).unwrap().as_deref(), Some("26.8.1"));
    // A toolchain directive names the exact release.
    let module = step(Adapter::GoSetup, &[("go-version-file", "go.mod")]);
    assert_eq!(declared_version(&module, "go-version", "go-version-file", &read).unwrap().as_deref(), Some("1.27.1"));
    assert_eq!(go_mod_version("module x\ngo 1.26.2\n").as_deref(), Some("1.26.2"));
    // Nothing declared keeps the machine's own release, and says so.
    assert_eq!(declared_version(&step(Adapter::NodeSetup, &[]), "node-version", "node-version-file", &read).unwrap(), None);
    // Both is ambiguous, and a missing file is an error rather than no version.
    let both = step(Adapter::NodeSetup, &[("node-version", "22"), ("node-version-file", ".node-version")]);
    assert!(declared_version(&both, "node-version", "node-version-file", &read).is_err());
    let missing = step(Adapter::NodeSetup, &[("node-version-file", ".nvmrc")]);
    assert!(declared_version(&missing, "node-version", "node-version-file", &read).is_err());
}

#[test]
fn only_numeric_versions_are_accepted() {
    assert_eq!(version_spec("v26.8.1").unwrap(), [26, 8, 1]);
    assert_eq!(version_spec("1.27").unwrap(), [1, 27]);
    for unsupported in ["lts/*", "latest", "node", "1.27.x", ">=22", "1.2.3.4", ""] {
        assert!(version_spec(unsupported).is_err(), "{unsupported}");
    }
}

#[test]
fn a_node_release_line_resolves_to_its_newest_release() {
    let index = serde_json::json!([
        { "version": "v26.9.0" }, { "version": "v26.8.10" }, { "version": "v26.8.2" }, { "version": "v25.1.0" }
    ]);
    assert_eq!(resolve_node("26", &index).unwrap(), "26.9.0");
    // 26.8.10 is newer than 26.8.2: versions compare as numbers.
    assert_eq!(resolve_node("26.8", &index).unwrap(), "26.8.10");
    assert_eq!(resolve_node("26.8.2", &index).unwrap(), "26.8.2");
    assert!(resolve_node("27", &index).is_err());
}

#[test]
fn a_go_release_line_resolves_to_its_newest_stable_archive_for_this_machine() {
    let file = |version: &str, os: &str, arch: &str, kind: &str| {
        serde_json::json!({ "filename": format!("go{version}.{os}-{arch}.tar.gz"), "os": os, "arch": arch, "kind": kind, "sha256": format!("sum-{version}-{arch}") })
    };
    let releases = serde_json::json!([
        { "version": "go1.28rc1", "stable": false, "files": [file("1.28rc1", "linux", "arm64", "archive")] },
        { "version": "go1.27.10", "stable": true, "files": [file("1.27.10", "linux", "arm64", "archive"), file("1.27.10", "linux", "amd64", "archive"), file("1.27.10", "src", "", "source")] },
        { "version": "go1.27.2", "stable": true, "files": [file("1.27.2", "linux", "arm64", "archive")] },
        { "version": "go1.26.5", "stable": true, "files": [file("1.26.5", "linux", "arm64", "archive")] }
    ]);
    assert_eq!(
        resolve_go("1.27", &releases, "linux", "arm64").unwrap(),
        GoArchive { version: "1.27.10".to_owned(), filename: "go1.27.10.linux-arm64.tar.gz".to_owned(), sha256: "sum-1.27.10-arm64".to_owned() }
    );
    assert_eq!(resolve_go("1.27.2", &releases, "linux", "arm64").unwrap().version, "1.27.2");
    // A release candidate is never what a version line means.
    assert!(resolve_go("1.28", &releases, "linux", "arm64").is_err());
    // No archive for this machine is an error, not another architecture.
    assert!(resolve_go("1.27.2", &releases, "linux", "amd64").is_err());
}

#[test]
fn a_checksum_comes_from_the_publishers_listing() {
    let listing = "aaa  node-v26.8.1-linux-x64.tar.xz\nbbb  node-v26.8.1-linux-arm64.tar.xz\n";
    assert_eq!(listed_checksum(listing, "node-v26.8.1-linux-arm64.tar.xz").unwrap(), "bbb");
    assert!(listed_checksum(listing, "node-v26.8.1-darwin-arm64.tar.gz").is_err());
}

#[test]
fn php_versions_and_extensions_are_read_as_setup_php_reads_them() {
    assert_eq!(php_extensions(Some(&"pdo_mysql, PDO_PGSQL , openssl".to_owned())).unwrap(), ["pdo_mysql", "pdo_pgsql", "openssl"]);
    assert_eq!(php_version("8.5").unwrap(), "8.5");
    for unsupported in ["8", "8.5.1", "latest"] {
        assert!(php_version(unsupported).is_err(), "{unsupported}");
    }
    // Disabling extensions changes the image instead of adding to it.
    for unsupported in [":xdebug", "none", "redis-6.0.2"] {
        assert!(php_extensions(Some(&unsupported.to_owned())).is_err(), "{unsupported}");
    }
}

/// On a hosted Ubuntu runner setup-php installs a PHP release the image does
/// not carry from its cached builds, shivammathur/php-ubuntu, one archive per
/// release, Ubuntu version and architecture. The distribution's archive has
/// one PHP release per Ubuntu version: Ubuntu 26.04 carries 8.5, not the 8.4
/// orm's lowest-release check asks for.
#[test]
fn php_comes_from_setup_php_s_build_for_this_ubuntu_and_architecture() {
    assert_eq!(php_build_name("8.4", "26.04", "aarch64").unwrap(), "php_8.4-nts+ubuntu26.04_arm64.tar.zst");
    assert_eq!(php_build_name("8.5", "24.04", "x86_64").unwrap(), "php_8.5-nts+ubuntu24.04.tar.zst");
    assert!(php_build_name("8.5", "26.04", "riscv64").is_err());

    let release = serde_json::json!({
        "tag_name": "builds",
        "assets": [
            { "name": "php_8.4-nts+ubuntu26.04_arm64.tar.zst", "digest": "sha256:6c27", "browser_download_url": "https://github.com/shivammathur/php-ubuntu/releases/download/builds/php_8.4-nts%2Bubuntu26.04_arm64.tar.zst" },
            { "name": "php_8.3-nts+ubuntu26.04_arm64.tar.zst", "browser_download_url": "https://example.invalid/unsigned" }
        ]
    });
    let build = php_build(&release, "php_8.4-nts+ubuntu26.04_arm64.tar.zst").unwrap();
    assert_eq!(build.sha256, "6c27");
    assert!(build.url.ends_with("php_8.4-nts%2Bubuntu26.04_arm64.tar.zst"));
    // No build is an explicit failure naming what is missing, never another release.
    let missing = format!("{:#}", php_build(&release, "php_7.4-nts+ubuntu26.04_arm64.tar.zst").unwrap_err());
    assert!(missing.contains("php_7.4-nts+ubuntu26.04_arm64.tar.zst"), "{missing}");
    // A build without a published checksum is not installed.
    assert!(php_build(&release, "php_8.3-nts+ubuntu26.04_arm64.tar.zst").is_err());
}

#[test]
fn the_php_plan_reads_the_version_file_and_refuses_what_it_does_not_install() {
    let read = files(&[(".php-version", "8.5\n")]);
    let orm = step(Adapter::PhpSetup, &[("php-version-file", ".php-version"), ("extensions", "pdo_mysql, pdo_pgsql, pdo_sqlite, openssl")]);
    let (version, extensions) = php_plan(&orm, &read).unwrap();
    assert_eq!(version, "8.5");
    assert_eq!(extensions, ["pdo_mysql", "pdo_pgsql", "pdo_sqlite", "openssl"]);
    assert!(php_plan(&step(Adapter::PhpSetup, &[]), &read).is_err(), "a version is required");
    let tools = step(Adapter::PhpSetup, &[("php-version", "8.5"), ("tools", "phpunit")]);
    assert!(php_plan(&tools, &read).is_err());
    let coverage = step(Adapter::PhpSetup, &[("php-version", "8.5"), ("coverage", "phpdbg")]);
    assert!(php_plan(&coverage, &read).is_err());
}

/// `coverage: none` disables Xdebug and PCOV; `xdebug` and `pcov` select one
/// and disable the other, as setup-php does. The release's `php -m` after the
/// step proves it, naming what is still loaded.
#[test]
fn coverage_disables_and_selects_the_drivers_as_setup_php_does() {
    use build_machine_core::workflow::PhpCoverage;
    use build_machine_worker::actions::{coverage_change, coverage_check};
    assert_eq!(coverage_change(PhpCoverage::None), (vec!["xdebug", "pcov"], None));
    assert_eq!(coverage_change(PhpCoverage::Xdebug), (vec!["pcov"], Some("xdebug")));
    assert_eq!(coverage_change(PhpCoverage::Pcov), (vec!["xdebug"], Some("pcov")));
    let loaded = "[PHP Modules]\nCore\npcov\nxdebug\n\n[Zend Modules]\nXdebug\n";
    let error = coverage_check(PhpCoverage::None, loaded).unwrap_err().to_string();
    assert!(error.contains("xdebug") && error.contains("pcov"), "{error}");
    coverage_check(PhpCoverage::None, "[PHP Modules]\nCore\n\n[Zend Modules]\n").unwrap();
    coverage_check(PhpCoverage::Pcov, "[PHP Modules]\nCore\npcov\n").unwrap();
    assert!(coverage_check(PhpCoverage::Pcov, "[PHP Modules]\nCore\n").unwrap_err().to_string().contains("pcov"));
}

/// A PHP that cannot start is named by the libraries it lacks, as `ldd`
/// reports them, so the failed step says what the machine is missing.
#[test]
fn missing_libraries_are_read_from_ldd() {
    let ldd = "\tlinux-vdso.so.1 (0x0000ffff)\n\tlibargon2.so.1 => not found\n\tlibsodium.so.23 => not found\n\tlibc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0000ffff)\n";
    assert_eq!(missing_libraries(ldd), ["libargon2.so.1", "libsodium.so.23"]);
    assert!(missing_libraries("\tlibc.so.6 => /lib/libc.so.6 (0x1)\n").is_empty());
}

/// The selection setup-php changes is read first, so a PHP that fails its
/// check leaves the machine with the selection it had.
#[test]
fn the_current_alternative_is_read_from_its_query() {
    let query = "Name: php\nLink: /usr/bin/php\nStatus: manual\nBest: /usr/bin/php8.5\nValue: /usr/bin/php8.5\n\nAlternative: /usr/bin/php8.5\nPriority: 85\n";
    assert_eq!(alternative_value(query).as_deref(), Some("/usr/bin/php8.5"));
    assert_eq!(alternative_value("Name: php\nValue: none\n"), None);
    assert_eq!(alternative_value(""), None);
}
