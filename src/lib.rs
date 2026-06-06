/// The `orch` library: parse Orchfiles into a resolved [`types::OrchFile`].
///
/// The full pipeline (`parser` -> `merge` -> `resolve`) is exposed so consumers
/// can parse in-process rather than shelling out to the `orch` CLI. Deserialize
/// CLI JSON output via [`types`], or call [`parse_files`] / the module functions
/// directly on already-read file contents.
pub mod error;
pub mod merge;
pub mod parser;
pub mod resolve;
pub mod types;

use std::collections::HashMap;

/// Parse, merge, and resolve a set of `(filename, contents)` inputs into a final
/// [`types::OrchFile`].
///
/// Mirrors what `orch parse <files...>` does, but operates on already-read
/// contents (no file I/O) and returns the structured value. Files are merged
/// left-to-right using the overlay model. `overrides` take precedence over
/// Orchfile `ARG` defaults.
pub fn parse_files(
    files: &[(String, String)],
    overrides: &HashMap<String, String>,
) -> Result<types::OrchFile, Vec<error::OrchError>> {
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect();
    parser::parse_files(&refs, overrides)
}
