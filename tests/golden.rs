//! CLI regressions inspected with xapian-delve. The tool is required: missing or
//! failed inspection must never turn a correctness check into a passing test.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xapianbuilder"))
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny.jsonl")
}

fn build(mode: &str, input: &Path, output: &Path, options: &[&str]) -> Output {
    Command::new(bin())
        .args([mode, "--input"])
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(options)
        .output()
        .expect("launch xapianbuilder")
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn delve(db: &Path, args: &[&str]) -> String {
    success(
        Command::new("xapian-delve")
            .args(args)
            .arg(db)
            .output()
            .expect("install xapian-tools (Linux) or xapian (Homebrew) to run integration tests"),
    )
}

fn doc_count(db: &Path) -> u32 {
    let listing = delve(db, &[]);
    listing
        .lines()
        .find_map(|line| line.strip_prefix("number of documents = "))
        .expect("missing document count in xapian-delve output")
        .parse()
        .unwrap()
}

fn terms(db: &Path, id: &str) -> Vec<String> {
    delve(db, &["-1", "-r", id])
        .lines()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

fn write_doc(path: &Path, body: &str) {
    std::fs::write(
        path,
        serde_json::json!({
            "path": "a", "title": "x", "mimetype": "text/html", "body": body
        })
        .to_string()
            + "\n",
    )
    .unwrap();
}

#[test]
fn fulltext_build_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("fulltext.xapian");
    success(build("fulltext", &fixture(), &out, &["--language", "eng"]));
    assert_eq!(doc_count(&out), 2); // Apollo_13 is NOINDEX.
}

#[test]
fn title_build_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("title.xapian");
    success(build("title", &fixture(), &out, &["--language", "eng"]));
    assert_eq!(doc_count(&out), 3); // NOINDEX does not hide entry titles.
}

#[test]
fn refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("existing");
    std::fs::write(&out, "unrelated user data").unwrap();
    assert!(!build("fulltext", &fixture(), &out, &[]).status.success());
    assert_eq!(std::fs::read(&out).unwrap(), b"unrelated user data");
}

#[test]
fn skip_if_empty_omits_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("empty.jsonl");
    std::fs::write(&input, "").unwrap();
    let out = dir.path().join("out");
    success(build("fulltext", &input, &out, &["--skip-if-empty"]));
    assert!(!out.exists());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "workspace leaked"
    );
}

#[test]
fn per_doc_language_overrides_cli() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("mixed.jsonl");
    let docs = ["porter", "english"]
        .map(|language| {
            serde_json::json!({
                "path": language, "title": "x", "mimetype": "text/html",
                "language": language, "body": "<body>dying universities</body>"
            })
            .to_string()
        })
        .join("\n");
    std::fs::write(&input, docs).unwrap();
    let out = dir.path().join("out");
    success(build(
        "fulltext",
        &input,
        &out,
        &["--language", "deu", "--keep-termlists"],
    ));
    let porter = terms(&out, "1");
    let english = terms(&out, "2");
    assert!(porter.iter().any(|term| term == "dy"));
    assert!(!porter.iter().any(|term| term == "die"));
    assert!(english.iter().any(|term| term == "die"));
}

#[test]
fn fulltext_title_is_unaccented_like_libzim() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("accents.jsonl");
    std::fs::write(
        &input,
        serde_json::json!({"path":"a", "title":"Café Été",
        "mimetype":"text/html", "body":"<body>body text here</body>"})
        .to_string(),
    )
    .unwrap();
    let out = dir.path().join("out");
    success(build(
        "fulltext",
        &input,
        &out,
        &["--language", "eng", "--keep-termlists"],
    ));
    let listing = delve(&out, &["-1", "-d", "-V", "-r", "1"]);
    assert!(listing.contains("cafe ete"), "{listing}");
    assert!(!listing.contains("café"), "{listing}");
}

#[test]
fn accent_rule_latin_preserves_indic_marks() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("indic.jsonl");
    write_doc(&input, "<body>कितना अच्छा</body>");
    for (rule, preserved) in [("libzim", false), ("latin", true)] {
        let out = dir.path().join(rule);
        success(build(
            "fulltext",
            &input,
            &out,
            &["--accent-rule", rule, "--keep-termlists"],
        ));
        assert_eq!(
            terms(&out, "1").iter().any(|t| t.contains('\u{093f}')),
            preserved
        );
    }
}

#[test]
fn stemmer_override_changes_terms() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("stems.jsonl");
    write_doc(&input, "<body>dying</body>");
    for (stemmer, expected) in [("english", "die"), ("porter", "dy"), ("none", "dying")] {
        let out = dir.path().join(stemmer);
        success(build(
            "fulltext",
            &input,
            &out,
            &[
                "--language",
                "eng",
                "--stemmer",
                stemmer,
                "--keep-termlists",
            ],
        ));
        let listing = terms(&out, "1");
        assert!(
            listing.iter().any(|term| term == expected),
            "{stemmer}: {listing:?}"
        );
        for other in ["die", "dy", "dying"]
            .into_iter()
            .filter(|term| *term != expected)
        {
            assert!(
                !listing.iter().any(|term| term == other),
                "unexpected {other}: {listing:?}"
            );
        }
    }
}

#[test]
fn cjk_ngrams_produce_bigram_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cjk.jsonl");
    write_doc(&input, "<body>中国语言研究</body>");
    let out = dir.path().join("out");
    success(build("fulltext", &input, &out, &["--keep-termlists"]));
    let listing = terms(&out, "1");
    for bigram in ["中国", "国语", "语言", "言研", "研究"] {
        assert!(
            listing.iter().any(|term| term == bigram),
            "missing {bigram}: {listing:?}"
        );
    }
}

#[test]
fn thai_marks_preserved_under_latin_rule() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("thai.jsonl");
    write_doc(&input, "<body>ความรัก</body>");
    for (rule, preserved) in [("libzim", false), ("latin", true)] {
        let out = dir.path().join(rule);
        success(build(
            "fulltext",
            &input,
            &out,
            &["--accent-rule", rule, "--keep-termlists"],
        ));
        assert_eq!(
            terms(&out, "1")
                .iter()
                .any(|term| term.contains('\u{0e31}')),
            preserved
        );
    }
}

#[test]
fn leaves_unowned_scratch_directory_untouched_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("out.tmp");
    std::fs::create_dir(&existing).unwrap();
    let sentinel = existing.join("important.txt");
    std::fs::write(&sentinel, "unrelated").unwrap();
    assert!(!build(
        "title",
        &dir.path().join("missing"),
        &dir.path().join("out"),
        &[]
    )
    .status
    .success());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unrelated");
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "failed build leaked workspace"
    );
}

#[test]
fn bad_geo_and_nul_entry_do_not_destroy_good_documents() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("mixed.jsonl");
    let docs = [
        serde_json::json!({"path":"good", "title":"good", "mimetype":"text/html", "body":"<body>ordinary</body>"}),
        serde_json::json!({"path":"badgeo", "title":"badgeo", "mimetype":"text/html", "body":"<meta name=\"geo.position\" content=\"91;0\"><body>searchable</body>"}),
        serde_json::json!({"path":"bad\u{0000}path", "title":"bad", "body":"text"}),
        serde_json::json!({"path":"noindex", "title":"noindex", "mimetype":"text/html", "body":"<meta name=\"robots\" content=\"noindex\"><body>hidden</body>"}),
    ];
    std::fs::write(
        &input,
        docs.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let out = dir.path().join("out");
    let result = build("fulltext", &input, &out, &["--keep-termlists"]);
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    success(result);
    assert_eq!(doc_count(&out), 2);
    assert!(terms(&out, "2").iter().any(|term| term == "searchable"));
    let reported_count: u32 = stderr
        .lines()
        .find_map(|line| line.strip_prefix("done: "))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(reported_count, doc_count(&out));
}

#[test]
fn malformed_json_never_publishes_partial_index() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("malformed.jsonl");
    let mut corpus = "{\"path\":\"a\",\"title\":\"hello\"}\n".repeat(300);
    corpus.push_str("not JSON\n");
    std::fs::write(&input, corpus).unwrap();
    let out = dir.path().join("out");
    assert!(!build("title", &input, &out, &[]).status.success());
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn ordering_survives_parallel_chunk_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("corpus.jsonl");
    let corpus = (0..513)
        .map(|n| {
            serde_json::json!({"path":format!("entry{n}"), "title":format!("Title {n}")})
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&input, corpus).unwrap();
    let single = dir.path().join("single");
    let parallel = dir.path().join("parallel");
    success(build(
        "title",
        &input,
        &single,
        &["--jobs", "1", "--keep-termlists"],
    ));
    success(build(
        "title",
        &input,
        &parallel,
        &["--jobs", "4", "--keep-termlists"],
    ));
    assert_eq!(doc_count(&single), 513);
    assert_eq!(doc_count(&parallel), 513);
    for id in ["1", "256", "257", "512", "513"] {
        let args = ["-1", "-d", "-V", "-r", id];
        assert_eq!(delve(&single, &args), delve(&parallel, &args));
    }
}
