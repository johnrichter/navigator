//! Discovers the reachable file set, parses and validates each file's
//! frontmatter, and holds the result every command (`search`/`find`/`lint`)
//! reads from. A file that fails to parse or fails validation is never
//! dropped from this set -- it is demoted (ranked and reported after
//! conformant files, never silently absent) so `search`/`find` still
//! surface it and `lint` still reports exactly why.

use std::path::{Path, PathBuf};

use frontmatter::{
    validate, FrontmatterEntry, FrontmatterParseError, ParsedFrontmatter, Profile, RawFields,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::cache::FreshnessCache;

/// One discovered file's parse/validate outcome. Every file this crate
/// scans becomes exactly one of these -- there is no "excluded" variant.
pub enum Status {
    /// Parsed and validated; conformant iff `entry.is_valid`.
    Validated(FrontmatterEntry),
    /// The file's frontmatter block itself did not parse (unclosed
    /// delimiter, malformed YAML, over-deep nesting, non-mapping top
    /// level). Always demoted -- there is no schema entry to be valid.
    Unparsable(FrontmatterParseError),
}

impl Status {
    /// Conformant files rank/list first; everything else (a validation
    /// failure or an outright parse failure) is demoted, never excluded.
    #[must_use]
    pub fn is_conformant(&self) -> bool {
        matches!(self, Status::Validated(entry) if entry.is_valid)
    }
}

/// One discovered file, its parsed frontmatter (best-effort for an
/// unparsable file -- see [`ParsedFrontmatter::body_only`]) and its
/// [`Status`].
pub struct FileRecord {
    pub rel_path: String,
    pub parsed: ParsedFrontmatter,
    pub status: Status,
}

impl FileRecord {
    /// This file's schema-derived class, or `"unparsable"` for a file whose
    /// frontmatter block itself never parsed -- there is no [`FrontmatterEntry`]
    /// to classify in that case.
    #[must_use]
    pub fn file_class(&self) -> &str {
        match &self.status {
            Status::Validated(entry) => entry.file_class.as_str(),
            Status::Unparsable(_) => "unparsable",
        }
    }
}

/// The default `[lint] include`/`exclude` a repo with no `navigator.toml`
/// (or one that omits the section) scans under: every Markdown file,
/// minus the paths no repo wants scanned.
pub fn default_include() -> Vec<String> {
    vec!["**/*.md".to_string()]
}

pub fn default_exclude() -> Vec<String> {
    vec![".git/**".to_string()]
}

fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Walks `dirs` (repo-root-relative), keeping every file whose POSIX
/// relative path matches `include` and none of `exclude`, parses each
/// kept file's frontmatter through `cache`, validates it against
/// `profile`, and returns one [`FileRecord`] per file -- in a
/// deterministic order (sorted by relative path) so two runs over
/// unchanged input produce identical results regardless of the
/// filesystem's own directory-entry order.
pub fn scan(
    repo_root: &Path,
    dirs: &[PathBuf],
    include: &[String],
    exclude: &[String],
    profile: &Profile,
    cache: &mut FreshnessCache,
) -> Vec<FileRecord> {
    let include_set = build_globset(include);
    let exclude_set = build_globset(exclude);
    let roots: Vec<PathBuf> = if dirs.is_empty() {
        vec![repo_root.to_path_buf()]
    } else {
        dirs.iter().map(|d| repo_root.join(d)).collect()
    };

    let mut records = Vec::new();
    for root in roots {
        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let abs_path = entry.into_path();
            let Ok(rel_path) = abs_path.strip_prefix(repo_root) else {
                continue;
            };
            // Glob matching needs `/`-separated segments regardless of host
            // platform, so relative paths are normalized before matching.
            let rel_str = rel_path.to_string_lossy().replace('\\', "/");
            if !include_set.is_match(&rel_str) || exclude_set.is_match(&rel_str) {
                continue;
            }
            if let Some(record) = load_record(repo_root, &abs_path, &rel_str, profile, cache) {
                records.push(record);
            }
        }
    }
    records.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    records
}

fn load_record(
    _repo_root: &Path,
    abs_path: &Path,
    rel_path: &str,
    profile: &Profile,
    cache: &mut FreshnessCache,
) -> Option<FileRecord> {
    if let Some(parsed) = cache.get(abs_path) {
        let entry = validate::validate(&parsed, rel_path, profile);
        return Some(FileRecord {
            rel_path: rel_path.to_string(),
            parsed,
            status: Status::Validated(entry),
        });
    }

    let contents = std::fs::read(abs_path).ok()?;
    let text = String::from_utf8_lossy(&contents).into_owned();
    match frontmatter::parse(&text) {
        Ok(parsed) => {
            cache.insert(abs_path, &contents, parsed.clone());
            let entry = validate::validate(&parsed, rel_path, profile);
            Some(FileRecord {
                rel_path: rel_path.to_string(),
                parsed,
                status: Status::Validated(entry),
            })
        }
        Err(e) => {
            // Not cached: a parse failure is rare and re-attempting it
            // costs nothing the freshness cache needs to save. `parse`
            // is this crate's only entry point for parsing untrusted
            // input, so an unparsable file gets a body-only stand-in built
            // by hand from its public fields, the same shape `parse`
            // itself would have produced for a frontmatter-free file.
            let parsed = ParsedFrontmatter {
                tags: Vec::new(),
                name: None,
                id: None,
                description: None,
                body_text: text,
                raw_fields: RawFields::from_ordered_pairs(Vec::new()),
            };
            Some(FileRecord {
                rel_path: rel_path.to_string(),
                parsed,
                status: Status::Unparsable(e),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn profile() -> Profile {
        Profile::core_only(frontmatter::embedded_core_json()).unwrap()
    }

    #[test]
    fn nonconformant_and_unparsable_files_are_present_not_excluded() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("good.md"), "no frontmatter, plain body").unwrap();
        fs::write(dir.path().join("broken.md"), "---\nname: [unterminated").unwrap();
        let mut cache = FreshnessCache::open_at(&dir.path().join(".cache.json"));
        let records = scan(
            dir.path(),
            &[],
            &default_include(),
            &default_exclude(),
            &profile(),
            &mut cache,
        );
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|r| r.rel_path == "good.md" && r.status.is_conformant()));
        assert!(records
            .iter()
            .any(|r| r.rel_path == "broken.md" && !r.status.is_conformant()));
    }

    #[test]
    fn scan_is_deterministic_across_runs() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        let mut cache = FreshnessCache::open_at(&dir.path().join(".cache.json"));
        let records = scan(
            dir.path(),
            &[],
            &default_include(),
            &default_exclude(),
            &profile(),
            &mut cache,
        );
        let paths: Vec<&str> = records.iter().map(|r| r.rel_path.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md"]);
    }
}
