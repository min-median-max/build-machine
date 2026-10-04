//! `${{ }}` expressions and step outputs, read as GitHub reads them.
//!
//! orm's CI computes the lowest PHP release in a step with `id: php-min`,
//! which writes `version=…` to `$GITHUB_OUTPUT`, and the next step installs
//! `${{ steps.php-min.outputs.version }}`. The value exists only once that
//! step has run; validation checks the reference, the run supplies the value.

use build_machine_core::workflow::{self, Condition, JobStatus, Outputs, Template};

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
    assert_eq!(template.render(&outputs(&[("php-min", "version", "8.4")])), "php8.4-cli");
    // An output the step did not write is empty, as on GitHub.
    assert_eq!(template.render(&Outputs::new()), "php-cli");
    // || yields the first truthy operand, not a boolean.
    let fallback = Template::parse("${{ steps.a.outputs.v || 'none' }}").unwrap();
    assert_eq!(fallback.render(&Outputs::new()), "none");
    assert_eq!(fallback.render(&outputs(&[("a", "v", "x")])), "x");
    assert_eq!(Template::parse("no expression").unwrap().render(&Outputs::new()), "no expression");
    for invalid in ["${{ github.ref }}", "${{ steps.a.outputs }}", "${{ hashFiles('x') }}", "${{ steps.a.outputs.v", "${{ 1 < 2 }}"] {
        assert!(Template::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_condition_compares_step_outputs_as_github_compares_values() {
    let runs = |text: &str, values: &Outputs| Condition::parse(Some(text)).unwrap().runs(JobStatus::Success, values);
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
    assert!(!Condition::parse(Some("steps.php-min.outputs.version")).unwrap().runs(JobStatus::Failure, &php));
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
        ("      - run: echo ${{ github.ref }} && make check\n", "github"),
        ("      - run: make check\n        env:\n          REF: ${{ github.ref_name }}\n", "github"),
        ("      - run: make check\n        working-directory: ${{ runner.temp }}\n", "runner"),
    ] {
        let error = load(steps).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{steps}: {error:#}");
    }
}
