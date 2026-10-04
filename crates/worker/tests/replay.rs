//! How a replay orders steps and bounds their time.

use build_machine_core::workflow;
use build_machine_worker::ci::{ordered_steps, step_limit, Limit};
use std::time::Duration;

/// A step without `timeout-minutes` is bounded only by what is left of its
/// job's limit, as on GitHub; a step with one by the smaller of the two.
#[test]
fn a_step_runs_for_its_own_limit_within_its_job_s() {
    let hour = Duration::from_secs(3600);
    assert_eq!(step_limit(None, hour), (hour, Limit::Job));
    assert_eq!(step_limit(Some(5), hour), (Duration::from_secs(300), Limit::Step));
    assert_eq!(step_limit(Some(90), hour), (hour, Limit::Job));
}

/// orm's "database servers" step runs `make test-servers`, classified as
/// test, and the next step reads the file it writes and is classified as
/// setup. The request a worker receives runs them in workflow order.
#[test]
fn the_worker_runs_steps_in_workflow_order_whatever_their_stage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ci.yml");
    std::fs::write(
        &path,
        r#"# build-machine: skip build reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: ubuntu-26.04-arm
    steps:
      - uses: actions/checkout@v5
      - name: database servers
        run: make test-servers
      - name: environment of the database servers
        run: sed -e 's/^export //' .runtime/servers/env >> "$GITHUB_ENV"
      - name: make check
        run: make check
      - name: decoder fuzz smoke checks
        if: ${{ !cancelled() }}
        run: make fuzz-check
      - name: Go
        run: go test ./...
"#,
    )
    .unwrap();
    let parsed = workflow::load(&path, "workflow_dispatch", None).unwrap();
    let request = build_machine_core::request::WorkRequest {
        protocol: build_machine_core::request::PROTOCOL.to_owned(),
        snapshot: serde_json::from_value(serde_json::json!({
            "revision": "r", "dirty": false, "sourceHash": "h", "fileCount": 0, "sourceMode": "local",
            "projectKey": "orm", "project": "/orm", "archive": ""
        }))
        .unwrap(),
        archive: String::new(),
        history: None,
        repositories: Default::default(),
        target: String::new(),
        bundle: None,
        framework: None,
        command: None,
        artifact: None,
        jobs: workflow::jobs_for(&parsed, Some(build_machine_core::Platform::Linux)),
        skips: parsed.skips.clone(),
        workflow_signature: None,
    };
    let document = serde_json::to_string(&request).unwrap();
    let request: build_machine_core::request::WorkRequest = serde_json::from_str(&document).unwrap();
    let order: Vec<(&str, &str)> =
        ordered_steps(&request).into_iter().map(|(stage, step)| (stage, step.name.as_str())).collect();
    assert_eq!(
        order,
        [
            ("setup", "actions/checkout@v5"),
            ("test", "database servers"),
            ("setup", "environment of the database servers"),
            ("test", "make check"),
            ("smoke", "decoder fuzz smoke checks"),
            ("test", "Go"),
        ]
    );
}

/// A job asks GitHub for a runner image; the replay runs on this machine. A
/// difference the label states — another Ubuntu release or another
/// architecture — is recorded, not passed over.
#[test]
fn a_runner_image_the_machine_does_not_match_is_a_recorded_limit() {
    use build_machine_worker::ci::runner_image_limit;
    assert_eq!(runner_image_limit("ubuntu-26.04-arm", "26.04", "aarch64"), None);
    assert_eq!(runner_image_limit("ubuntu-26.04", "26.04", "x86_64"), None);
    let release = runner_image_limit("ubuntu-24.04-arm", "26.04", "aarch64").unwrap();
    assert!(release.contains("24.04") && release.contains("26.04"), "{release}");
    let arch = runner_image_limit("ubuntu-26.04", "26.04", "aarch64").unwrap();
    assert!(arch.contains("x86_64") || arch.contains("X64"), "{arch}");
    // ubuntu-latest names no release, which is itself a difference to record.
    assert!(runner_image_limit("ubuntu-latest", "26.04", "aarch64").is_some());
}

/// `$GITHUB_STEP_SUMMARY` exists for every step, as on a runner, so a step
/// that appends to it does not fail on an unset variable.
#[test]
fn every_step_has_a_step_summary_file() {
    use build_machine_worker::runner::Runner;
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(&directory.path().join("runner"), directory.path(), build_machine_core::Platform::Linux).unwrap();
    let files = runner.begin_step(1).unwrap();
    let environment = runner.environment(&[], &files);
    let summary = environment.iter().find(|(key, _)| key == "GITHUB_STEP_SUMMARY").map(|(_, value)| value.clone()).unwrap();
    assert!(std::path::Path::new(&summary).is_file());
}

/// A job's workspace is removed when the job ends: two orm replays left 21 GB
/// of workspaces, a Rust target of 7 GB each, in the Linux machine. What the
/// result names — the artifacts with their checksums — is kept beside it.
#[test]
fn a_finished_job_leaves_only_its_artifacts() {
    use build_machine_core::report::Artifact;
    use build_machine_worker::ci::finish_job;
    let directory = tempfile::tempdir().unwrap();
    let work = directory.path().join("work");
    let workspace = work.join("orm").join("orm");
    let bundle = workspace.join("src-tauri/target/release/bundle/deb");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::create_dir_all(workspace.join("target/debug")).unwrap();
    std::fs::write(workspace.join("target/debug/huge"), vec![0u8; 4096]).unwrap();
    std::fs::write(bundle.join("app.deb"), b"package").unwrap();
    let sha256 = build_machine_core::source::sha256_file(&bundle.join("app.deb")).unwrap();
    let elsewhere = directory.path().join("other.deb");
    std::fs::write(&elsewhere, b"not this job's").unwrap();
    let mut artifacts = vec![
        Artifact { path: bundle.join("app.deb").to_string_lossy().into_owned(), sha256: sha256.clone(), size: 7 },
        Artifact { path: elsewhere.to_string_lossy().into_owned(), sha256: "x".to_owned(), size: 1 },
    ];
    let keep = directory.path().join("artifacts").join("test");

    finish_job(&work, &keep, &mut artifacts).unwrap();

    assert!(!work.exists());
    let kept = keep.join("orm/orm/src-tauri/target/release/bundle/deb/app.deb");
    assert_eq!(artifacts[0].path, kept.to_string_lossy());
    assert_eq!(build_machine_core::source::sha256_file(&kept).unwrap(), sha256);
    assert_eq!(artifacts[1].path, elsewhere.to_string_lossy());
}

/// Steps see the runner image's declared environment. The Linux worker runs
/// through `runuser -l`, whose PATH comes from login.defs ENV_PATH and has no
/// /usr/sbin, so orm's `test "$(command -v mysqld)" = /usr/sbin/mysqld` failed
/// where the image's /etc/environment PATH finds it.
#[test]
fn a_replay_runs_under_the_runner_image_s_environment() {
    use build_machine_core::config::Machine;
    use build_machine_worker::ci::image_environment;
    let machine = Machine::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../machine.json")).unwrap();
    let image = machine.profile(build_machine_core::Platform::Linux).unwrap().image.clone().unwrap();
    let base = vec![
        ("PATH".to_owned(), "/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games".to_owned()),
        ("HOME".to_owned(), "/home/parallels".to_owned()),
        ("KEEP".to_owned(), "1".to_owned()),
    ];
    let managed = vec![std::path::PathBuf::from("/home/parallels/.local/share/build-machine/node/bin")];
    let environment = image_environment(&base, &image, "/home/parallels", &managed);
    let value = |key: &str| environment.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
    let path = value("PATH").unwrap();
    assert!(path.starts_with("/home/parallels/.local/share/build-machine/node/bin:/snap/bin:/home/parallels/.local/bin:"), "{path}");
    for entry in ["/usr/local/sbin", "/usr/sbin", "/sbin", "/home/parallels/.cargo/bin"] {
        assert!(path.split(':').any(|part| part == entry), "{entry} in {path}");
    }
    assert!(!path.contains("$HOME"), "{path}");
    assert_eq!(value("DEBIAN_FRONTEND"), Some("noninteractive"));
    assert_eq!(value("XDG_CONFIG_HOME"), Some("/home/parallels/.config"));
    assert_eq!(value("KEEP"), Some("1"));
    assert_eq!(environment.iter().filter(|(name, _)| name == "PATH").count(), 1);
}

/// A GitHub runner sets no NO_COLOR. The machine's builds set it for their
/// own output; a step that sets FORCE_COLOR then saw node warn that
/// NO_COLOR is ignored, and soksak's repeat test failed on that warning.
#[test]
fn a_replay_does_not_see_the_variables_of_the_machine_s_builds() {
    use build_machine_worker::ci::replay_base;
    let pair = |key: &str, value: &str| (key.to_owned(), value.to_owned());
    let base = replay_base(&[
        pair("NO_COLOR", "1"),
        pair("RUSTUP_TOOLCHAIN", "1.95.0"),
        pair("GITHUB_TOKEN", "secret"),
        pair("APPLE_ID", "secret"),
        pair("CI", "true"),
        pair("KEEP", "1"),
    ]);
    assert_eq!(base, vec![pair("CI", "true"), pair("KEEP", "1")]);
}

/// A job runs in the runner's own layout, `$HOME/work/<repo>/<repo>`, as a
/// runner checks out to /home/runner/work/<repo>/<repo>. Under the worker's
/// state directory the workspace was about 85 characters long, and orm's
/// MySQL socket under the checkout passed the 107-byte limit of a unix
/// socket path ("The socket file path is too long (> 107)").
#[test]
fn a_job_runs_in_the_runner_s_workspace_layout() {
    use build_machine_worker::ci::runner_work;
    let work = runner_work(std::path::Path::new("/home/parallels"), "orm");
    assert_eq!(work.runner_workspace, std::path::Path::new("/home/parallels/work/orm"));
    assert_eq!(work.workspace, std::path::Path::new("/home/parallels/work/orm/orm"));
    assert_eq!(work.temp, std::path::Path::new("/home/parallels/work/_temp"));
    let socket = work.workspace.join(".runtime/servers/mysql-replica/replica.sock");
    assert!(socket.to_string_lossy().len() <= 107, "{}", socket.display());
    // The same socket under the runner's own home is as long as on GitHub.
    let github = runner_work(std::path::Path::new("/home/runner"), "orm").workspace.join(".runtime/servers/mysql-replica/replica.sock");
    assert_eq!(github.to_string_lossy().len() + "parallels".len() - "runner".len(), socket.to_string_lossy().len());
}

/// A job runs by its `if:` against the results of the jobs before it. orm's
/// deploy job runs only for a push to main; without an `if` a job runs when
/// every job before it — the jobs it needs and theirs — succeeded.
#[test]
fn a_job_runs_by_its_condition_and_the_jobs_before_it() {
    use build_machine_core::workflow::{Github, Job, JobResult};
    use build_machine_worker::ci::job_gate;
    use std::collections::BTreeMap;
    let job = |id: &str, needs: &[&str], condition: Option<&str>| Job {
        id: id.to_owned(),
        runs_on: "ubuntu-26.04-arm".to_owned(),
        needs: needs.iter().map(|need| (*need).to_owned()).collect(),
        timeout_minutes: 360,
        env: BTreeMap::new(),
        steps: Vec::new(),
        environment: None,
        reads_ref: false,
        condition: condition.map(str::to_owned),
    };
    let jobs = vec![
        job("build", &[], None),
        job("test", &["build"], None),
        job("deploy", &["test"], Some("github.event_name != 'pull_request' && github.ref == 'refs/heads/main'")),
        job("report", &["test"], Some("always()")),
    ];
    let main = Github { event_name: "push".to_owned(), sha: "s".to_owned(), reference: Some("refs/heads/main".to_owned()) };
    let results = |entries: &[(&str, JobResult)]| -> BTreeMap<String, JobResult> {
        entries.iter().map(|(id, result)| ((*id).to_owned(), *result)).collect()
    };
    let passed = results(&[("build", JobResult::Success), ("test", JobResult::Success)]);
    assert_eq!(job_gate(&jobs[2], &passed, &jobs, &main).unwrap(), None);
    let topic = Github { reference: Some("refs/heads/topic".to_owned()), ..main.clone() };
    assert!(job_gate(&jobs[2], &passed, &jobs, &topic).unwrap().unwrap().contains("if"));
    // build failed and test was skipped: deploy is skipped; report, always(), runs.
    let failed = results(&[("build", JobResult::Failure), ("test", JobResult::Skipped)]);
    assert!(job_gate(&jobs[1], &results(&[("build", JobResult::Failure)]), &jobs, &main).unwrap().is_some());
    assert!(job_gate(&jobs[2], &failed, &jobs, &main).unwrap().is_some());
    assert_eq!(job_gate(&jobs[3], &failed, &jobs, &main).unwrap(), None);
}
