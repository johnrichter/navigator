//! This CLI's one `logkit::Logger`. Stdout is reserved for the clikit
//! `ResultRecord` (see `main.rs`); every log line goes to stderr, either as
//! logkit's human line (the default) or its machine JSON (`--json`) -- see
//! [`crate::config::RuntimeConfig::json`].

use logkit::{Level, Logger, Sink};

/// Builds the logger for this run. `json` selects the stderr rendering;
/// `navigator` is a fixed, schema-valid service name, so construction never
/// fails in practice.
#[must_use]
pub fn build(json: bool) -> Logger {
    let builder = Logger::builder("navigator")
        .service_version(env!("CARGO_PKG_VERSION"))
        .threshold(Level::Info);
    let builder = if json {
        builder.json_writer(Some(Sink::stderr())).human_writer(None)
    } else {
        builder.json_writer(None).human_writer(Some(Sink::stderr()))
    };
    builder
        .build()
        .expect("the fixed service name 'navigator' always satisfies logkit's schema")
}
