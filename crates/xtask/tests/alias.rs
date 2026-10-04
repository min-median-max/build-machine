//! The README runs every development task as `cargo xtask …`. Cargo has no
//! such subcommand of its own, so the workspace has to declare it.

use std::path::Path;
use std::process::Command;

#[test]
fn cargo_xtask_runs_the_task_runner_from_the_workspace() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let output = Command::new(env!("CARGO"))
        .args(["xtask", "--help"])
        .current_dir(workspace)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    for task in ["dev", "build", "test", "worker"] {
        assert!(stdout.lines().any(|line| line.trim_start().starts_with(task)), "{task} missing:\n{stdout}");
    }
}
