use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use the_orch::error::OrchError;
use the_orch::types::OrchFile;
use the_orch::{canonical, json};

const USAGE: &str = "\
Usage:
  orch resolve <file> [<file> ...] [--json] [--arg name=value ...]
  orch validate <file> [<file> ...] [--arg name=value ...]

Commands:
  resolve    Merge the files and print the canonical Orchfile
  validate   Check the files; exit 0 if valid, 1 if not

Multiple files are merged left to right: the first is the base, the rest are overlays.

Options:
  --json             Print the resolved result as JSON (see schema/orch.schema.json)
  --arg name=value   Override an ARG default (repeatable; also --arg=name=value)

Environment:
  ORCH_ARG_<name>=value   Override an ARG default
";

const EXIT_INVALID: u8 = 1;
const EXIT_USAGE: u8 = 2;

#[derive(Debug, PartialEq)]
enum Command {
    Resolve,
    Validate,
}

#[derive(Debug, PartialEq)]
enum Format {
    Canonical,
    Json,
}

#[derive(Debug, PartialEq)]
struct Invocation {
    command: Command,
    format: Format,
    files: Vec<String>,
    arg_flags: Vec<(String, String)>,
}

fn main() -> ExitCode {
    let invocation = match invocation(env::args().skip(1).collect()) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let sources = match read_files(&invocation.files) {
        Ok(sources) => sources,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let overrides = overrides(env::vars(), &invocation.arg_flags);
    match resolve(&sources, &overrides) {
        Ok(orch) => {
            warn_about_undeclared(&invocation.arg_flags, &orch);
            report_success(&invocation, &orch);
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("error: {error}");
            }
            ExitCode::from(EXIT_INVALID)
        }
    }
}

fn invocation(args: Vec<String>) -> Result<Invocation, String> {
    let mut args = args.into_iter();
    let command = match args.next().as_deref() {
        Some("resolve") => Command::Resolve,
        Some("validate") => Command::Validate,
        Some("parse") => return Err("'parse' was renamed to 'resolve'; add --json for JSON".into()),
        Some(other) => return Err(format!("unknown command '{other}'")),
        None => return Err("a command is required".into()),
    };

    let mut format = Format::Canonical;
    let mut files = Vec::new();
    let mut arg_flags = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--json" {
            format = Format::Json;
        } else if arg == "--arg" {
            let assignment = args.next().ok_or("--arg requires name=value")?;
            arg_flags.push(arg_assignment(&assignment)?);
        } else if let Some(assignment) = arg.strip_prefix("--arg=") {
            arg_flags.push(arg_assignment(assignment)?);
        } else if arg.starts_with("--") {
            return Err(format!("unknown option '{arg}'"));
        } else {
            files.push(arg);
        }
    }

    if files.is_empty() {
        return Err("at least one file is required".into());
    }
    Ok(Invocation {
        command,
        format,
        files,
        arg_flags,
    })
}

fn arg_assignment(assignment: &str) -> Result<(String, String), String> {
    match assignment.split_once('=') {
        Some((name, value)) if !name.is_empty() => Ok((name.to_string(), value.to_string())),
        _ => Err(format!("--arg expects name=value, got '{assignment}'")),
    }
}

fn overrides(
    environment: impl Iterator<Item = (String, String)>,
    arg_flags: &[(String, String)],
) -> HashMap<String, String> {
    let from_environment = environment.filter_map(|(key, value)| {
        key.strip_prefix("ORCH_ARG_")
            .filter(|name| !name.is_empty())
            .map(|name| (name.to_string(), value))
    });
    from_environment.chain(arg_flags.iter().cloned()).collect()
}

fn read_files(paths: &[String]) -> Result<Vec<(String, String)>, String> {
    paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|content| (path.clone(), content))
                .map_err(|error| format!("cannot read '{path}': {error}"))
        })
        .collect()
}

fn resolve(
    sources: &[(String, String)],
    overrides: &HashMap<String, String>,
) -> Result<OrchFile, Vec<OrchError>> {
    match sources {
        [(_, only)] => the_orch::parser::parse(only, overrides),
        _ => the_orch::parse_files(sources, overrides),
    }
}

fn warn_about_undeclared(arg_flags: &[(String, String)], orch: &OrchFile) {
    for (name, _) in arg_flags {
        if !orch.args.contains_key(name) {
            eprintln!("warning: --arg {name} matches no ARG declared in the files");
        }
    }
}

fn report_success(invocation: &Invocation, orch: &OrchFile) {
    match (&invocation.command, &invocation.format) {
        (Command::Validate, _) => eprintln!("valid"),
        (Command::Resolve, Format::Canonical) => print!("{}", canonical::render(orch)),
        (Command::Resolve, Format::Json) => println!("{}", json::render(orch)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Invocation, String> {
        invocation(args.iter().map(|arg| arg.to_string()).collect())
    }

    #[test]
    fn files_after_a_flag_are_still_files() {
        let got = parsed(&["resolve", "base.orch", "--arg", "port=9090", "over.orch"]).unwrap();
        assert_eq!(got.files, vec!["base.orch", "over.orch"]);
        assert_eq!(
            got.arg_flags,
            vec![("port".to_string(), "9090".to_string())]
        );
    }

    #[test]
    fn flags_may_come_before_files() {
        let got = parsed(&["resolve", "--json", "--arg=port=9090", "base.orch"]).unwrap();
        assert_eq!(got.format, Format::Json);
        assert_eq!(got.files, vec!["base.orch"]);
        assert_eq!(
            got.arg_flags,
            vec![("port".to_string(), "9090".to_string())]
        );
    }

    #[test]
    fn arg_values_may_contain_equals_signs() {
        let got = parsed(&["validate", "a.orch", "--arg", "url=http://h/?a=b"]).unwrap();
        assert_eq!(got.arg_flags[0].1, "http://h/?a=b");
    }

    #[test]
    fn malformed_invocations_are_rejected() {
        for bad in [
            &["resolve"][..],
            &["resolve", "a.orch", "--arg"],
            &["resolve", "a.orch", "--arg", "novalue"],
            &["resolve", "a.orch", "--arg", "=x"],
            &["resolve", "a.orch", "--verbose"],
            &["frobnicate", "a.orch"],
            &[],
        ] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parse_points_to_its_new_name() {
        assert!(
            parsed(&["parse", "a.orch"])
                .unwrap_err()
                .contains("resolve")
        );
    }

    #[test]
    fn arg_flags_override_the_environment() {
        let environment = vec![
            ("ORCH_ARG_port".to_string(), "7070".to_string()),
            ("ORCH_ARG_".to_string(), "ignored".to_string()),
            ("HOME".to_string(), "/root".to_string()),
        ];
        let flags = vec![("port".to_string(), "9090".to_string())];
        let got = overrides(environment.into_iter(), &flags);
        assert_eq!(got.len(), 1);
        assert_eq!(got["port"], "9090");
    }
}
