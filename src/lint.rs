//! `navigator lint`: the FB12-successor frontmatter conformance report.
//!
//! Per-file-class description-length caps are never a navigator-owned copy
//! -- every run reads them straight off the resolved schema
//! ([`frontmatter::Profile::description_cap`]) and logs the cap in force
//! for every class actually present in the scanned set. Every violation's
//! code and message are logged at `Warn` on the run's own (non-`--json`)
//! output too, not only inside the `--json` record, so a file over its
//! class's cap is diagnosable from one run's terminal output alone.

use std::collections::BTreeSet;

use clikit::{log_diagnostic, ClikitError, Diagnostic, ResultRecord, Status, Triage};
use logkit::Level;
use serde_json::json;

use crate::corpus::{FileRecord, Status as ScanStatus};

/// One violation, resolved to the file it's about -- [`frontmatter::Violation`]
/// carries no path of its own since it's scoped to a single file's
/// [`frontmatter::FrontmatterEntry`].
struct Finding<'a> {
    rel_path: &'a str,
    code: String,
    message: String,
}

/// Runs `lint`, returning the finished [`ResultRecord`]: `Success` with zero
/// violations, `Caveats` (one caveat per finding, capped at clikit's
/// 50-member limit) otherwise. Never a failure class -- a nonconformant
/// file is a reportable condition, not this invocation's own error.
pub fn run(
    logger: &logkit::Logger,
    records: &[FileRecord],
    profile: &frontmatter::Profile,
) -> Result<ResultRecord, ClikitError> {
    log_class_caps(logger, records, profile);

    let findings = collect_findings(records);
    for finding in &findings {
        let diagnostic = caveat_diagnostic(finding);
        let _ = log_diagnostic(
            logger,
            Level::Warn,
            format!("{}: {}", finding.rel_path, finding.code),
            &diagnostic,
        );
    }

    let files: Vec<_> = records
        .iter()
        .map(|r| {
            json!({
                "path": r.rel_path,
                "file_class": r.file_class(),
                "conformant": r.status.is_conformant(),
            })
        })
        .collect();

    if findings.is_empty() {
        return ResultRecord::builder(Status::Success, ["navigator", "lint"])
            .data("scanned", records.len() as u64)
            .data("violations", 0u64)
            .data("files", files)
            .build();
    }

    let mut builder = ResultRecord::builder(Status::Caveats, ["navigator", "lint"])
        .data("scanned", records.len() as u64)
        .data("violations", findings.len() as u64)
        .data("files", files);
    for finding in findings.iter().take(50) {
        builder = builder.caveat(caveat_diagnostic(finding));
    }
    builder.build()
}

fn caveat_diagnostic(finding: &Finding<'_>) -> Diagnostic {
    Diagnostic::new(
        format!("caveats.lint.{}", finding.code.to_lowercase()),
        format!("{}: {}", finding.rel_path, finding.message),
        Triage::manual(format!(
            "fix {}'s frontmatter and re-run `navigator lint`",
            finding.rel_path
        )),
    )
}

fn collect_findings(records: &[FileRecord]) -> Vec<Finding<'_>> {
    let mut findings = Vec::new();
    for record in records {
        match &record.status {
            ScanStatus::Validated(entry) => {
                for violation in &entry.violations {
                    findings.push(Finding {
                        rel_path: &record.rel_path,
                        code: violation.code.clone(),
                        message: violation.message.clone(),
                    });
                }
            }
            ScanStatus::Unparsable(error) => findings.push(Finding {
                rel_path: &record.rel_path,
                code: "UNPARSABLE_FRONTMATTER".to_string(),
                message: error.to_string(),
            }),
        }
    }
    findings
}

/// Logs the live description-length cap for every file class this scan
/// actually saw, straight off `profile` -- the one place an author reads
/// the caps in force, sourced fresh every run rather than restated as a
/// second, driftable copy in this crate's own source or `--help` text.
fn log_class_caps(logger: &logkit::Logger, records: &[FileRecord], profile: &frontmatter::Profile) {
    let classes: BTreeSet<&str> = records.iter().map(FileRecord::file_class).collect();
    for class in classes {
        let cap = profile
            .description_cap(class)
            .map_or_else(|| "uncapped".to_string(), |cap| cap.to_string());
        let _ = logger
            .info(format!("file class '{class}' description cap: {cap}"))
            .emit();
    }
}
