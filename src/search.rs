//! `navigator search`: hybrid BM25 free-text ranking over the scanned
//! corpus.
//!
//! A nonconformant candidate is never dropped from the index -- it competes
//! for a hit on equal terms -- but the final ranking always lists every
//! conformant hit before any nonconformant one, score order preserved
//! within each group. Demoted, never excluded.

use bm25::{OkapiDocument, OkapiIndex, ScoredDocument, Tokenizer};
use clikit::{log_diagnostic, ClikitError, Diagnostic, ResultRecord, Status, Triage};
use logkit::Level;
use serde_json::json;

use crate::cli::SearchArgs;
use crate::corpus::FileRecord;

/// Runs `search`, returning the finished [`ResultRecord`] -- always
/// `Success` (an empty `hits` array is a legitimate "nothing matched", not a
/// failure) unless `--filter` fails to parse, which is `Usage`.
pub fn run(
    logger: &logkit::Logger,
    records: &[FileRecord],
    profile: &frontmatter::Profile,
    args: &SearchArgs,
) -> Result<ResultRecord, ClikitError> {
    let candidates: Vec<&FileRecord> = match &args.filter {
        None => records.iter().collect(),
        Some(expr) => match facetquery::parse(expr) {
            Ok(query) => records
                .iter()
                .filter(|r| frontmatter::matches(&r.parsed, &query, profile).matched)
                .collect(),
            Err(e) => {
                let diagnostic = Diagnostic::new(
                    "usage.search.filter_invalid",
                    format!("--filter did not parse: {e}"),
                    Triage::manual("fix the --filter facetquery expression and retry"),
                );
                let _ = log_diagnostic(logger, Level::Error, "search rejected", &diagnostic);
                return ResultRecord::builder(Status::Usage, ["navigator", "search"])
                    .error(diagnostic)
                    .build();
            }
        },
    };

    // Text bodies must outlive the `OkapiDocument`s borrowing from them.
    let texts: Vec<String> = candidates.iter().map(|r| search_text(r)).collect();
    let docs = candidates
        .iter()
        .zip(&texts)
        .map(|(record, text)| OkapiDocument {
            id: record.rel_path.as_str(),
            text: text.as_str(),
        });
    let index = OkapiIndex::build(Tokenizer::CaseSplit, docs);

    // Ask for every candidate's score, not just `--limit` worth -- the
    // conformant-first resort below must see the whole ranked set before
    // truncating, or a low-scoring conformant file could be cut in favor of
    // a higher-scoring nonconformant one instead of merely ranked after it.
    let mut scored = index.search(&args.query, candidates.len());
    let is_conformant = |scored: &ScoredDocument| {
        candidates
            .iter()
            .find(|r| r.rel_path == scored.id)
            .is_some_and(|r| r.status.is_conformant())
    };
    scored.sort_by(|a, b| {
        is_conformant(b)
            .cmp(&is_conformant(a))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    scored.truncate(args.limit);

    for hit in &scored {
        if !is_conformant(hit) {
            let _ = logger
                .warn(format!("{}: nonconformant hit, demoted not excluded", hit.id))
                .emit();
        }
    }

    let hits: Vec<_> = scored
        .iter()
        .map(|hit| {
            let record = candidates.iter().find(|r| r.rel_path == hit.id);
            json!({
                "path": hit.id,
                "score": hit.score,
                "file_class": record.map(|r| r.file_class()).unwrap_or_default(),
                "conformant": is_conformant(hit),
            })
        })
        .collect();

    ResultRecord::builder(Status::Success, ["navigator", "search"])
        .data("hits", hits)
        .build()
}

/// The text one document contributes to the BM25 index: everything a
/// bareword full-text search would also read (name, id, description, body),
/// so `search` and `find`'s bareword matching stay conceptually aligned.
fn search_text(record: &FileRecord) -> String {
    let p = &record.parsed;
    [
        p.name.as_deref().unwrap_or(""),
        p.id.as_deref().unwrap_or(""),
        p.description.as_deref().unwrap_or(""),
        p.body_text.as_str(),
    ]
    .join(" ")
}
