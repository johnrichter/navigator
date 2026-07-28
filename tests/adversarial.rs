//! Supplementary adversarial coverage beyond `tests/cli.rs`: an explicit
//! pre-v2 sentinel version, `OR`/`NOT` facetquery grouping, and a
//! zero-file scan -- none of these paths are exercised by `cli.rs`.

use std::path::Path;
use std::process::{Command, Output};

fn navigator_bin() -> &'static str {
    env!("CARGO_BIN_EXE_navigator")
}

fn run(repo: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(navigator_bin())
        .args(["--root"])
        .arg(repo)
        .args(args)
        .env("HOME", home)
        .output()
        .expect("navigator binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8")
}

fn write(repo: &Path, rel: &str, contents: &str) {
    let path = repo.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// `sentinel_version = 1` is a pre-v2 sentinel format this build never
/// shipped support for -- it must fail closed like any other unsupported
/// version, not be silently accepted as "close enough" to 2.
#[test]
fn sentinel_version_1_is_rejected_as_unsupported_not_silently_upgraded() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "navigator.toml", "sentinel_version = 1\nextensions = []\n");
    write(dir.path(), "docs/a.md", "plain body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(30), "sentinel_version 1 must fail closed: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "precondition_unmet");
}

/// `OR` and `NOT` are as load-bearing to the facet/boolean grammar as
/// `AND` -- a query composing all three must resolve to the exact set the
/// boolean logic says it should, no more, no fewer.
#[test]
fn find_or_and_not_grammar_composes_correctly() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = \"default@1.0.0\"\n",
    );
    write(
        dir.path(),
        "a.md",
        "---\nname: a\nid: a\ndescription: skill a\ntags: [type:skill, status:complete]\nlinks: []\nupdated: 2026-01-01\n---\nbody\n",
    );
    write(
        dir.path(),
        "b.md",
        "---\nname: b\nid: b\ndescription: report b\ntags: [type:report, status:complete]\nlinks: []\nupdated: 2026-01-01\n---\nbody\n",
    );
    write(
        dir.path(),
        "c.md",
        "---\nname: c\nid: c\ndescription: skill c in draft\ntags: [type:skill, status:draft]\nlinks: []\nupdated: 2026-01-01\n---\nbody\n",
    );
    // (type:skill OR type:report) AND NOT status:draft -> a.md, b.md; excludes c.md.
    let out = run(
        dir.path(),
        home.path(),
        &["--json", "find", "(type:skill OR type:report) AND NOT status:draft"],
    );
    assert!(out.status.success(), "query must parse and execute: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let mut paths: Vec<&str> = json["data"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["path"].as_str().unwrap())
        .collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["a.md", "b.md"], "OR/NOT grouping resolved wrong set: {json}");
}

/// A repo with no scannable files at all (every path excluded, or the
/// tree is empty) is a legitimate zero-match run, not a crash or a
/// failure -- an empty corpus is not an error condition anywhere in this
/// CLI's contract.
#[test]
fn empty_repo_scans_to_zero_files_not_a_crash() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = []\n",
    );
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(0), "empty repo must succeed with zero scanned: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["data"]["scanned"], 0);
    assert_eq!(json["data"]["violations"], 0);
}
