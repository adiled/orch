mod error;
mod merge;
mod parser;
mod resolve;
mod types;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

fn usage() {
    eprintln!("Usage:");
    eprintln!("  orch parse <file> [<file> ...] [--arg name=value ...]");
    eprintln!("  orch validate <file> [<file> ...] [--arg name=value ...]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  parse      Parse Orchfile(s) and emit JSON to stdout");
    eprintln!("  validate   Validate Orchfile(s), exit 0 if valid, 1 if errors");
    eprintln!();
    eprintln!("Multiple files are merged left-to-right (overlay model).");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --arg name=value   Override ARG default (repeatable)");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  ORCH_ARG_<name>=value   Override ARG default via environment");
}

fn collect_overrides(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut overrides = HashMap::new();

    // Collect from environment (ORCH_ARG_<name>=value)
    for (key, value) in env::vars() {
        if let Some(name) = key.strip_prefix("ORCH_ARG_") {
            if !name.is_empty() {
                overrides.insert(name.to_string(), value);
            }
        }
    }

    // Collect from --arg flags (higher priority than env)
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--arg" {
            i += 1;
            if i >= args.len() {
                return Err("--arg requires a name=value argument".to_string());
            }
            let arg = &args[i];
            match arg.find('=') {
                Some(pos) => {
                    let name = &arg[..pos];
                    let value = &arg[pos + 1..];
                    if name.is_empty() {
                        return Err(format!("invalid --arg: '{}'", arg));
                    }
                    overrides.insert(name.to_string(), value.to_string());
                }
                None => {
                    return Err(format!("--arg value must be name=value, got '{}'", arg));
                }
            }
        }
        i += 1;
    }

    Ok(overrides)
}

/// Split args after the command into (file_paths, remaining_flags).
/// File paths are all positional args before the first `--arg`.
fn split_files_and_flags(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut flags = Vec::new();
    let mut in_flags = false;

    let mut i = 0;
    while i < args.len() {
        if args[i] == "--arg" {
            in_flags = true;
        }
        if in_flags {
            flags.push(args[i].clone());
        } else {
            files.push(args[i].clone());
        }
        i += 1;
    }

    (files, flags)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        usage();
        process::exit(2);
    }

    let command = &args[1];
    let (file_paths, flag_args) = split_files_and_flags(&args[2..]);

    if file_paths.is_empty() {
        eprintln!("error: at least one file is required");
        usage();
        process::exit(2);
    }

    let overrides = match collect_overrides(&flag_args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    };

    // Read all files
    let mut file_contents: Vec<(String, String)> = Vec::new();
    for path in &file_paths {
        match fs::read_to_string(path) {
            Ok(c) => file_contents.push((path.clone(), c)),
            Err(e) => {
                eprintln!("error: cannot read '{}': {}", path, e);
                process::exit(2);
            }
        }
    }

    // Build refs for parse_files
    let files: Vec<(&str, &str)> = file_contents
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect();

    let result = if files.len() == 1 {
        parser::parse(&files[0].1, &overrides)
    } else {
        parser::parse_files(&files, &overrides)
    };

    match command.as_str() {
        "parse" => match result {
            Ok(orch) => {
                let json = serde_json::to_string_pretty(&orch).unwrap();
                println!("{}", json);
            }
            Err(errors) => {
                for e in &errors {
                    eprintln!("error: {}", e);
                }
                process::exit(1);
            }
        },
        "validate" => match result {
            Ok(_) => {
                eprintln!("valid");
                process::exit(0);
            }
            Err(errors) => {
                for e in &errors {
                    eprintln!("error: {}", e);
                }
                process::exit(1);
            }
        },
        _ => {
            eprintln!("error: unknown command '{}'\n", command);
            usage();
            process::exit(2);
        }
    }
}
