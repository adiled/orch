mod error;
mod parser;
mod types;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

fn usage() {
    eprintln!("Usage:");
    eprintln!("  orch parse <file> [--arg name=value ...]");
    eprintln!("  orch validate <file> [--arg name=value ...]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  parse      Parse Orchfile and emit JSON to stdout");
    eprintln!("  validate   Validate Orchfile, exit 0 if valid, 1 if errors");
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

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        usage();
        process::exit(2);
    }

    let command = &args[1];
    let file_path = &args[2];
    let remaining = &args[3..];

    let overrides = match collect_overrides(remaining) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    };

    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", file_path, e);
            process::exit(2);
        }
    };

    match command.as_str() {
        "parse" => match parser::parse(&content, &overrides) {
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
        "validate" => match parser::parse(&content, &overrides) {
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
