//! The `navigator.toml` v2 sentinel and the schema-bundle model it resolves
//! to: a repo's opt-in declaration of which frontmatter profile it
//! validates against and which files `lint` covers, standardized so every
//! adopting repo declares vocabulary the same way regardless of whether it
//! ships its own pack or reuses an embedded one.
//!
//! A repo with no `navigator.toml` is not an error -- it resolves to the
//! neutral core-only floor ([`frontmatter::Profile::core_only`]), which
//! validates every file against the schema's mechanisms with zero required
//! fields and zero namespaces.

use std::fmt;
use std::path::Path;

use figment2::providers::{Format, Toml};
use figment2::Figment;
use frontmatter::{MergeWarning, Profile, ProfileError};
use serde::Deserialize;

/// `sentinel_version` values this build understands. A file declaring any
/// other version fails to load rather than being parsed best-effort
/// against the wrong shape.
pub const SUPPORTED_SENTINEL_VERSIONS: &[u64] = &[2];

/// The parsed, validated contents of a repo's `navigator.toml`. Field names
/// mirror the file's own TOML keys, including `sentinel_version` repeating
/// this struct's name -- the sentinel format dictates that key, not this
/// crate's naming style.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_field_names)]
pub struct Sentinel {
    pub sentinel_version: u64,
    /// Informational only: the repo's own build pin, not read by this
    /// crate. Kept so `navigator_version` round-trips through
    /// `deny_unknown_fields` instead of being rejected as unknown.
    #[serde(default)]
    #[allow(dead_code)]
    pub navigator_version: Option<String>,
    #[serde(deserialize_with = "deserialize_extensions")]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub schema: SchemaSection,
    #[serde(default)]
    pub lint: LintSection,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaSection {
    /// The frontmatter profile + version this repo validates against
    /// (e.g. `"core@2.0.0"`), informational -- the embedded core is always
    /// `core@2.0.0` and every embedded pack declares `extends` against it,
    /// so this field is a repo-visible pin rather than a resolver input,
    /// never read by this crate.
    #[serde(default)]
    #[allow(dead_code)]
    pub profile: Option<String>,
    #[serde(default)]
    pub suppress_merge_warnings: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintSection {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Accepts a bare string (one extension) or an array of strings, so a repo
/// with a single pack doesn't have to write `extensions = ["default@1.0.0"]`.
fn deserialize_extensions<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// Why a `navigator.toml` could not be loaded or resolved to a [`Profile`].
#[derive(Debug)]
pub enum SentinelError {
    Load(Box<figment2::Error>),
    UnsupportedVersion(u64),
    /// An `extensions` entry that is neither a known named bundle
    /// (`embedded_pack_json`) nor a readable file at that path, relative to
    /// the repo root.
    UnresolvedExtension(String),
    Profile(ProfileError),
}

impl fmt::Display for SentinelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => write!(f, "navigator.toml is not valid: {e}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "navigator.toml declares sentinel_version {v}, this build supports {SUPPORTED_SENTINEL_VERSIONS:?}"
            ),
            Self::UnresolvedExtension(name) => write!(
                f,
                "extension '{name}' is neither a known bundle nor a file at that path"
            ),
            Self::Profile(e) => write!(f, "schema bundle did not resolve to a valid profile: {e}"),
        }
    }
}

impl std::error::Error for SentinelError {}

/// Reads and validates `path` as a v2 `navigator.toml`, via figment2's TOML
/// provider per SC-STACK. Unknown fields anywhere in the file are rejected
/// (`deny_unknown_fields`): the sentinel is a small, versioned contract,
/// not an open config surface.
pub fn load(path: &Path) -> Result<Sentinel, SentinelError> {
    let sentinel: Sentinel = Figment::new()
        .merge(Toml::file(path))
        .extract()
        .map_err(|e| SentinelError::Load(Box::new(e)))?;
    if !SUPPORTED_SENTINEL_VERSIONS.contains(&sentinel.sentinel_version) {
        return Err(SentinelError::UnsupportedVersion(sentinel.sentinel_version));
    }
    Ok(sentinel)
}

/// The schema bundle a resolved sentinel produces: the merged [`Profile`]
/// every `search`/`find`/`lint` call validates and queries against, plus
/// any non-fatal merge warnings the layering produced (an override or a
/// removal -- see [`frontmatter::MergeWarning`]).
pub struct SchemaBundle {
    pub profile: Profile,
    pub warnings: Vec<MergeWarning>,
}

/// Resolves `sentinel`'s `extensions` to a [`SchemaBundle`]: each entry is
/// tried first as a named bundle (`frontmatter::embedded_pack_json`, e.g.
/// `"default@1.0.0"`), then as a path to a committed pack file, relative to
/// `repo_root`. Packs layer in declaration order onto the embedded core.
///
/// A v2 sentinel with an empty `extensions` list is a repo that opts into
/// navigator (its `[lint]` scope, `[schema]` pin) while adopting only the
/// core vocabulary -- it resolves to the same core-only floor a repo with no
/// `navigator.toml` gets ([`neutral_bundle`]), never a hard failure, so the
/// two ways to say "core vocabulary only" behave identically.
pub fn resolve(sentinel: &Sentinel, repo_root: &Path) -> Result<SchemaBundle, SentinelError> {
    if sentinel.extensions.is_empty() {
        return Ok(neutral_bundle());
    }
    let mut pack_texts: Vec<String> = Vec::with_capacity(sentinel.extensions.len());
    for entry in &sentinel.extensions {
        if let Some(embedded) = frontmatter::embedded_pack_json(entry) {
            pack_texts.push(embedded.to_string());
            continue;
        }
        let path = repo_root.join(entry);
        match std::fs::read_to_string(&path) {
            Ok(text) => pack_texts.push(text),
            Err(_) => return Err(SentinelError::UnresolvedExtension(entry.clone())),
        }
    }
    let pack_refs: Vec<&str> = pack_texts.iter().map(String::as_str).collect();
    let (profile, warnings) = Profile::from_packs(frontmatter::embedded_core_json(), &pack_refs)
        .map_err(SentinelError::Profile)?;
    Ok(SchemaBundle { profile, warnings })
}

/// The floor a repo with no `navigator.toml` resolves to: the embedded
/// core with zero vocabulary. Every file validates against the schema's
/// mechanisms, with nothing required and no namespace to query.
pub fn neutral_bundle() -> SchemaBundle {
    let profile = Profile::core_only(frontmatter::embedded_core_json())
        .expect("the crate's own embedded core JSON is always valid");
    SchemaBundle {
        profile,
        warnings: Vec::new(),
    }
}

/// One human-readable line per [`MergeWarning`], for a `--quiet-schema-warnings`-free
/// run's caveats. `navigator.toml`'s `schema.suppress_merge_warnings` (or
/// the CLI flag of the same intent) suppresses these at the caller, not
/// here -- this function always renders whatever it's given.
#[must_use]
pub fn describe_merge_warning(warning: &MergeWarning) -> String {
    match warning {
        MergeWarning::Override {
            dimension,
            key,
            from_layer,
            to_layer,
            base_layer,
        } => {
            let severity = if *base_layer { "WARN" } else { "INFO" };
            format!(
                "{severity}: {dimension:?} '{key}' from '{from_layer}' overridden by '{to_layer}'"
            )
        }
        MergeWarning::Removal {
            dimension,
            key,
            removing_layer,
            removed_from_layer,
            base_layer,
        } => {
            let severity = if *base_layer { "WARN" } else { "INFO" };
            match removed_from_layer {
                Some(from) => format!(
                    "{severity}: {dimension:?} '{key}' from '{from}' removed by '{removing_layer}'"
                ),
                None => format!(
                    "{severity}: {dimension:?} '{key}' removed by '{removing_layer}' but nothing defined it"
                ),
            }
        }
    }
}
