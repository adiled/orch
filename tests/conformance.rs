use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use the_orch::canonical;
use the_orch::error::OrchError;
use the_orch::types::OrchFile;

const EXPECTED: &str = "expected.orch";

struct Case {
    name: String,
    directory: PathBuf,
    sources: Vec<(String, String)>,
    overrides: HashMap<String, String>,
}

impl Case {
    fn load(directory: PathBuf) -> Case {
        let name = directory
            .file_name()
            .expect("a case directory has a name")
            .to_string_lossy()
            .into_owned();
        let mut inputs: Vec<PathBuf> = entries(&directory)
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "orch"))
            .filter(|path| path.file_name().is_some_and(|file| file != EXPECTED))
            .collect();
        inputs.sort();
        let sources = inputs
            .iter()
            .map(|path| {
                let file = path.file_name().unwrap().to_string_lossy().into_owned();
                (file, read(path))
            })
            .collect();
        let overrides = fs::read_to_string(directory.join("args"))
            .map(|text| overrides(&text))
            .unwrap_or_default();
        Case {
            name,
            directory,
            sources,
            overrides,
        }
    }

    fn resolve(&self) -> Result<OrchFile, Vec<OrchError>> {
        the_orch::parse_files(&self.sources, &self.overrides)
    }

    fn expected(&self) -> String {
        read(&self.directory.join(EXPECTED))
    }
}

fn overrides(args_file: &str) -> HashMap<String, String> {
    args_file
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_string(), value.replace("\\n", "\n")))
        .collect()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"))
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot list {directory:?}: {error}"))
        .map(|entry| entry.expect("a directory entry is readable").path())
        .collect()
}

fn cases(kind: &str) -> Vec<Case> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("conformance")
        .join(kind);
    let mut directories = entries(&root);
    directories.sort();
    directories.into_iter().map(Case::load).collect()
}

fn joined(errors: &[OrchError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn valid_cases_resolve_to_their_expected_canonical_form() {
    let failures: Vec<String> = cases("valid")
        .iter()
        .filter_map(|case| match case.resolve() {
            Ok(orch) => {
                let got = canonical::render(&orch);
                let expected = case.expected();
                (got != expected)
                    .then(|| format!("{}:\n--- expected\n{expected}--- got\n{got}", case.name))
            }
            Err(errors) => Some(format!("{}: rejected: {}", case.name, joined(&errors))),
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn expected_outputs_are_already_canonical() {
    let failures: Vec<String> = cases("valid")
        .iter()
        .filter_map(|case| {
            let expected = case.expected();
            let again = the_orch::parser::parse(&expected, &HashMap::new())
                .map(|orch| canonical::render(&orch));
            match again {
                Ok(text) if text == expected => None,
                Ok(text) => Some(format!("{}: re-resolves to\n{text}", case.name)),
                Err(errors) => Some(format!("{}: rejected: {}", case.name, joined(&errors))),
            }
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn invalid_cases_are_rejected() {
    let accepted: Vec<String> = cases("invalid")
        .iter()
        .filter(|case| case.resolve().is_ok())
        .map(|case| case.name.clone())
        .collect();
    assert!(accepted.is_empty(), "accepted: {accepted:#?}");
}

#[test]
fn the_suite_is_not_empty() {
    assert!(cases("valid").len() >= 40);
    assert!(cases("invalid").len() >= 60);
}
