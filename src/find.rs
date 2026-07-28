//! `navigator find`: facet/boolean query match, no free-text ranking.
//!
//! Every file the query matches is returned, conformant files first (path
//! order within each group) -- a nonconformant match is demoted to the tail
//! of the list, never dropped.

use clikit::{log_diagnostic, ClikitError, Diagnostic, ResultRecord, Status, Triage};
use logkit::Level;
use serde_json::json;

use crate::cli::FindArgs;
use crate::corpus::FileRecord;

/// Runs `find`, returning the finished [`ResultRecord`] -- `Success` for
/// any match count including zero; `Usage` if `query` fails to parse.
pub fn run(
    logger: &logkit::Logger,
    records: &[FileRecord],
    profile: &frontmatter::Profile,
    args: &FindArgs,
) -> Result<ResultRecord, ClikitError> {
    let query = match facetquery::parse(&args.query) {
        Ok(query) => query,
        Err(e) => {
            let diagnostic = Diagnostic::new(
                "usage.find.query_invalid",
                format!("query did not parse: {e}"),
                Triage::manual("fix the facetquery expression and retry"),
            );
            let _ = log_diagnostic(logger, Level::Error, "find rejected", &diagnostic);
            return ResultRecord::builder(Status::Usage, ["navigator", "find"])
                .error(diagnostic)
                .build();
        }
    };

    let mut matches: Vec<&FileRecord> = records
        .iter()
        .filter(|r| frontmatter::matches(&r.parsed, &query, profile).matched)
        .collect();
    matches.sort_by(|a, b| {
        b.status
            .is_conformant()
            .cmp(&a.status.is_conformant())
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    if let Some(limit) = args.limit {
        matches.truncate(limit);
    }

    for record in &matches {
        if !record.status.is_conformant() {
            let _ = logger
                .warn(format!("{}: nonconformant match, demoted not excluded", record.rel_path))
                .emit();
        }
    }

    let hits: Vec<_> = matches
        .iter()
        .map(|record| {
            json!({
                "path": record.rel_path,
                "file_class": record.file_class(),
                "conformant": record.status.is_conformant(),
            })
        })
        .collect();

    ResultRecord::builder(Status::Success, ["navigator", "find"])
        .data("hits", hits)
        .build()
}
