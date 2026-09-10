//! Making sure a desktop session exists to build and launch in.
//!
//! Builds and launches run as the signed-in user, so `prlctl exec
//! --current-user` needs a session to attach to. A machine that has just been
//! installed or restarted sits at the login screen and has none, and waiting
//! for a person to sign in is not provisioning.
//!
//! This is part of `setup-system`: declared, idempotent, and it does nothing
//! when the machine is already in the state it asks for.
#![cfg(target_os = "linux")]

use crate::stream;
use anyhow::{Context, Result};
use std::path::Path;

const GDM_CONFIG: &str = "/etc/gdm3/custom.conf";

/// The configuration that signs the desktop user in automatically.
///
/// Returns the new text only when a change is needed. The commented examples
/// the file ships with are not settings, so a line has to be both uncommented
/// and correct to count as already configured.
pub fn configure_autologin(text: &str, user: &str) -> Option<String> {
    let setting = |line: &str, key: &str| -> Option<String> {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    };
    let enabled = text.lines().any(|line| setting(line, "AutomaticLoginEnable").as_deref() == Some("true"));
    let named = text.lines().any(|line| setting(line, "AutomaticLogin").as_deref() == Some(user));
    if enabled && named {
        return None;
    }
    // Drop any existing setting rather than leaving a second one behind, then
    // declare both under the daemon section the file already has.
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| setting(line, "AutomaticLoginEnable").is_none() && setting(line, "AutomaticLogin").is_none())
        .map(str::to_owned)
        .collect();
    let position = lines.iter().position(|line| line.trim() == "[daemon]");
    let block = vec![
        "# Set by the build machine: builds and launches run in the desktop".to_owned(),
        "# user's own session, which has to exist after a restart too.".to_owned(),
        "AutomaticLoginEnable = true".to_owned(),
        format!("AutomaticLogin = {user}"),
    ];
    match position {
        Some(index) => lines.splice(index + 1..index + 1, block),
        None => {
            lines.push("[daemon]".to_owned());
            lines.splice(lines.len().., block)
        }
    };
    let mut result = lines.join("\n");
    result.push('\n');
    Some(result)
}

/// Whether the declared desktop user currently has a session.
pub fn has_session(user: &str, environment: &[(String, String)]) -> bool {
    stream::capture("loginctl", &["list-sessions".to_owned(), "--no-legend".to_owned()], environment)
        .map(|output| {
            output.lines().any(|line| {
                let mut fields = line.split_whitespace();
                let _session = fields.next();
                let _uid = fields.next();
                fields.next() == Some(user)
            })
        })
        .unwrap_or(false)
}

/// Bring up a session for the declared desktop user, if there is not one.
///
/// Called as root from `setup-system`. Restarting the display manager is only
/// reached when nobody is signed in, so no one's session is taken away.
pub fn ensure_session(user: &str, environment: &[(String, String)]) -> Result<()> {
    let path = Path::new(GDM_CONFIG);
    if !path.is_file() {
        println!("OK: no GDM configuration on this machine; the desktop session is left alone.");
        return Ok(());
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("{GDM_CONFIG}를 읽지 못했어요."))?;
    match configure_autologin(&text, user) {
        None => println!("OK: {user} already signs in automatically. No change."),
        Some(updated) => {
            let backup = path.with_extension("conf.before-build-machine");
            if !backup.exists() {
                std::fs::copy(path, &backup)?;
            }
            std::fs::write(path, updated).with_context(|| format!("{GDM_CONFIG}를 쓰지 못했어요."))?;
            println!("Configured {user} to sign in automatically. Previous file: {}", backup.display());
        }
    }
    if has_session(user, environment) {
        println!("OK: {user} is signed in. No restart.");
        return Ok(());
    }
    println!("No desktop session for {user}; restarting the display manager to start one.");
    stream::checked("systemctl", &["restart".to_owned(), "gdm".to_owned()], None, environment)?;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if has_session(user, environment) {
            println!("OK: {user} is signed in.");
            return Ok(());
        }
    }
    anyhow::bail!("A desktop session for {user} did not start. Sign in on the machine and run setup again.")
}
