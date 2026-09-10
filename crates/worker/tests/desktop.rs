#![cfg(target_os = "linux")]

use build_machine_worker::desktop;

const SHIPPED: &str = "\
# GDM configuration storage

[daemon]

# Enabling automatic login
#  AutomaticLoginEnable = true
#  AutomaticLogin = user1

[security]
";

/// The file ships with the settings commented out as examples. Reading a
/// commented line as configuration is what makes a machine sit at the login
/// screen while provisioning reports success.
#[test]
fn commented_examples_are_not_configuration() {
    let updated = desktop::configure_autologin(SHIPPED, "parallels").expect("a change is needed");
    assert!(updated.contains("\nAutomaticLoginEnable = true\n"));
    assert!(updated.contains("\nAutomaticLogin = parallels\n"));
    assert!(updated.contains("#  AutomaticLoginEnable = true"), "the examples are left as they were");
}

/// Running it again changes nothing, which is what makes it safe to run on
/// every setup.
#[test]
fn a_machine_already_in_this_state_is_left_alone() {
    let updated = desktop::configure_autologin(SHIPPED, "parallels").unwrap();
    assert!(desktop::configure_autologin(&updated, "parallels").is_none());
}

/// A different user is a different state, and the old setting does not survive.
#[test]
fn changing_the_user_replaces_the_setting_rather_than_adding_one() {
    let first = desktop::configure_autologin(SHIPPED, "parallels").unwrap();
    let second = desktop::configure_autologin(&first, "builder").expect("a change is needed");
    assert_eq!(second.matches("\nAutomaticLogin = ").count(), 1);
    assert!(second.contains("\nAutomaticLogin = builder\n"));
    assert!(!second.contains("\nAutomaticLogin = parallels\n"));
}

/// A file without the section still ends up configured.
#[test]
fn a_file_without_a_daemon_section_gains_one() {
    let updated = desktop::configure_autologin("[security]\n", "parallels").unwrap();
    assert!(updated.contains("[daemon]"));
    assert!(updated.contains("AutomaticLogin = parallels"));
    assert!(desktop::configure_autologin(&updated, "parallels").is_none());
}
