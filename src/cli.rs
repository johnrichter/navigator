//! The argument surface: one `clap` derive tree, so the parser and the
//! surface it parses stay one definition (see `Cargo.toml`'s `clap` doc
//! comment). Every flag here is resolved into a [`crate::config::RuntimeConfig`]
//! layer or a per-command scan scope in `main.rs` -- this module only
//! shapes and documents the surface, it does not interpret it.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Hybrid BM25 + facet/boolean search and lint over a repo's
/// frontmatter-tagged files.
///
/// A file whose frontmatter fails to parse or fails schema validation is
/// always demoted -- ranked or listed after conformant files -- never
/// dropped from a `search`/`find` result or from `lint`'s report. Adopting a
/// repo (writing its first `navigator.toml`) is not this tool's job: run it
/// against any repo, with or without one -- a repo with none resolves to the
/// neutral core-only schema floor.
#[derive(Debug, Parser)]
#[command(
    name = "navigator",
    version,
    propagate_version = true,
    after_help = "EXAMPLES:\n  \
        navigator search \"span init\"\n  \
        navigator search \"retry backoff\" --filter 'type:skill AND status:complete'\n  \
        navigator find 'type:skill AND topic:apm'\n  \
        navigator lint\n\n\
        EXIT CODES: 0 success, 10 caveats, 40 not_found, 50 usage, 90 internal -- \
        the full eleven-class taxonomy is clikit's, not this tool's own."
)]
pub struct Cli {
    /// The repo root `navigator.toml`, the freshness cache key, and every
    /// scanned path resolve against. Defaults to the current directory --
    /// `navigator.toml` is documented as repo-root-relative, so running
    /// from anywhere else needs this flag.
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<PathBuf>,

    /// Emits stderr narration as logkit's machine JSON instead of its
    /// human line. Stdout's result record is always canonical JSON either
    /// way -- this flag only changes how a person reads the run, not what
    /// a script parses.
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppresses the schema-bundle merge override/removal caveats a
    /// layered `navigator.toml` extension can produce.
    #[arg(long = "quiet-schema-warnings", global = true)]
    pub quiet_schema_warnings: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Hybrid BM25 free-text ranking over the scanned corpus, optionally
    /// narrowed first by a facet/boolean filter.
    Search(SearchArgs),
    /// Facet/boolean query match with no free-text ranking -- every file
    /// the query matches, conformant files first.
    Find(FindArgs),
    /// Validates every scanned file's frontmatter against the resolved
    /// schema bundle. Per-file-class description-length caps come straight
    /// off that resolved schema -- this command's own (non-`--json`) output
    /// logs each present class's live cap, then every violation's code and
    /// message, so an over-cap file is diagnosable from one run alone.
    Lint(LintArgs),
}

impl Command {
    /// The clikit command-path segment this subcommand reports itself as
    /// (`["navigator", <this>]`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Command::Search(_) => "search",
            Command::Find(_) => "find",
            Command::Lint(_) => "lint",
        }
    }

    /// The `--dir` scope this invocation scans, shared by every subcommand.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        match self {
            Command::Search(a) => &a.dir,
            Command::Find(a) => &a.dir,
            Command::Lint(a) => &a.dir,
        }
    }
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// The free-text BM25 query.
    pub query: String,

    /// A facetquery expression every candidate must match before ranking
    /// (e.g. `type:skill AND status:complete`) -- narrows the corpus, it
    /// never itself contributes to the BM25 score.
    #[arg(long)]
    pub filter: Option<String>,

    /// Maximum ranked hits to return.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,

    /// Scan only this repo-root-relative directory. Repeatable. Defaults
    /// to the whole repo.
    #[arg(long = "dir", value_name = "PATH")]
    pub dir: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    /// The facetquery boolean expression (e.g. `type:skill AND topic:apm`,
    /// `NOT status:draft`, `updated>2026-01-01`).
    pub query: String,

    /// Maximum matches to return. Unset returns every match.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Scan only this repo-root-relative directory. Repeatable. Defaults
    /// to the whole repo.
    #[arg(long = "dir", value_name = "PATH")]
    pub dir: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct LintArgs {
    /// Scan only this repo-root-relative directory. Repeatable. Defaults
    /// to the whole repo.
    #[arg(long = "dir", value_name = "PATH")]
    pub dir: Vec<PathBuf>,
}
