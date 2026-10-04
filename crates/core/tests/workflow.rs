use build_machine_core::workflow;
use std::path::PathBuf;

fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let workflows = directory.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    let path = workflows.join("build.yml");
    std::fs::write(&path, text).unwrap();
    (directory, path)
}

fn load(text: &str) -> anyhow::Result<workflow::Workflow> {
    let (_directory, path) = write(text);
    workflow::load(&path, "workflow_dispatch", None)
}

#[test]
fn supported_steps_and_explicit_skip_markers_are_read() {
    let parsed = load(
        r#"# build-machine: skip test reason=no test command in this release workflow
# build-machine: skip smoke reason=smoke is verified by the desktop fixture
name: release
on:
  workflow_dispatch:
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - run: pnpm install --frozen-lockfile
      - uses: tauri-apps/tauri-action@v0
        with:
          args: --target universal-apple-darwin
"#,
    )
    .unwrap();
    assert_eq!(parsed.name, "release");
    assert_eq!(parsed.skips["smoke"], "smoke is verified by the desktop fixture");
    assert_eq!(parsed.jobs[0].steps.len(), 4);
    let stages = workflow::stages(&parsed);
    assert_eq!(stages["build"][0].adapter, workflow::Adapter::TauriBuild);
}

#[test]
fn unsupported_action_fails_closed() {
    let error = load(
        r#"name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - uses: evil/action@v1
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("어댑터"), "{error:#}");
}

#[test]
fn a_missing_gate_requires_a_reasoned_comment() {
    let error = load(
        r#"name: bad
on: workflow_dispatch
jobs:
  build:
    runs-on: ubuntu
    steps:
      - name: build
        run: echo build
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("test 단계"), "{error:#}");
}

/// `actions/checkout` contains "check". Reading an action's own name as a stage
/// let a workflow with no test command satisfy the test gate.
#[test]
fn checkout_does_not_satisfy_the_test_gate() {
    let error = load(
        r#"name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("test 단계"), "{error:#}");
}

#[test]
fn action_steps_are_classified_by_adapter_not_by_name() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - name: Restore build cache
        uses: actions/cache@v4
      - uses: tauri-apps/tauri-action@v0
      - uses: actions/upload-artifact@v4
"#,
    )
    .unwrap();
    let stages = workflow::stages(&parsed);
    let adapters = |stage: &str| -> Vec<workflow::Adapter> {
        stages[stage].iter().map(|step| step.adapter).collect()
    };
    assert_eq!(adapters("setup"), [workflow::Adapter::Checkout, workflow::Adapter::Cache]);
    assert_eq!(adapters("build"), [workflow::Adapter::TauriBuild]);
    assert_eq!(adapters("release"), [workflow::Adapter::ArtifactUpload]);
    assert_eq!(adapters("test"), [workflow::Adapter::Skip]);
}

#[test]
fn an_unsupported_job_shape_fails_closed() {
    for unsupported in ["container: node:20", "strategy:\n      matrix:\n        os: [a, b]"] {
        let text = format!(
            "name: bad\non: workflow_dispatch\njobs:\n  build:\n    runs-on: ubuntu\n    {unsupported}\n    steps:\n      - run: echo build\n"
        );
        let error = load(&text).unwrap_err();
        assert!(format!("{error:#}").contains("지원하지 않아요"), "{error:#}");
    }
}

#[test]
fn an_event_the_workflow_does_not_declare_is_rejected() {
    let (_directory, path) = write("name: bad\non: push\njobs:\n  build:\n    runs-on: ubuntu\n    steps:\n      - run: echo build\n");
    let error = workflow::load(&path, "workflow_dispatch", None).unwrap_err();
    assert!(format!("{error:#}").contains("workflow_dispatch 이벤트"), "{error:#}");
}

#[test]
fn job_order_follows_needs_and_a_cycle_is_rejected() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: ordered
on: workflow_dispatch
jobs:
  publish:
    runs-on: ubuntu
    needs: compile
    steps:
      - uses: actions/upload-artifact@v4
  compile:
    runs-on: ubuntu
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    assert_eq!(parsed.jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(), ["compile", "publish"]);

    let error = load(
        r#"name: cycle
on: workflow_dispatch
jobs:
  a:
    runs-on: ubuntu
    needs: b
    steps:
      - run: echo build
  b:
    runs-on: ubuntu
    needs: a
    steps:
      - run: echo build
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("순환"), "{error:#}");
}

/// The controller writes the stages into a request document and the worker
/// reads them back. An adapter that does not survive that trip is read as a
/// shell step, and the run fails on a command that was never there.
#[test]
fn every_adapter_survives_the_request_document() {
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
      - uses: actions/setup-go@v6
      - uses: shivammathur/setup-php@v2
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
      - run: echo compile
      - uses: tauri-apps/tauri-action@v0
      - uses: actions/upload-artifact@v4
      - uses: softprops/action-gh-release@v2
"#,
    )
    .unwrap();
    let stages = workflow::stages(&parsed);
    let document = serde_json::to_string(&stages).unwrap();
    let restored: std::collections::BTreeMap<String, Vec<workflow::Step>> =
        serde_json::from_str(&document).unwrap();
    assert_eq!(restored.len(), stages.len());
    for (stage, steps) in &stages {
        let read_back = &restored[stage];
        assert_eq!(read_back.len(), steps.len(), "{stage}");
        for (before, after) in steps.iter().zip(read_back) {
            assert_eq!(after.adapter, before.adapter, "{stage} step {}", before.index);
            assert_eq!(after.run, before.run, "{stage} step {}", before.index);
            assert_eq!(after.reason, before.reason, "{stage} step {}", before.index);
        }
    }
    // The one adapter that is not a shell step must not read back as one.
    assert!(restored["setup"].iter().all(|step| step.adapter != workflow::Adapter::Run));
}

/// A three-OS release workflow has a job per operating system. Each platform
/// takes the jobs its own runner label claims, so the Windows job never runs on
/// Linux and every platform still gets a complete set of stages.
#[test]
fn each_platform_replays_only_the_jobs_written_for_it() {
    use build_machine_core::Platform;
    let parsed = load(
        r#"# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  macos:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - name: Test on macOS
        run: cargo test
      - uses: tauri-apps/tauri-action@v0
  linux:
    runs-on: ubuntu-24.04-arm
    steps:
      - uses: actions/checkout@v4
      - name: Test on Linux
        run: cargo test
      - uses: tauri-apps/tauri-action@v0
  windows:
    runs-on: windows-11-arm
    steps:
      - uses: actions/checkout@v4
      - name: Test on Windows
        run: cargo test
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    assert_eq!(workflow::declared_platforms(&parsed).len(), 3);
    for (platform, expected) in [
        (Platform::Macos, "Test on macOS"),
        (Platform::Linux, "Test on Linux"),
        (Platform::Windows, "Test on Windows"),
    ] {
        let stages = workflow::stages_for(&parsed, Some(platform));
        let names: Vec<&str> = stages["test"].iter().map(|step| step.name.as_str()).collect();
        assert_eq!(names, [expected], "{platform}");
        assert_eq!(stages["build"].len(), 1, "{platform} builds once");
        assert_eq!(stages["setup"].len(), 1, "{platform} checks out once");
        assert_eq!(stages["smoke"].len(), 1, "{platform} records the skip");
    }
    // Without a platform every job is included, which is what validation reads.
    assert_eq!(workflow::stages(&parsed)["test"].len(), 3);
}

/// A runner this machine does not recognise constrains nothing, so such a job
/// is replayed everywhere rather than silently dropped.
#[test]
fn an_unfamiliar_runner_runs_on_every_platform() {
    use build_machine_core::Platform;
    let parsed = load(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  build:
    runs-on: self-hosted
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap();
    assert!(workflow::declared_platforms(&parsed).is_empty());
    for platform in Platform::ALL {
        assert_eq!(workflow::stages_for(&parsed, Some(platform))["build"].len(), 1, "{platform}");
    }
}

/// A workflow with a job per operating system must satisfy the gates on each
/// of them. Testing on one and not another would otherwise pass validation
/// while that platform's rehearsal ran no tests at all.
#[test]
fn every_platform_the_workflow_covers_must_satisfy_the_gates() {
    let error = load(
        r#"# build-machine: skip smoke reason=fixture
name: release
on: workflow_dispatch
jobs:
  macos:
    runs-on: macos-14
    steps:
      - name: Test on macOS
        run: cargo test
      - uses: tauri-apps/tauri-action@v0
  linux:
    runs-on: ubuntu-24.04-arm
    steps:
      - uses: tauri-apps/tauri-action@v0
"#,
    )
    .unwrap_err();
    let text = format!("{error:#}");
    assert!(text.contains("linux"), "{text}");
    assert!(text.contains("test 단계"), "{text}");
}

/// orm's CI selects Go, Node.js and PHP through their setup actions. Each has
/// an adapter, an action's owner is matched without regard to case as GitHub
/// matches it, and every one of them is setup.
#[test]
fn setup_actions_of_go_node_and_php_are_supported() {
    let parsed = load(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/setup-go@v6
        with: { go-version: "1.27" }
      - uses: actions/setup-node@v7
        with:
          node-version-file: .node-version
          cache: npm
      - uses: Swatinem/rust-cache@v2
      - uses: shivammathur/setup-php@v2
        with:
          php-version-file: .php-version
          extensions: pdo_mysql, pdo_pgsql, pdo_sqlite, openssl
      - name: make check
        run: make check
"#,
    )
    .unwrap();
    let stages = workflow::stages(&parsed);
    let adapters: Vec<workflow::Adapter> = stages["setup"].iter().map(|step| step.adapter).collect();
    assert_eq!(
        adapters,
        [workflow::Adapter::GoSetup, workflow::Adapter::NodeSetup, workflow::Adapter::Cache, workflow::Adapter::PhpSetup]
    );
    assert_eq!(stages["setup"][3].with["php-version-file"], ".php-version");
}

/// An input the adapter does not honour would change what the action does, so
/// it fails validation instead of being dropped.
#[test]
fn an_input_a_setup_adapter_does_not_honour_fails_closed() {
    let error = load(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/setup-go@v6
        with: { go-version: "1.27", check-latest: true }
      - run: make check
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("check-latest"), "{error:#}");
}

/// `shell`, `continue-on-error` and `timeout-minutes` change how a runner runs
/// a step; dropping them would replay a different step.
#[test]
fn a_step_key_the_replay_does_not_honour_fails_closed() {
    for key in ["shell: python", "continue-on-error: true"] {
        let text = format!(
            "# build-machine: skip build reason=fixture\n# build-machine: skip smoke reason=fixture\nname: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: make check\n        {key}\n"
        );
        let error = load(&text).unwrap_err();
        let name = key.split(':').next().unwrap();
        assert!(format!("{error:#}").contains(name), "{key}: {error:#}");
    }
}

/// A repository whose CI produces no artifact says so, as it does for test and
/// smoke, instead of failing the build gate.
#[test]
fn a_build_stage_can_be_skipped_with_a_reason() {
    let text = "name: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: make check\n# build-machine: skip smoke reason=fixture\n";
    let error = load(text).unwrap_err();
    assert!(format!("{error:#}").contains("skip build reason="), "{error:#}");

    let parsed = load(&format!("# build-machine: skip build reason=the library ships no artifact\n{text}")).unwrap();
    let stages = workflow::stages(&parsed);
    assert_eq!(stages["build"][0].adapter, workflow::Adapter::Skip);
    assert_eq!(stages["build"][0].reason.as_deref(), Some("the library ships no artifact"));
}

/// A step's place in the workflow is kept, jobs in `needs` order, because a
/// replay runs steps in that order whatever stage reports them.
#[test]
fn steps_keep_their_workflow_position_across_jobs() {
    let parsed = load(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: workflow_dispatch
jobs:
  second:
    runs-on: ubuntu-24.04
    needs: first
    steps:
      - name: make check
        run: make check
  first:
    runs-on: ubuntu-24.04
    steps:
      - name: database servers
        run: make test-servers
      - name: environment of the database servers
        run: cat .runtime/servers/env >> "$GITHUB_ENV"
"#,
    )
    .unwrap();
    let mut steps: Vec<&workflow::Step> = parsed.jobs.iter().flat_map(|job| &job.steps).collect();
    steps.sort_by_key(|step| step.position);
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names, ["database servers", "environment of the database servers", "make check"]);
    // The stage of the second step is setup and of the first is test, which
    // is why execution cannot follow the stage order.
    assert_eq!(workflow::stage_of(steps[0]), "test");
    assert_eq!(workflow::stage_of(steps[1]), "setup");
}

#[test]
fn conditions_follow_the_job_status_as_github_evaluates_them() {
    use workflow::{Condition, JobStatus};
    let runs = |text: &str, failed: bool| {
        Condition::parse(Some(text))
            .unwrap()
            .runs(if failed { JobStatus::Failure } else { JobStatus::Success }, &Default::default(), &Default::default())
    };
    // No condition is success().
    assert!(Condition::parse(None).unwrap().runs(JobStatus::Success, &Default::default(), &Default::default()));
    assert!(!Condition::parse(None).unwrap().runs(JobStatus::Failure, &Default::default(), &Default::default()));
    // orm's later steps run after a failure, so one run reports every result.
    assert!(runs("${{ !cancelled() }}", true));
    assert!(runs("${{ !cancelled() }}", false));
    assert!(runs("always()", true));
    assert!(runs("failure()", true));
    assert!(!runs("failure()", false));
    // Without a status function a condition is success() && (condition).
    assert!(runs("true", false));
    assert!(!runs("true", true));
    assert!(!runs("${{ false }}", false));
    assert!(runs("success() || failure()", true));
    assert!(!runs("!(always())", false));
    assert_eq!(Condition::parse(Some("${{ secrets.TOKEN != '' }}")).unwrap(), Condition::Secret);
    for unknown in ["github.actor == 'octocat'", "hashFiles('x')", "steps.a.outputs", "(always()"] {
        assert!(Condition::parse(Some(unknown)).is_err(), "{unknown}");
    }
}

/// A condition that reads the event payload fails validation when the replay
/// has no payload, before anything runs.
#[test]
fn a_condition_without_a_local_value_fails_validation() {
    let workflow = load(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
      - run: make check
        if: github.event.pull_request.merged == true
"#,
    )
    .unwrap();
    let error = build_machine_core::workflow::github_context(&workflow.jobs, "workflow_dispatch", "s", None, None).unwrap_err();
    assert!(format!("{error:#}").contains("github.event.pull_request.merged"), "{error:#}");
}

/// A checkout step with `repository` names a workflow of `steps` around it:
/// the soksak component releases check out `soksak-app/core` beside their own
/// repository to build its `sok`.
fn checkout_of(with: &str) -> String {
    format!(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: ci
on: workflow_dispatch
jobs:
  test:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - id: version
        run: echo "tag=v0.0.2" >> "$GITHUB_OUTPUT"
      - uses: actions/checkout@v4
        with:
{with}
      - run: make check
"#
    )
}

/// `repository`, `ref` and `path` are read, and `ref` is a template whose
/// step outputs take their values when the step runs.
#[test]
fn a_checkout_of_another_repository_is_read_with_its_ref_and_path() {
    let parsed = load(&checkout_of(
        "          repository: soksak-app/core\n          ref: ${{ steps.version.outputs.tag }}\n          path: core",
    ))
    .unwrap();
    let step = &parsed.jobs[0].steps[2];
    assert_eq!(step.with["repository"], "soksak-app/core");
    assert_eq!(step.with["path"], "core");
    let mut outputs = workflow::Outputs::new();
    outputs.insert("version".to_owned(), [("tag".to_owned(), "v0.0.2".to_owned())].into_iter().collect());
    assert_eq!(step.resolve(&outputs, &Default::default()).unwrap().with["ref"], "v0.0.2");
}

/// `path` places the other repository inside the workspace; a path that
/// leaves it is refused before anything runs.
#[test]
fn a_checkout_path_outside_the_workspace_fails_validation() {
    for path in ["../core", "/tmp/core", "core/../../core"] {
        let error = load(&checkout_of(&format!("          repository: soksak-app/core\n          path: {path}"))).unwrap_err();
        assert!(format!("{error:#}").contains("GITHUB_WORKSPACE 안의 상대 경로"), "{path}: {error:#}");
    }
}

/// Without `repository` the step checks out the replayed revision, so a
/// `ref` would be dropped; it is refused instead.
#[test]
fn a_ref_without_a_repository_fails_validation() {
    let error = load(&checkout_of("          ref: v0.0.2")).unwrap_err();
    assert!(format!("{error:#}").contains("repository와 함께"), "{error:#}");
}

/// core's and the registry's CI check out their own repository into `core/`
/// or `registry/`, beside the other repositories in `plugins/<name>`.
#[test]
fn the_own_repository_is_checked_out_into_a_path() {
    let parsed = load(&checkout_of("          path: core")).unwrap();
    assert_eq!(parsed.jobs[0].steps[2].with["path"], "core");
    for path in ["../core", "/tmp/core"] {
        let error = load(&checkout_of(&format!("          path: {path}"))).unwrap_err();
        assert!(format!("{error:#}").contains("GITHUB_WORKSPACE 안의 상대 경로"), "{path}: {error:#}");
    }
}

/// A `ref` expression is read like every other template: a context with no
/// local value fails validation.
#[test]
fn a_checkout_ref_without_a_local_value_fails_validation() {
    let error =
        load(&checkout_of("          repository: soksak-app/core\n          ref: ${{ github.actor }}")).unwrap_err();
    assert!(format!("{error:#}").contains("Unsupported workflow context: github.actor"), "{error:#}");
}

/// A repository is `owner/name`; an expression has no value to look up in
/// `machine.json` before the run.
#[test]
fn a_checkout_repository_must_be_owner_and_name() {
    for repository in ["core", "soksak-app/core/extra", "${{ steps.version.outputs.tag }}/core"] {
        let error = load(&checkout_of(&format!("          repository: {repository}"))).unwrap_err();
        assert!(format!("{error:#}").contains("owner/name"), "{repository}: {error:#}");
    }
}

/// A tag push reads `github.ref_name` in a checkout's `ref`, in `run`, `env`
/// and `if`; validation accepts the four `github` values a replay has.
#[test]
fn the_github_values_of_a_push_pass_validation() {
    let (_directory, path) = write(
        r#"# build-machine: skip build reason=fixture
# build-machine: skip smoke reason=fixture
name: release
on:
  push:
    tags: ['v*']
jobs:
  release:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - uses: actions/checkout@v4
        with:
          repository: soksak-app/core
          ref: ${{ github.ref_name }}
          path: core
      - name: make check
        if: github.event_name == 'push' && github.ref != ''
        env:
          SHA: ${{ github.sha }}
        run: echo ${{ github.ref_name }} && make check
"#,
    );
    let parsed = workflow::load(&path, "push", None).unwrap();
    assert_eq!(parsed.jobs[0].steps[1].with["ref"], "${{ github.ref_name }}");
}

/// Only `event_name`, `sha`, `ref`, `ref_name` and `event.<path>` have a
/// value; any other `github` name fails validation as before.
#[test]
fn any_other_github_value_still_fails_validation() {
    for name in ["actor", "repository", "run_id", "event"] {
        let error = load(&checkout_of(&format!("          repository: soksak-app/core\n          ref: ${{{{ github.{name} }}}}")))
            .unwrap_err();
        assert!(format!("{error:#}").contains(&format!("Unsupported workflow context: github.{name}")), "{name}: {error:#}");
    }
}

/// The registry's publish job deploys `site` to GitHub Pages in the
/// `github-pages` environment.
fn pages(environment: &str, deploy: &str) -> String {
    format!(
        r#"# build-machine: skip test reason=fixture
# build-machine: skip smoke reason=fixture
name: publish
on:
  push:
    tags: ['v*']
permissions:
  contents: read
  pages: write
  id-token: write
jobs:
  publish:
    runs-on: macos-15
{environment}
    steps:
      - uses: actions/checkout@v4
      - name: Build the index
        run: mkdir site && cp index.json site/index.json
      - uses: actions/upload-pages-artifact@v3
        with:
          path: site
      - uses: actions/deploy-pages@v4
{deploy}
"#
    )
}

#[test]
fn a_pages_deployment_and_its_environment_pass_validation() {
    for environment in ["    environment: github-pages", "    environment:\n      name: github-pages\n      url: https://soksak.app"] {
        let (_directory, path) = write(&pages(environment, ""));
        let parsed = workflow::load(&path, "push", None).unwrap();
        let job = serde_json::to_value(&parsed.jobs[0]).unwrap();
        assert_eq!(job["environment"]["name"], "github-pages", "{job:#}");
        let adapters: Vec<&str> = parsed.jobs[0].steps.iter().map(|step| step.adapter.as_str()).collect();
        assert_eq!(adapters, ["checkout", "run", "pages-artifact", "deploy-pages"]);
    }
}

/// The environment is recorded by name, so it has to be one; an expression
/// has no value before the job runs.
#[test]
fn an_environment_without_a_literal_name_fails_validation() {
    for (environment, expected) in [
        ("    environment:\n      url: https://soksak.app", "environment의 name"),
        ("    environment: ${{ steps.a.outputs.name }}", "environment의 name"),
        ("    environment:\n      name: github-pages\n      url: ${{ steps.deployment.outputs.page_url }}", "environment의 url"),
        ("    environment:\n      name: github-pages\n      deployment: false", "deployment"),
    ] {
        let (_directory, path) = write(&pages(environment, ""));
        let error = workflow::load(&path, "push", None).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{environment}: {error:#}");
    }
}

/// `preview` deploys somewhere else; an input the stand-in does not honour
/// fails validation.
#[test]
fn a_deploy_pages_input_it_does_not_honour_fails_validation() {
    let (_directory, path) = write(&pages("    environment: github-pages", "        with:\n          preview: true"));
    let error = workflow::load(&path, "push", None).unwrap_err();
    assert!(format!("{error:#}").contains("'preview'"), "{error:#}");
}

/// The release stand-in records `files` and `body_path` under the tag; an
/// input that would publish another release fails validation.
#[test]
fn a_release_input_it_does_not_honour_fails_validation() {
    let release = |with: &str| {
        format!(
            "# build-machine: skip build reason=fixture\n# build-machine: skip test reason=fixture\n# build-machine: skip smoke reason=fixture\nname: release\non:\n  push:\n    tags: ['v*']\njobs:\n  release:\n    runs-on: macos-15\n    steps:\n      - uses: actions/checkout@v4\n      - uses: softprops/action-gh-release@v2\n        with:\n{with}\n"
        )
    };
    let (_directory, path) = write(&release("          files: dist/*\n          body_path: notes.md"));
    workflow::load(&path, "push", None).unwrap();
    let (_directory, path) = write(&release("          files: dist/*\n          draft: true"));
    let error = workflow::load(&path, "push", None).unwrap_err();
    assert!(format!("{error:#}").contains("'draft'"), "{error:#}");
}
