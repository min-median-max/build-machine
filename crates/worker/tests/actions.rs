//! The setup actions install the release the workflow declares.
//!
//! orm's CI names Go 1.27, Node.js from `.node-version` and PHP from
//! `.php-version`. Standing in for those actions with whatever `machine.json`
//! installed would run the checks on releases the workflow did not ask for.

use build_machine_core::workflow::{Adapter, Step};
use build_machine_worker::actions::{
    declared_version, go_mod_version, listed_checksum, php_extensions, php_packages, php_plan, php_version,
    resolve_go, resolve_node, version_spec, GoArchive,
};
use std::collections::BTreeMap;

fn step(adapter: Adapter, with: &[(&str, &str)]) -> Step {
    Step {
        index: 1,
        position: 1,
        name: "setup".to_owned(),
        adapter,
        action: None,
        action_ref: None,
        run: None,
        working_directory: None,
        condition: None,
        reason: None,
        env: BTreeMap::new(),
        with: with.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
        job_env: BTreeMap::new(),
        job_id: "test".to_owned(),
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
fn php_extensions_map_to_their_debian_packages() {
    let extensions = php_extensions(Some(&"pdo_mysql, pdo_pgsql, pdo_sqlite, openssl, mbstring, mysqli".to_owned())).unwrap();
    assert_eq!(
        php_packages("8.5", &extensions),
        ["php8.5-cli", "php8.5-mysql", "php8.5-pgsql", "php8.5-sqlite3", "php8.5-mbstring"]
    );
    assert_eq!(php_version("8.5").unwrap(), "8.5");
    for unsupported in ["8", "8.5.1", "latest"] {
        assert!(php_version(unsupported).is_err(), "{unsupported}");
    }
    // Disabling extensions changes the image instead of adding to it.
    for unsupported in [":xdebug", "none", "redis-6.0.2"] {
        assert!(php_extensions(Some(&unsupported.to_owned())).is_err(), "{unsupported}");
    }
}

#[test]
fn the_php_plan_reads_the_version_file_and_refuses_what_it_does_not_install() {
    let read = files(&[(".php-version", "8.5\n")]);
    let orm = step(Adapter::PhpSetup, &[("php-version-file", ".php-version"), ("extensions", "pdo_mysql, pdo_pgsql, pdo_sqlite, openssl")]);
    let (version, extensions, packages) = php_plan(&orm, &read).unwrap();
    assert_eq!(version, "8.5");
    assert_eq!(extensions, ["pdo_mysql", "pdo_pgsql", "pdo_sqlite", "openssl"]);
    assert_eq!(packages, ["php8.5-cli", "php8.5-mysql", "php8.5-pgsql", "php8.5-sqlite3"]);
    assert!(php_plan(&step(Adapter::PhpSetup, &[]), &read).is_err(), "a version is required");
    let tools = step(Adapter::PhpSetup, &[("php-version", "8.5"), ("tools", "phpunit")]);
    assert!(php_plan(&tools, &read).is_err());
    let coverage = step(Adapter::PhpSetup, &[("php-version", "8.5"), ("coverage", "xdebug")]);
    assert!(php_plan(&coverage, &read).is_err());
}
