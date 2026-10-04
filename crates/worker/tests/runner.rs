//! `GITHUB_ENV` and `GITHUB_PATH` carry one step's environment to the next.
//!
//! orm's CI writes the database servers' DSNs into `$GITHUB_ENV` and the
//! PostgreSQL programs into `$GITHUB_PATH`; the steps after read them.

use build_machine_core::Platform;
use build_machine_worker::build::workflow_shell;
use build_machine_worker::runner::{parse_env_file, Runner};
use std::path::PathBuf;

#[test]
fn an_env_file_holds_lines_and_delimited_blocks() {
    let parsed = parse_env_file("A=1\n\nB=x=y\nNOTES<<EOF\nfirst\nsecond\nEOF\nC=\n").unwrap();
    assert_eq!(
        parsed,
        [
            ("A".to_owned(), "1".to_owned()),
            ("B".to_owned(), "x=y".to_owned()),
            ("NOTES".to_owned(), "first\nsecond".to_owned()),
            ("C".to_owned(), String::new()),
        ]
    );
    for malformed in ["no equals sign", "1A=x", "BLOCK<<EOF\nnever closed", "A B=1"] {
        assert!(parse_env_file(malformed).is_err(), "{malformed:?}");
    }
}

fn value<'a>(environment: &'a [(String, String)], key: &str) -> Option<&'a str> {
    environment.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
}

#[test]
fn what_a_step_writes_reaches_the_steps_after_it() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("source");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut runner = Runner::new(&directory.path().join("runner"), &workspace, Platform::Linux).unwrap();
    let base = vec![("PATH".to_owned(), "/usr/bin".to_owned()), ("KEEP".to_owned(), "1".to_owned())];

    let first = runner.begin_step(1).unwrap();
    let environment = runner.environment(&base, &first);
    assert_eq!(value(&environment, "GITHUB_WORKSPACE"), Some(workspace.to_str().unwrap()));
    assert_eq!(value(&environment, "GITHUB_ACTIONS"), Some("true"));
    assert_eq!(value(&environment, "RUNNER_OS"), Some("Linux"));
    let env_file = PathBuf::from(value(&environment, "GITHUB_ENV").unwrap());
    let path_file = PathBuf::from(value(&environment, "GITHUB_PATH").unwrap());
    std::fs::write(&env_file, "MYSQL_DSN=root@tcp(127.0.0.1:3306)/orm_test\n").unwrap();
    std::fs::write(&path_file, "/usr/lib/postgresql/17/bin\n").unwrap();
    runner.finish_step(&first, None).unwrap();
    runner.add_path(PathBuf::from("/opt/go/bin"));

    let second = runner.begin_step(2).unwrap();
    let environment = runner.environment(&base, &second);
    assert_eq!(value(&environment, "MYSQL_DSN"), Some("root@tcp(127.0.0.1:3306)/orm_test"));
    assert_eq!(value(&environment, "KEEP"), Some("1"));
    // The most recent addition comes first, as on a runner.
    assert_eq!(value(&environment, "PATH"), Some("/opt/go/bin:/usr/lib/postgresql/17/bin:/usr/bin"));
    // Each step writes its own, empty files.
    assert_ne!(value(&environment, "GITHUB_ENV").map(PathBuf::from), Some(env_file));
    assert_eq!(std::fs::read_to_string(&second.env).unwrap(), "");
}

#[test]
fn a_new_replay_starts_without_what_an_earlier_one_set() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("runner");
    let mut runner = Runner::new(&state, directory.path(), Platform::Linux).unwrap();
    let files = runner.begin_step(1).unwrap();
    std::fs::write(&files.env, "LEFTOVER=1\n").unwrap();
    runner.finish_step(&files, None).unwrap();
    std::fs::write(state.join("stale"), "x").unwrap();

    let fresh = Runner::new(&state, directory.path(), Platform::Linux).unwrap();
    let files = fresh.begin_step(1).unwrap();
    assert_eq!(value(&fresh.environment(&[], &files), "LEFTOVER"), None);
    assert!(!state.join("stale").exists());
}

/// GitHub runs a `run:` block that names no shell with `bash -e`. The `sh -eu`
/// of a custom build command would fail a block that reads an unset variable
/// or uses bash syntax, which passes on a runner.
#[test]
fn a_workflow_step_runs_in_the_runners_shell() {
    let (program, arguments) = workflow_shell("echo ${UNSET_ON_PURPOSE:-}");
    if cfg!(windows) {
        assert!(program.contains("powershell"), "{program}");
    } else {
        assert_eq!(program, "bash");
        assert_eq!(arguments, ["-e", "-c", "echo ${UNSET_ON_PURPOSE:-}"]);
    }
}

/// A step with an `id` sets outputs through `$GITHUB_OUTPUT`, in the format of
/// `$GITHUB_ENV`, and the steps after it read them as
/// `steps.<id>.outputs.<name>`. A step without an `id` has no outputs.
#[test]
fn what_a_step_writes_to_github_output_is_its_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let mut runner = Runner::new(&directory.path().join("runner"), directory.path(), Platform::Linux).unwrap();

    let first = runner.begin_step(1).unwrap();
    let output = PathBuf::from(value(&runner.environment(&[], &first), "GITHUB_OUTPUT").unwrap());
    std::fs::write(&output, "version=8.4\nnotes<<EOF\na\nb\nEOF\n").unwrap();
    runner.finish_step(&first, Some("php-min")).unwrap();

    let second = runner.begin_step(2).unwrap();
    let path = PathBuf::from(value(&runner.environment(&[], &second), "GITHUB_OUTPUT").unwrap());
    assert_ne!(path, output);
    std::fs::write(&path, "ignored=1\n").unwrap();
    runner.finish_step(&second, None).unwrap();

    assert_eq!(runner.outputs()["php-min"]["version"], "8.4");
    assert_eq!(runner.outputs()["php-min"]["notes"], "a\nb");
    assert_eq!(runner.outputs().len(), 1);
    let template = build_machine_core::workflow::Template::parse("${{ steps.php-min.outputs.version }}").unwrap();
    assert_eq!(template.render(runner.outputs(), &Default::default()), "8.4");
}
