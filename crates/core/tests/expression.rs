//! `${{ }}` expressions and step outputs, read as GitHub reads them.
//!
//! orm's CI computes the lowest PHP release in a step with `id: php-min`,
//! which writes `version=…` to `$GITHUB_OUTPUT`, and the next step installs
//! `${{ steps.php-min.outputs.version }}`. The value exists only once that
//! step has run; validation checks the reference, the run supplies the value.

use build_machine_core::workflow::{self, Condition, Github, JobStatus, Outputs, Template};

fn outputs(entries: &[(&str, &str, &str)]) -> Outputs {
    let mut outputs = Outputs::new();
    for (step, name, value) in entries {
        outputs.entry((*step).to_owned()).or_default().insert((*name).to_owned(), (*value).to_owned());
    }
    outputs
}

#[test]
fn a_template_takes_step_outputs_when_it_is_rendered() {
    let template = Template::parse("php${{ steps.php-min.outputs.version }}-cli").unwrap();
    assert_eq!(template.render(&outputs(&[("php-min", "version", "8.4")]), &Github::default()), "php8.4-cli");
    // An output the step did not write is empty, as on GitHub.
    assert_eq!(template.render(&Outputs::new(), &Github::default()), "php-cli");
    // || yields the first truthy operand, not a boolean.
    let fallback = Template::parse("${{ steps.a.outputs.v || 'none' }}").unwrap();
    assert_eq!(fallback.render(&Outputs::new(), &Github::default()), "none");
    assert_eq!(fallback.render(&outputs(&[("a", "v", "x")]), &Github::default()), "x");
    assert_eq!(Template::parse("no expression").unwrap().render(&Outputs::new(), &Github::default()), "no expression");
    for invalid in ["${{ github.actor }}", "${{ steps.a.outputs }}", "${{ hashFiles('x') }}", "${{ steps.a.outputs.v", "${{ 1 < 2 }}"] {
        assert!(Template::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_condition_compares_step_outputs_as_github_compares_values() {
    let runs = |text: &str, values: &Outputs| Condition::parse(Some(text)).unwrap().runs(JobStatus::Success, values, &Github::default());
    let php = outputs(&[("php-min", "version", "8.4"), ("flags", "on", "true")]);
    assert!(runs("steps.php-min.outputs.version == '8.4'", &php));
    assert!(runs("${{ steps.php-min.outputs.version != '8.5' }}", &php));
    // Strings compare without regard to case.
    assert!(runs("steps.flags.outputs.on == 'TRUE'", &php));
    // Different types compare as numbers.
    assert!(runs("steps.php-min.outputs.version == 8.4", &php));
    // An empty output is falsy; a non-empty one is truthy.
    assert!(!runs("steps.php-min.outputs.missing", &php));
    assert!(runs("steps.php-min.outputs.version", &php));
    // A condition without a status function is still success() && (…).
    assert!(!Condition::parse(Some("steps.php-min.outputs.version")).unwrap().runs(JobStatus::Failure, &php, &Github::default()));
}

fn load(steps: &str) -> anyhow::Result<workflow::Workflow> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ci.yml");
    std::fs::write(
        &path,
        format!("# build-machine: skip build reason=fixture\n# build-machine: skip smoke reason=fixture\nname: ci\non: workflow_dispatch\njobs:\n  test:\n    runs-on: ubuntu-26.04-arm\n    steps:\n{steps}"),
    )
    .unwrap();
    workflow::load(&path, "workflow_dispatch", None)
}

#[test]
fn validation_checks_the_reference_and_not_the_value() {
    load(
        r#"      - id: php-min
        run: echo "version=8.4" >> "$GITHUB_OUTPUT"
      - uses: shivammathur/setup-php@v2
        with:
          php-version: ${{ steps.php-min.outputs.version }}
      - name: make check
        if: steps.php-min.outputs.version != ''
        env:
          PHP_MIN: ${{ steps.php-min.outputs.version }}
        run: echo "${{ steps.php-min.outputs.version }}"
"#,
    )
    .unwrap();

    for (steps, expected) in [
        // A step that does not exist.
        ("      - uses: shivammathur/setup-php@v2\n        with:\n          php-version: ${{ steps.nothing.outputs.version }}\n      - run: make check\n", "nothing"),
        // A step that runs later.
        ("      - run: echo ${{ steps.later.outputs.v }} && make check\n      - id: later\n        run: echo v=1 >> \"$GITHUB_OUTPUT\"\n", "later"),
        // A context with no local value, in run, env and working-directory.
        ("      - run: echo ${{ github.actor }} && make check\n", "github.actor"),
        ("      - run: make check\n        env:\n          REPOSITORY: ${{ github.repository }}\n", "github.repository"),
        ("      - run: make check\n        working-directory: ${{ runner.temp }}\n", "runner"),
    ] {
        let error = load(steps).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{steps}: {error:#}");
    }
}

/// A push of tag `v0.0.2` carries these four values; the replay gives them
/// from its event, its revision and the ref it checks out.
fn tag_push() -> Github {
    Github { event_name: "push".to_owned(), sha: "3c5863aefb8a161832542faa9b0d29af65a40fb8".to_owned(), reference: Some("refs/tags/v0.0.2".to_owned()) }
}

#[test]
fn the_github_context_takes_the_replay_s_values() {
    let github = tag_push();
    let template = Template::parse("${{ github.event_name }} ${{ github.sha }} ${{ github.ref }} ${{ github.ref_name }}").unwrap();
    assert_eq!(
        template.render(&Outputs::new(), &github),
        "push 3c5863aefb8a161832542faa9b0d29af65a40fb8 refs/tags/v0.0.2 v0.0.2"
    );
    let branch = Github { reference: Some("refs/heads/main".to_owned()), ..tag_push() };
    assert_eq!(Template::parse("${{ github.ref_name }}").unwrap().render(&Outputs::new(), &branch), "main");
    let runs = |text: &str| Condition::parse(Some(text)).unwrap().runs(JobStatus::Success, &Outputs::new(), &github);
    assert!(runs("github.ref_name == 'v0.0.2'"));
    assert!(runs("${{ github.ref == 'refs/tags/v0.0.2' && github.event_name == 'push' }}"));
    assert!(!runs("github.ref_name == 'main'"));
}

/// The soksak releases check out `soksak-app/core` at `${{ github.ref_name }}`.
#[test]
fn a_step_s_inputs_env_and_command_take_the_github_context() {
    let parsed = load(
        r#"      - uses: actions/checkout@v4
        with:
          repository: soksak-app/core
          ref: ${{ github.ref_name }}
          path: core
      - name: make check
        if: github.event_name != 'pull_request'
        env:
          REF: ${{ github.ref }}
        working-directory: ${{ github.ref_name }}
        run: echo ${{ github.sha }} && make check
"#,
    )
    .unwrap();
    let github = tag_push();
    let steps = &parsed.jobs[0].steps;
    assert_eq!(steps[0].resolve(&Outputs::new(), &github).unwrap().with["ref"], "v0.0.2");
    let check = steps[1].resolve(&Outputs::new(), &github).unwrap();
    assert_eq!(check.env["REF"], "refs/tags/v0.0.2");
    assert_eq!(check.working_directory.as_deref(), Some("v0.0.2"));
    assert_eq!(check.run.as_deref(), Some("echo 3c5863aefb8a161832542faa9b0d29af65a40fb8 && make check"));
}

/// A replay of a commit that no branch or tag names has no `github.ref`; a
/// workflow that reads it is refused, and one that does not is replayed.
#[test]
fn a_replay_without_a_ref_name_refuses_a_workflow_that_reads_it() {
    let reads = load("      - run: echo ${{ github.ref_name }} && make check\n").unwrap();
    let error = workflow::github_context(&reads.jobs, "push", "3c5863a", None).unwrap_err();
    assert!(format!("{error:#}").contains("branch나 tag"), "{error:#}");
    let context = workflow::github_context(&reads.jobs, "push", "3c5863a", Some("refs/tags/v0.0.2")).unwrap();
    assert_eq!(context, tag_push_with_sha("3c5863a"));
    let other = load("      - run: echo ${{ github.sha }} && make check\n").unwrap();
    assert_eq!(workflow::github_context(&other.jobs, "push", "3c5863a", None).unwrap().reference, None);
}

fn tag_push_with_sha(sha: &str) -> Github {
    Github { sha: sha.to_owned(), ..tag_push() }
}
