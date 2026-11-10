pub mod canonical;
pub mod error;
pub mod json;
pub mod parser;
pub mod types;

mod lex;
mod merge;
mod names;
mod syntax;
mod value;
mod vars;

use std::collections::HashMap;

pub fn parse_files(
    files: &[(String, String)],
    overrides: &HashMap<String, String>,
) -> Result<types::OrchFile, Vec<error::OrchError>> {
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect();
    parser::parse_files(&borrowed, overrides)
}
