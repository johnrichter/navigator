//! `navigator` -- the CLI entry point.
//!
//! One invocation: parse args, resolve `navigator.toml` (or the neutral
//! floor if a repo has none) into a schema bundle, scan the corpus through
//! the out-of-tree freshness cache, run the requested subcommand, then
//! write exactly one clikit [`ResultRecord`] to stdout as canonical JSON
//! and exit with its paired code. Every subcommand's own narration goes to
//! stderr through [`logging::build`]; stdout never carries a log line.

mod cache;
mod cli;
mod config;
mod corpus;
mod find;
mod lint;
mod logging;
mod search;
mod sentinel;

use std::process::ExitCode;

use clap::Parser;
use clikit::{ClikitError, Diagnostic, ResultRecord, Status, Triage};

use cache::FreshnessCache;
use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let repo_root = cli
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current directory is readable"));
    let command_name = cli.command.name();

    let sentinel_path = repo_root.join("navigator.toml");
    let sentinel = if sentinel_path.is_file() {
        match sentinel::load(&sentinel_path) {
            Ok(loaded) => Some(loaded),
            Err(e) => {
                // No sentinel resolved, but `RuntimeConfig::json` only ever
                // comes from the flag/env layers (see `config::FromSentinel`),
                // so a `None` sentinel here still resolves it correctly.
                let runtime = config::resolve(None, cli.json, cli.quiet_schema_warnings);
                let logger = logging::build(runtime.json);
                return finish(&logger, &sentinel_failure(command_name, &e));
            }
        }
    } else {
        None
    };

    let runtime = config::resolve(sentinel.as_ref(), cli.json, cli.quiet_schema_warnings);
    let logger = logging::build(runtime.json);

    let bundle = match &sentinel {
        Some(s) => match sentinel::resolve(s, &repo_root) {
            Ok(bundle) => bundle,
            Err(e) => return finish(&logger, &schema_failure(command_name, &e)),
        },
        None => sentinel::neutral_bundle(),
    };

    if !runtime.quiet_schema_warnings {
        for warning in &bundle.warnings {
            let _ = logger.info(sentinel::describe_merge_warning(warning)).emit();
        }
    }

    let (include, exclude) = scan_globs(sentinel.as_ref());
    let mut cache = FreshnessCache::open(&repo_root);
    let records = corpus::scan(
        &repo_root,
        cli.command.dirs(),
        &include,
        &exclude,
        &bundle.profile,
        &mut cache,
    );
    cache.save();

    let outcome = match &cli.command {
        Command::Search(args) => search::run(&logger, &records, &bundle.profile, args),
        Command::Find(args) => find::run(&logger, &records, &bundle.profile, args),
        Command::Lint(_) => lint::run(&logger, &records, &bundle.profile),
    };
    let record = outcome.unwrap_or_else(|e| internal_failure(command_name, &e));

    finish(&logger, &record)
}

/// A `navigator.toml` present but not a supported/valid v2 sentinel: the
/// repo's own state, not this invocation, is unready.
fn sentinel_failure(command_name: &str, error: &sentinel::SentinelError) -> ResultRecord {
    let diagnostic = Diagnostic::new(
        "precondition_unmet.sentinel.invalid",
        format!("navigator.toml: {error}"),
        Triage::manual("fix navigator.toml and retry"),
    );
    ResultRecord::builder(Status::PreconditionUnmet, ["navigator", command_name])
        .error(diagnostic)
        .build()
        .expect("one correctly-prefixed error always builds a precondition_unmet record")
}

/// A syntactically valid sentinel whose `extensions` don't resolve to a
/// schema bundle (an unknown named pack, or an unreadable committed pack
/// file).
fn schema_failure(command_name: &str, error: &sentinel::SentinelError) -> ResultRecord {
    let diagnostic = Diagnostic::new(
        "precondition_unmet.schema.unresolved",
        format!("schema bundle: {error}"),
        Triage::manual("fix navigator.toml's extensions and retry"),
    );
    ResultRecord::builder(Status::PreconditionUnmet, ["navigator", command_name])
        .error(diagnostic)
        .build()
        .expect("one correctly-prefixed error always builds a precondition_unmet record")
}

/// A subcommand's own [`ResultRecord::builder`] call failed its schema
/// validation -- a navigator defect (a governing-code/prefix mismatch this
/// crate introduced), never a caller input problem.
fn internal_failure(command_name: &str, error: &ClikitError) -> ResultRecord {
    let diagnostic = Diagnostic::new(
        "internal.clikit.record_build_failed",
        format!("could not build a result record: {error}"),
        Triage::manual("this is a navigator defect; file a bug with the command and its output"),
    );
    ResultRecord::builder(Status::Internal, ["navigator", command_name])
        .error(diagnostic)
        .build()
        .expect("one correctly-prefixed error always builds an internal record")
}

/// The scan scope every subcommand's corpus walk uses: `navigator.toml`'s
/// `[lint]` include/exclude when the sentinel actually declares one,
/// otherwise [`corpus::default_include`]/[`corpus::default_exclude`] -- a
/// sentinel that omits `[lint]` entirely resolves to an empty `include`,
/// which would otherwise silently scan nothing.
fn scan_globs(sentinel: Option<&sentinel::Sentinel>) -> (Vec<String>, Vec<String>) {
    match sentinel {
        Some(s) if !s.lint.include.is_empty() => (s.lint.include.clone(), s.lint.exclude.clone()),
        _ => (corpus::default_include(), corpus::default_exclude()),
    }
}

/// Writes `record` to stdout as canonical JSON, logs the terminating
/// narration line, and returns the process's exit code.
fn finish(logger: &logkit::Logger, record: &ResultRecord) -> ExitCode {
    let json = record
        .canonical_json()
        .expect("a built ResultRecord always serializes");
    println!("{json}");
    let _ = clikit::log_terminating(
        logger,
        record,
        format!("{} finished", record.command.join(" ")),
    );
    ExitCode::from(u8::try_from(record.exit_code).unwrap_or(u8::MAX))
}
