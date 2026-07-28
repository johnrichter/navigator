//! Out-of-tree freshness cache for parsed frontmatter.
//!
//! Lives entirely under the platform cache directory, one file per repo
//! (named by the SHA-256 of the repo's absolute path) -- never inside the
//! repo working tree, so there is no manifest to commit and nothing here
//! is ever checked in. Deleting the cache file reproduces byte-identical
//! results on the next run: it only ever saves reparse work, never changes
//! what a scan finds.
//!
//! Freshness protocol per file: unchanged mtime+size reuses the cached
//! parse (fast path); a changed mtime or size triggers a SHA-256 confirm,
//! reusing the parse if the hash still matches (a touch without an edit)
//! and reparsing only when it doesn't; no entry at all is a cold-parse
//! miss. A cache file that fails to deserialize (truncated, foreign
//! format, corrupt) is treated as empty rather than a hard error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use frontmatter::ParsedFrontmatter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    mtime_secs: u64,
    mtime_nanos: u32,
    size: u64,
    sha256: String,
    parsed: ParsedFrontmatter,
}

/// On-disk shape: a path-sorted list, so the persisted JSON never depends
/// on hash-map iteration order.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    entries: Vec<(PathBuf, Entry)>,
}

/// A repo-scoped cache of parsed frontmatter, keyed by absolute file path.
pub struct FreshnessCache {
    file_path: PathBuf,
    entries: HashMap<PathBuf, Entry>,
    dirty: bool,
}

impl FreshnessCache {
    /// Opens the cache for `repo_root` under the real platform cache
    /// directory. A missing or unreadable cache file starts empty.
    #[must_use]
    pub fn open(repo_root: &Path) -> Self {
        Self::open_at(&cache_file_path(&platform_cache_base(), repo_root))
    }

    /// Opens the cache at an explicit `file_path` -- the seam tests use to
    /// point at a temp directory instead of the operator's real cache.
    #[must_use]
    pub fn open_at(file_path: &Path) -> Self {
        Self {
            file_path: file_path.to_path_buf(),
            entries: read_cache_file(file_path),
            dirty: false,
        }
    }

    /// Returns the cached parse for `abs_path` iff its fingerprint still
    /// matches what's on disk. On a size/mtime mismatch, confirms via
    /// content hash before declaring a miss (so a touch without an edit
    /// stays a hit); a hash mismatch invalidates the entry outright.
    pub fn get(&mut self, abs_path: &Path) -> Option<ParsedFrontmatter> {
        let metadata = std::fs::metadata(abs_path).ok()?;
        let (mtime_secs, mtime_nanos) = fingerprint_mtime(&metadata);
        let size = metadata.len();
        let entry = self.entries.get(abs_path)?;
        if entry.mtime_secs == mtime_secs && entry.mtime_nanos == mtime_nanos && entry.size == size {
            return Some(entry.parsed.clone());
        }
        let contents = std::fs::read(abs_path).ok()?;
        let sha256 = sha256_hex(&contents);
        if sha256 == entry.sha256 {
            let parsed = entry.parsed.clone();
            self.put(abs_path, mtime_secs, mtime_nanos, size, sha256, parsed.clone());
            return Some(parsed);
        }
        None
    }

    /// Records `parsed` as the current fingerprinted result for `abs_path`.
    /// A caller reparses first (this cache never parses on its own) and
    /// stores the result here so the next run's [`get`](Self::get) can
    /// reuse it.
    pub fn insert(&mut self, abs_path: &Path, contents: &[u8], parsed: ParsedFrontmatter) {
        if let Ok(metadata) = std::fs::metadata(abs_path) {
            let (mtime_secs, mtime_nanos) = fingerprint_mtime(&metadata);
            let sha256 = sha256_hex(contents);
            self.put(abs_path, mtime_secs, mtime_nanos, metadata.len(), sha256, parsed);
        }
    }

    fn put(
        &mut self,
        abs_path: &Path,
        mtime_secs: u64,
        mtime_nanos: u32,
        size: u64,
        sha256: String,
        parsed: ParsedFrontmatter,
    ) {
        self.entries.insert(
            abs_path.to_path_buf(),
            Entry {
                mtime_secs,
                mtime_nanos,
                size,
                sha256,
                parsed,
            },
        );
        self.dirty = true;
    }

    /// Persists the cache if anything changed this run. Best-effort: a
    /// write failure (read-only cache dir, disk full) is silently dropped
    /// -- the cache is an accelerator, never a source of truth, so losing a
    /// write only costs the next run some reparse time.
    pub fn save(&self) {
        if !self.dirty {
            return;
        }
        let Some(parent) = self.file_path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let mut entries: Vec<(PathBuf, Entry)> = self
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if let Ok(json) = serde_json::to_string(&CacheFile { entries }) {
            let _ = std::fs::write(&self.file_path, json);
        }
    }
}

/// The real platform cache base directory, with `navigator` appended.
fn platform_cache_base() -> PathBuf {
    let base = dirs::cache_dir()
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("navigator")
}

/// The cache file for `repo_root`, named by the SHA-256 hex of its
/// absolute path so two distinct repos -- including two clones of the same
/// repo at different locations -- never collide on one cache file.
fn cache_file_path(cache_base: &Path, repo_root: &Path) -> PathBuf {
    let abs = std::path::absolute(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let key = sha256_hex(abs.to_string_lossy().as_bytes());
    cache_base.join(format!("{key}.json"))
}

fn read_cache_file(path: &Path) -> HashMap<PathBuf, Entry> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(file) = serde_json::from_str::<CacheFile>(&contents) else {
        return HashMap::new();
    };
    file.entries.into_iter().collect()
}

fn fingerprint_mtime(metadata: &std::fs::Metadata) -> (u64, u32) {
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let since_epoch = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    (since_epoch.as_secs(), since_epoch.subsec_nanos())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    fn parsed_stub() -> ParsedFrontmatter {
        frontmatter::parse("no frontmatter here").unwrap()
    }

    #[test]
    fn miss_on_first_lookup_then_hit_after_insert() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "one").unwrap();
        let cache_path = dir.path().join("cache.json");
        let mut cache = FreshnessCache::open_at(&cache_path);
        assert!(cache.get(&file).is_none());
        cache.insert(&file, b"one", parsed_stub());
        assert!(cache.get(&file).is_some());
    }

    #[test]
    fn survives_and_reuses_across_save_and_reopen() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "one").unwrap();
        let cache_path = dir.path().join("cache.json");
        let mut cache = FreshnessCache::open_at(&cache_path);
        cache.insert(&file, b"one", parsed_stub());
        cache.save();

        let mut reopened = FreshnessCache::open_at(&cache_path);
        assert!(reopened.get(&file).is_some());
    }

    #[test]
    fn touch_without_edit_stays_a_hit() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "one").unwrap();
        let cache_path = dir.path().join("cache.json");
        let mut cache = FreshnessCache::open_at(&cache_path);
        cache.insert(&file, b"one", parsed_stub());

        // Advance mtime without changing content.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&file, "one").unwrap();
        assert!(cache.get(&file).is_some());
    }

    #[test]
    fn content_change_is_a_miss() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "one").unwrap();
        let cache_path = dir.path().join("cache.json");
        let mut cache = FreshnessCache::open_at(&cache_path);
        cache.insert(&file, b"one", parsed_stub());

        std::thread::sleep(Duration::from_millis(10));
        fs::write(&file, "two, much longer content").unwrap();
        assert!(cache.get(&file).is_none());
    }

    #[test]
    fn corrupt_cache_file_is_treated_as_empty() {
        let dir = TempDir::new().unwrap();
        let cache_path = dir.path().join("cache.json");
        fs::write(&cache_path, "not json").unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "one").unwrap();
        let mut cache = FreshnessCache::open_at(&cache_path);
        assert!(cache.get(&file).is_none());
    }
}
