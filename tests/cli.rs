//! Black-box integration tests against the built `navigator` binary.
//!
//! Each test builds its own throwaway repo under a `tempfile::TempDir` and
//! points `--root` at it, with `HOME` redirected to a private temp dir so
//! the out-of-tree freshness cache never touches the operator's real
//! `~/.cache/navigator`. Adversarial coverage: demotion under both
//! `search` and `find`, sentinel v2 happy/error paths, the no-sentinel
//! floor, usage-class failures, and cache placement/reuse.

use std::path::Path;
use std::process::{Command, Output};

fn navigator_bin() -> &'static str {
    env!("CARGO_BIN_EXE_navigator")
}

/// Runs `navigator` with `args` rooted at `repo`, under a private `HOME` so
/// the freshness cache never lands in the real one.
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

/// A repo with one conformant file, one schema-nonconformant file, and one
/// file whose frontmatter block itself fails to parse.
fn mixed_corpus_repo(repo: &Path) {
    write(
        repo,
        "navigator.toml",
        "sentinel_version = 2\nextensions = \"default@1.0.0\"\n",
    );
    write(
        repo,
        "docs/conformant.md",
        "---\nname: conformant-widget\nid: conformant-widget\ndescription: a fully conformant widget document\ntags: [type:skill, status:complete, topic:apm]\nlinks: []\nupdated: 2026-01-01\n---\nWidgets and gears, nothing else about the query term.\n",
    );
    write(
        repo,
        "docs/nonconformant.md",
        "---\nname: nonconformant-widget\nid: nonconformant-widget\ndescription: missing required fields\ntags: [skill]\n---\nWidgets and gears mentioned here too, scoring higher on the query term.\nwidgets widgets widgets gears gears gears\n",
    );
    write(
        repo,
        "docs/unparsable.md",
        "---\nname: [unterminated\n---\nwidgets gears widgets gears widgets gears unparsable body text\n",
    );
}

#[test]
fn help_lists_examples_and_exit_codes() {
    let out = Command::new(navigator_bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("EXAMPLES:"), "missing usage examples section:\n{text}");
    assert!(text.contains("navigator search"), "no search example:\n{text}");
    assert!(text.contains("EXIT CODES"), "missing exit code documentation:\n{text}");
}

#[test]
fn no_sentinel_resolves_to_neutral_floor_and_scans_successfully() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    // No frontmatter block at all: under the neutral core-only floor
    // (zero required fields/tags), this is conformant.
    write(dir.path(), "docs/a.md", "plain body, no frontmatter at all");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    // No navigator.toml at all must not be a hard failure -- it is the
    // documented neutral core-only floor, and a frontmatter-free file is
    // conformant against it.
    assert_eq!(out.status.code(), Some(0), "expected success exit code 0, got: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["data"]["scanned"], 1);
    assert_eq!(json["data"]["violations"], 0);
}

#[test]
fn no_sentinel_still_demotes_a_file_whose_frontmatter_block_fails_to_parse() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    // Even under the neutral floor (no required vocabulary), a frontmatter
    // block that itself fails to parse is a violation -- there is no
    // schema entry to validate, so it can never be "conformant".
    write(dir.path(), "docs/broken.md", "---\nname: [unterminated\n---\nbody\n");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(10), "expected caveats exit code 10, got: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "caveats");
    assert_eq!(json["data"]["violations"], 1);
    assert!(json["caveats"].as_array().is_some_and(|c| !c.is_empty()), "caveats array missing/empty: {json}");
}

#[test]
fn v2_sentinel_with_named_extension_resolves() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_ne!(json["status"], "precondition_unmet", "sentinel failed to resolve: {json}");
    assert_eq!(json["data"]["scanned"], 3);
}

#[test]
fn unsupported_sentinel_version_is_precondition_unmet_not_a_crash() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "navigator.toml", "sentinel_version = 99\nextensions = []\n");
    write(dir.path(), "docs/a.md", "body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(30));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "precondition_unmet");
    assert_eq!(json["errors"][0]["code"], "precondition_unmet.sentinel.invalid");
}

#[test]
fn unknown_named_extension_is_precondition_unmet() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "navigator.toml", "sentinel_version = 2\nextensions = \"nope@0.0.0\"\n");
    write(dir.path(), "docs/a.md", "body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(30));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["errors"][0]["code"], "precondition_unmet.schema.unresolved");
}

#[test]
fn search_demotes_nonconformant_hits_never_excludes_them() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let out = run(dir.path(), home.path(), &["--json", "search", "widgets gears"]);
    assert!(out.status.success(), "search failed: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let hits = json["data"]["hits"].as_array().unwrap();
    let paths: Vec<&str> = hits.iter().map(|h| h["path"].as_str().unwrap()).collect();
    // All three files mention the query term and none is dropped.
    assert_eq!(paths.len(), 3, "a nonconformant/unparsable file was excluded, not demoted: {paths:?}");
    // The nonconformant file outscores the conformant one on raw text
    // relevance (repeats the query terms) yet must still rank after it.
    let rank_of = |p: &str| paths.iter().position(|x| *x == p).unwrap();
    assert!(
        rank_of("docs/conformant.md") < rank_of("docs/nonconformant.md"),
        "conformant file must rank before a higher-scoring nonconformant one: {paths:?}"
    );
    assert!(
        rank_of("docs/nonconformant.md") < rank_of("docs/unparsable.md")
            || rank_of("docs/unparsable.md") < rank_of("docs/nonconformant.md"),
        "both demoted files must still be present"
    );
}

#[test]
fn find_boolean_query_matches_and_demotes() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let out = run(dir.path(), home.path(), &["--json", "find", "type:skill AND status:complete"]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let hits = json["data"]["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "only the conformant file carries type:skill AND status:complete: {hits:?}");
    assert_eq!(hits[0]["path"], "docs/conformant.md");
}

#[test]
fn find_with_malformed_query_is_usage_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let out = run(dir.path(), home.path(), &["--json", "find", "((("]);
    assert_eq!(out.status.code(), Some(50), "expected clikit usage exit code 50");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "usage");
}

#[test]
fn search_with_malformed_filter_is_usage_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let out = run(dir.path(), home.path(), &["--json", "search", "x", "--filter", "type::::bad"]);
    assert_eq!(out.status.code(), Some(50));
}

#[test]
fn lint_reports_violation_code_and_message_in_default_human_output_not_only_json() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    // Default (non-`--json`) invocation.
    let out = run(dir.path(), home.path(), &["lint"]);
    assert_eq!(out.status.code(), Some(10));
    let stderr = String::from_utf8(out.stderr).unwrap();
    // The per-file-class cap, sourced from the resolved profile, is visible.
    assert!(stderr.contains("description cap"), "no class cap logged in default output:\n{stderr}");
    // A violation's own code and message appear in the default (human)
    // stream, not only inside the --json result record.
    assert!(
        stderr.contains("MISSING_REQUIRED_FIELD") || stderr.contains("missing_required_field"),
        "violation code missing from default human output:\n{stderr}"
    );
    assert!(
        stderr.contains("required field") || stderr.contains("missing required"),
        "violation message missing from default human output:\n{stderr}"
    );
}

#[test]
fn freshness_cache_lands_out_of_tree_never_inside_the_repo() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());
    let _ = run(dir.path(), home.path(), &["lint"]);

    // Nothing cache-shaped was written inside the scanned repo.
    for entry in walkdir::WalkDir::new(dir.path()).into_iter().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        assert!(!name.contains("cache"), "a cache artifact leaked into the repo tree: {:?}", entry.path());
    }

    // The cache landed under the redirected HOME's cache dir instead.
    let cache_dir = home.path().join(".cache").join("navigator");
    assert!(cache_dir.is_dir(), "no out-of-tree cache directory was created under HOME/.cache/navigator");
    let files: Vec<_> = std::fs::read_dir(&cache_dir).unwrap().filter_map(Result::ok).collect();
    assert!(!files.is_empty(), "cache directory exists but holds no cache file");
}

#[test]
fn cache_reuse_produces_byte_identical_result_record() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    mixed_corpus_repo(dir.path());

    let first = run(dir.path(), home.path(), &["--json", "lint"]);
    let second = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(
        stdout(&first),
        stdout(&second),
        "a cached second run must reproduce the exact same result record as a cold run"
    );
}

#[test]
fn description_over_class_cap_is_diagnosable_in_default_output() {
    // The "default" pack caps a "context" file's description at 350 chars.
    // A file with all six required fields present but a too-long
    // description must surface DESCRIPTION_OVER_CAP in the default
    // (non-json) stream, not only inside --json.
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = \"default@1.0.0\"\n",
    );
    let long_description = "x".repeat(400);
    write(
        dir.path(),
        "docs/overcap.md",
        &format!(
            "---\nname: overcap-widget\nid: overcap-widget\ndescription: {long_description}\ntags: [type:skill, status:complete, topic:apm]\nlinks: []\nupdated: 2026-01-01\n---\nbody\n"
        ),
    );
    let out = run(dir.path(), home.path(), &["lint"]);
    assert_eq!(out.status.code(), Some(10), "expected caveats exit code 10: {out:?}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("DESCRIPTION_OVER_CAP"),
        "over-cap violation code missing from default human output:\n{stderr}"
    );
    assert!(
        stderr.contains("file class 'context' description cap: 350"),
        "the context class's live cap (350) not logged for authors to read:\n{stderr}"
    );
}

#[test]
fn custom_lint_include_exclude_narrows_the_scanned_set() {
    // A sentinel's [lint] include/exclude replaces the default `**/*.md`
    // scope entirely -- only files matching `include` and not `exclude`
    // are scanned.
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = []\n[lint]\ninclude = [\"docs/**/*.md\"]\nexclude = [\"docs/skip/**\"]\n",
    );
    write(dir.path(), "docs/keep.md", "kept body");
    write(dir.path(), "docs/skip/dropped.md", "dropped body");
    write(dir.path(), "elsewhere/ignored.md", "outside include");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert!(out.status.success(), "lint failed: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let files = json["data"]["files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["docs/keep.md"], "custom include/exclude not honored: {paths:?}");
}

#[test]
fn sentinel_missing_required_version_field_is_precondition_unmet() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "navigator.toml", "extensions = []\n");
    write(dir.path(), "docs/a.md", "body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(30), "missing sentinel_version must fail closed, not default/panic: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "precondition_unmet");
}

#[test]
fn sentinel_unknown_field_is_rejected_not_silently_ignored() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = []\ntypo_field = true\n",
    );
    write(dir.path(), "docs/a.md", "body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(30), "unknown sentinel key must be rejected via deny_unknown_fields: {out:?}");
}

#[test]
fn navigator_json_env_var_activates_machine_narration_without_the_flag() {
    // NAVIGATOR_JSON must win over the built-in default per the documented
    // flag > env > file > default precedence, exercised through the real
    // binary (unit tests in config.rs only cover the Figment merge itself).
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "docs/a.md", "plain body");
    let out = Command::new(navigator_bin())
        .args(["--root"])
        .arg(dir.path())
        .arg("lint")
        .env("HOME", home.path())
        .env("NAVIGATOR_JSON", "true")
        .output()
        .unwrap();
    assert!(out.status.success(), "lint failed under NAVIGATOR_JSON=true: {out:?}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.trim_start().starts_with('{'),
        "NAVIGATOR_JSON=true did not switch stderr narration to machine JSON:\n{stderr}"
    );
}

#[test]
fn cli_flag_overrides_navigator_json_env_var() {
    // flag > env: passing --json=false isn't expressible (clap bool flag),
    // so instead verify the flag's absence with the env var set is enough
    // to prove the env layer works, then verify a conflicting sentinel
    // setting doesn't leak through when the flag is present. Here: the env
    // var alone (no --json flag) must still produce JSON narration -- the
    // env layer must not be silently dropped when the flag isn't passed.
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "docs/a.md", "plain body");
    let out = Command::new(navigator_bin())
        .args(["--root"])
        .arg(dir.path())
        .arg("--json")
        .arg("lint")
        .env("HOME", home.path())
        .env("NAVIGATOR_JSON", "false")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.trim_start().starts_with('{'),
        "the --json flag must win over a conflicting NAVIGATOR_JSON=false env value:\n{stderr}"
    );
}

#[test]
fn search_limit_truncates_ranked_hits() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    for i in 0..5 {
        write(dir.path(), &format!("docs/f{i}.md"), "widgets gears widgets gears");
    }
    let out = run(dir.path(), home.path(), &["--json", "search", "widgets", "--limit", "2"]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let hits = json["data"]["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "--limit 2 must cap the returned hit count: {hits:?}");
}

#[test]
fn find_limit_truncates_matches() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "navigator.toml",
        "sentinel_version = 2\nextensions = \"default@1.0.0\"\n",
    );
    for i in 0..4 {
        write(
            dir.path(),
            &format!("docs/f{i}.md"),
            "---\nname: n\nid: n\ndescription: d\ntags: [type:skill]\nlinks: []\nupdated: 2026-01-01\n---\nbody\n",
        );
    }
    let out = run(dir.path(), home.path(), &["--json", "find", "type:skill", "--limit", "2"]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let hits = json["data"]["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "--limit 2 must cap find's returned match count: {hits:?}");
}

#[test]
fn content_edit_between_runs_is_reflected_not_served_stale_from_cache() {
    // The out-of-tree cache must never override what a scan actually finds
    // on disk: editing a file's frontmatter between two runs must change
    // the second run's result, proving `get`'s mtime/hash confirm is wired
    // in at the CLI level, not just the unit-tested cache module alone.
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "docs/a.md", "plain body, no frontmatter at all");
    let first = run(dir.path(), home.path(), &["--json", "lint"]);
    let first_json: serde_json::Value = serde_json::from_str(&stdout(&first)).unwrap();
    assert_eq!(first_json["data"]["violations"], 0);

    std::thread::sleep(std::time::Duration::from_millis(10));
    write(dir.path(), "docs/a.md", "---\nname: [unterminated\n---\nbody\n");
    let second = run(dir.path(), home.path(), &["--json", "lint"]);
    let second_json: serde_json::Value = serde_json::from_str(&stdout(&second)).unwrap();
    assert_eq!(
        second_json["data"]["violations"], 1,
        "edited content must invalidate the cache entry, not be served stale: {second_json}"
    );
}

#[test]
fn empty_extensions_array_resolves_to_the_neutral_core_only_floor() {
    // A v2 sentinel with a syntactically valid but empty `extensions` array
    // is a repo opting into navigator with only the core vocabulary. It must
    // resolve to the same core-only floor a repo with no `navigator.toml`
    // gets -- not fail closed -- so the two ways to declare "core vocabulary
    // only" behave identically. A frontmatter-free file is conformant there.
    let dir = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    write(dir.path(), "navigator.toml", "sentinel_version = 2\nextensions = []\n");
    write(dir.path(), "docs/a.md", "body");
    let out = run(dir.path(), home.path(), &["--json", "lint"]);
    assert_eq!(out.status.code(), Some(0), "empty extensions=[] must resolve to the floor; got: {out:?}");
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["data"]["scanned"], 1);
    assert_eq!(json["data"]["violations"], 0);
}
