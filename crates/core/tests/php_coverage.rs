//! setup-php's `coverage` input. orm's perf gate refused to measure on the
//! replay's PHP ("this PHP loads xdebug, pcov") although its setup-php step
//! says `coverage: none`, which disables both on a runner.

use build_machine_core::workflow::{self, PhpCoverage};

#[test]
fn coverage_values_are_read_as_setup_php_reads_them() {
    assert_eq!(PhpCoverage::parse("none").unwrap(), PhpCoverage::None);
    assert_eq!(PhpCoverage::parse("Xdebug").unwrap(), PhpCoverage::Xdebug);
    assert_eq!(PhpCoverage::parse("xdebug3").unwrap(), PhpCoverage::Xdebug);
    assert_eq!(PhpCoverage::parse("pcov").unwrap(), PhpCoverage::Pcov);
    for refused in ["xdebug2", "phpdbg", "true", ""] {
        assert!(PhpCoverage::parse(refused).is_err(), "{refused}");
    }
}

#[test]
fn an_unknown_coverage_fails_validation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ci.yml");
    let workflow = |coverage: &str| {
        format!("# build-machine: skip build reason=f\n# build-machine: skip smoke reason=f\nname: ci\non: push\njobs:\n  test:\n    runs-on: ubuntu-26.04-arm\n    steps:\n      - uses: shivammathur/setup-php@v2\n        with:\n          php-version: '8.5'\n          coverage: {coverage}\n      - run: make check\n")
    };
    std::fs::write(&path, workflow("none")).unwrap();
    workflow::load(&path, "push", None).unwrap();
    std::fs::write(&path, workflow("phpdbg")).unwrap();
    let error = workflow::load(&path, "push", None).unwrap_err();
    assert!(format!("{error:#}").contains("phpdbg"), "{error:#}");
}
