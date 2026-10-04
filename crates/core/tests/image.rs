//! The Linux machine provides the runner image its workflows ask for.
//!
//! setup-php's cached PHP builds link against libraries the GitHub runner
//! image carries because the image installs PHP 8.5 with its extensions. A
//! replay machine without that package set ran `php` into "libsodium.so.23:
//! cannot open shared object file".

use build_machine_core::config::Machine;
use build_machine_core::Platform;
use std::path::Path;

fn machine() -> Machine {
    Machine::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../machine.json")).unwrap()
}

#[test]
fn the_linux_profile_declares_the_runner_image_it_stands_for() {
    let machine = machine();
    let profile = machine.profile(Platform::Linux).unwrap();
    let image = profile.image.as_ref().expect("the Linux profile declares its runner image");
    assert_eq!(image.runner, profile.runner);
    assert!(!image.version.is_empty());
    assert!(image.source.iter().any(|source| source.contains("Ubuntu2604-Arm64-Readme.md")));
    // The image's PHP 8.5 and the extension packages whose libraries
    // setup-php's builds load (libsodium, libargon2, libpq, libzip, …).
    for package in ["php8.5-cli", "php8.5-common", "php8.5-pgsql", "php8.5-zip", "php8.5-amqp", "php8.5-tidy", "sqlite3", "jq", "xvfb"] {
        assert!(image.packages.iter().any(|name| name == package), "{package}");
    }
    // What the machine does not provide of the image is stated, not omitted.
    assert!(!image.not_provided.is_empty());
}

#[test]
fn setup_installs_the_profile_s_packages_and_the_image_s() {
    let machine = machine();
    let profile = machine.profile(Platform::Linux).unwrap();
    let packages = profile.system_packages();
    for package in profile.packages.iter().chain(&profile.image.as_ref().unwrap().packages) {
        assert_eq!(packages.iter().filter(|name| *name == package).count(), 1, "{package}");
    }
}
