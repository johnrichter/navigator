//! Runtime settings resolution, `flag > env > file > default`, per
//! SC-STACK. The `navigator.toml` sentinel itself is parsed once, through
//! figment2's own TOML provider (`sentinel::load`); this module merges that
//! already-parsed result back in as one more figment2 layer alongside
//! environment and CLI-flag overrides, so `navigator.toml` is never read
//! from disk twice.

use figment2::providers::{Env, Serialized};
use figment2::Figment;
use serde::{Deserialize, Serialize};

use crate::sentinel::Sentinel;

/// Settings a repo, the environment, or a flag may each want the final say
/// over -- everything else about a run (which files, which query) is a
/// per-invocation argument, not a layered setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Renders stderr narration as logkit's machine JSON instead of its
    /// human line. stdout's result record is unaffected either way -- it
    /// is always canonical JSON, per the clikit contract.
    pub json: bool,
    /// Suppresses schema-bundle merge override/removal caveats.
    pub quiet_schema_warnings: bool,
}

/// What `navigator.toml` contributes to [`RuntimeConfig`], as a figment2
/// layer. Only the fields the sentinel schema actually declares appear
/// here -- there is no independent file-level `json` setting.
#[derive(Debug, Serialize)]
struct FromSentinel {
    quiet_schema_warnings: bool,
}

/// What CLI flags contribute, as a figment2 layer. `None` means "not
/// passed" and is omitted from serialization, so an unset flag never
/// overrides a lower layer (`Figment` merge only ever sees the keys a
/// layer actually provides).
#[derive(Debug, Default, Serialize)]
struct FromFlags {
    #[serde(skip_serializing_if = "Option::is_none")]
    json: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quiet_schema_warnings: Option<bool>,
}

/// Resolves the final [`RuntimeConfig`] for this invocation: CLI flags win
/// over `NAVIGATOR_*` environment variables, which win over `navigator.toml`,
/// which wins over the built-in default.
pub fn resolve(
    sentinel: Option<&Sentinel>,
    cli_json: bool,
    cli_quiet_schema_warnings: bool,
) -> RuntimeConfig {
    let mut figment = Figment::new().merge(Serialized::defaults(RuntimeConfig::default()));

    if let Some(sentinel) = sentinel {
        figment = figment.merge(Serialized::defaults(FromSentinel {
            quiet_schema_warnings: sentinel.schema.suppress_merge_warnings,
        }));
    }

    figment = figment.merge(Env::prefixed("NAVIGATOR_"));

    figment = figment.merge(Serialized::defaults(FromFlags {
        json: cli_json.then_some(true),
        quiet_schema_warnings: cli_quiet_schema_warnings.then_some(true),
    }));

    figment
        .extract()
        .expect("every layer is a valid RuntimeConfig source")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_human_output_and_warnings_shown() {
        let config = resolve(None, false, false);
        assert!(!config.json);
        assert!(!config.quiet_schema_warnings);
    }

    #[test]
    fn sentinel_can_set_quiet_schema_warnings() {
        use figment2::providers::Format;

        let toml = "sentinel_version = 2\nextensions = []\n[schema]\nsuppress_merge_warnings = true\n";
        let sentinel: Sentinel = figment2::Figment::new()
            .merge(figment2::providers::Toml::string(toml))
            .extract()
            .unwrap();
        let config = resolve(Some(&sentinel), false, false);
        assert!(config.quiet_schema_warnings);
    }

    #[test]
    fn cli_flag_overrides_everything_below_it() {
        let config = resolve(None, true, false);
        assert!(config.json);
    }
}
