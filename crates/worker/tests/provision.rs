//! The apt commands setup runs.

use build_machine_core::config::Machine;
use build_machine_core::Platform;
use build_machine_worker::provision::apt_install_commands;
use std::path::Path;

/// actions/runner-images installs its toolset packages with
/// `apt-get install --no-install-recommends` (install-apt-vital.sh,
/// install-apt-common.sh, install-php.sh) under
/// `APT::Get::Always-Include-Phased-Updates "true"` (configure-apt.sh). The
/// image's packages are installed the same way, so the machine does not carry
/// what their recommendations would add. The machine's own packages keep apt's
/// defaults, as their installation guides use them.
#[test]
fn the_image_s_packages_are_installed_as_the_image_installs_them() {
    let machine = Machine::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../machine.json")).unwrap();
    let profile = machine.profile(Platform::Linux).unwrap();
    let missing: Vec<String> = ["git", "libwebkit2gtk-4.1-dev", "xvfb", "php8.5-cli", "jq"].iter().map(|name| (*name).to_owned()).collect();
    let commands = apt_install_commands(profile, &missing);
    assert_eq!(
        commands,
        [
            vec!["install", "-y", "git", "libwebkit2gtk-4.1-dev", "xvfb"],
            vec![
                "install",
                "-y",
                "--no-install-recommends",
                "-o",
                "APT::Get::Always-Include-Phased-Updates=true",
                "php8.5-cli",
                "jq"
            ],
        ]
    );
    assert_eq!(apt_install_commands(profile, &["git".to_owned()]), [vec!["install", "-y", "git"]]);
    assert!(apt_install_commands(profile, &[]).is_empty());
}
