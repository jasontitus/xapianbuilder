//! Publication races and ownership guarantees of the public library API.

use std::path::Path;
use xapianbuilder::{AccentRule, Builder, BuilderOptions, Mode};

fn builder(output: &Path) -> anyhow::Result<Builder> {
    Builder::new(
        output,
        BuilderOptions {
            language: "eng",
            stopwords: "",
            stemmer: "",
            accent_rule: AccentRule::Libzim,
            keep_termlists: false,
            mode: Mode::Title,
        },
    )
}

#[test]
fn competing_builders_never_replace_the_winner() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("index");
    let first = builder(&output).unwrap();
    let second = builder(&output).unwrap();
    first.add_title("first", "First", "", "").unwrap();
    second.add_title("second", "Second", "", "").unwrap();
    assert!(!output.exists(), "unfinalized database was exposed");
    first.finalize().unwrap();
    let published = std::fs::read(&output).unwrap();
    assert!(second.finalize().is_err());
    assert_eq!(std::fs::read(&output).unwrap(), published);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "workspace leaked"
    );
}

#[test]
fn caller_directory_and_successful_output_survive_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join("important");
    std::fs::write(&sentinel, "user data").unwrap();
    let output = dir.path().join("index");
    let index = builder(&output).unwrap();
    index.add_title("entry", "Entry", "", "").unwrap();
    index.finalize().unwrap();
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"user data");
    let inspected = std::process::Command::new("xapian-delve")
        .arg(&output)
        .output()
        .expect("xapian-delve required");
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    assert!(String::from_utf8(inspected.stdout)
        .unwrap()
        .lines()
        .any(|line| line == "number of documents = 1"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn dropping_unfinished_builder_removes_only_its_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("index");
    let index = builder(&output).unwrap();
    index.add_title("entry", "Entry", "", "").unwrap();
    drop(index);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn dangling_symlinks_are_never_followed_or_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("index");
    let victim = dir.path().join("victim");
    std::os::unix::fs::symlink(&victim, &output).unwrap();
    assert!(builder(&output).is_err());
    assert_eq!(std::fs::read_link(&output).unwrap(), victim);
    assert!(!victim.exists());

    std::fs::remove_file(&output).unwrap();
    let index = builder(&output).unwrap();
    index.add_title("entry", "Entry", "", "").unwrap();
    // A destination created after the constructor must be protected too.
    std::os::unix::fs::symlink(&victim, &output).unwrap();
    assert!(index.finalize().is_err());
    assert_eq!(std::fs::read_link(&output).unwrap(), victim);
    assert!(!victim.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn failed_construction_cleans_up_private_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let result = Builder::new(
        &dir.path().join("index"),
        BuilderOptions {
            language: "invalid\0language",
            stopwords: "",
            stemmer: "",
            accent_rule: AccentRule::Libzim,
            keep_termlists: false,
            mode: Mode::Title,
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
