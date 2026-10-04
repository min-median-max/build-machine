//! `softprops/action-gh-release`: the replay publishes nothing and records
//! the release the action would have made, with its assets' sizes and SHA-256.

use build_machine_worker::release::publish;
use std::collections::BTreeMap;
use std::path::Path;

fn inputs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn workspace(root: &Path) {
    std::fs::create_dir_all(root.join("dist/nested")).unwrap();
    std::fs::write(root.join("dist/a.tgz"), "abc").unwrap();
    std::fs::write(root.join("dist/b.tgz"), "{}").unwrap();
    std::fs::write(root.join("dist/.hidden"), "x").unwrap();
    std::fs::write(root.join("dist/nested/c.tgz"), "x").unwrap();
    std::fs::write(root.join("notes.md"), "notes").unwrap();
}

/// Each line of `files` is a pattern; `*` matches within one path segment and
/// not a leading dot, as the action's glob does. Folders are not assets.
#[test]
fn the_release_records_the_files_its_patterns_match() {
    let directory = tempfile::tempdir().unwrap();
    workspace(directory.path());
    let release = publish(
        directory.path(),
        &inputs(&[("files", "dist/*\n"), ("body_path", "notes.md")]),
        "refs/tags/v0.0.3",
    )
    .unwrap();
    assert_eq!(release.tag, "v0.0.3");
    let listed: Vec<(&str, u64, &str)> =
        release.assets.iter().map(|asset| (asset.path.as_str(), asset.size, asset.sha256.as_str())).collect();
    assert_eq!(
        listed,
        [
            ("a.tgz", 3, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            ("b.tgz", 2, "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"),
        ]
    );
    assert!(release.unmatched.is_empty());
    assert_eq!(release.body.unwrap().size, 5);
}

/// The action warns about a pattern that matches no file and publishes the
/// rest; the replay names the pattern.
#[test]
fn a_pattern_that_matches_nothing_is_named() {
    let directory = tempfile::tempdir().unwrap();
    workspace(directory.path());
    let release = publish(directory.path(), &inputs(&[("files", "dist/a.tgz\nout/*")]), "refs/tags/v1").unwrap();
    assert_eq!(release.assets.len(), 1);
    assert_eq!(release.unmatched, ["out/*"]);
}

/// A release needs a tag: the action fails on a branch without `tag_name`.
#[test]
fn a_release_of_a_branch_fails() {
    let directory = tempfile::tempdir().unwrap();
    workspace(directory.path());
    let error = publish(directory.path(), &inputs(&[("files", "dist/*")]), "refs/heads/main").unwrap_err();
    assert!(error.to_string().contains("tag"), "{error}");
}

/// A pattern form the replay does not match is refused rather than read
/// differently, and so is a body file that does not exist.
#[test]
fn an_unsupported_pattern_and_a_missing_body_fail() {
    let directory = tempfile::tempdir().unwrap();
    workspace(directory.path());
    for pattern in ["dist/**", "dist/{a,b}.tgz", "dist/[ab].tgz", "../dist/*", "/tmp/*"] {
        let error = publish(directory.path(), &inputs(&[("files", pattern)]), "refs/tags/v1").unwrap_err();
        assert!(error.to_string().contains(pattern), "{pattern}: {error}");
    }
    let error = publish(directory.path(), &inputs(&[("files", "dist/*"), ("body_path", "missing.md")]), "refs/tags/v1")
        .unwrap_err();
    assert!(error.to_string().contains("missing.md"), "{error}");
}
