use std::collections::HashMap;

use crate::error::{OrchError, ParseError};
use crate::types::*;

/// Validate a service name per spec: lowercase alphanumeric + hyphens,
/// starts with letter, max 63 chars.
fn validate_service_name(name: &str, line: usize) -> Result<(), ParseError> {
    if name.is_empty() {
        return Err(ParseError::new(line, "service name cannot be empty"));
    }
    if name.len() > 63 {
        return Err(ParseError::new(
            line,
            format!(
                "service name '{}' exceeds 63 character limit ({})",
                name,
                name.len()
            ),
        ));
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(ParseError::new(
            line,
            format!("service name '{}' must start with a lowercase letter", name),
        ));
    }
    for ch in name.chars() {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(ParseError::new(
                line,
                format!(
                    "service name '{}' contains invalid character '{}' (only lowercase alphanumeric and hyphens allowed)",
                    name, ch
                ),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Top-level parser (delegates to parse_raw → merge → resolve pipeline)
// ---------------------------------------------------------------------------

/// Parse an Orchfile from string content (single-file convenience wrapper).
///
/// `overrides` are CLI/env arg overrides that take precedence over file defaults.
pub fn parse(input: &str, overrides: &HashMap<String, String>) -> Result<OrchFile, Vec<OrchError>> {
    let raw = parse_raw(input, 0)?;
    let merged = crate::merge::merge(vec![raw]);
    crate::resolve::resolve(merged, overrides, &[])
}

/// Parse multiple Orchfiles and merge them left-to-right.
///
/// Each entry is (filename, content). Files are merged in order.
pub fn parse_files(
    files: &[(&str, &str)],
    overrides: &HashMap<String, String>,
) -> Result<OrchFile, Vec<OrchError>> {
    let mut raws = Vec::new();
    for (i, (_name, content)) in files.iter().enumerate() {
        match parse_raw(content, i) {
            Ok(raw) => raws.push(raw),
            Err(mut errs) => {
                // Annotate errors with filename
                for err in &mut errs {
                    if let OrchError::Parse(pe) = err {
                        pe.file = Some(files[i].0.to_string());
                    }
                }
                return Err(errs);
            }
        }
    }
    let merged = crate::merge::merge(raws);
    let file_names: Vec<String> = files.iter().map(|(name, _)| name.to_string()).collect();
    crate::resolve::resolve(merged, overrides, &file_names)
}

// =========================================================================
// Raw parser: produces unexpanded intermediate representation
// =========================================================================

/// The set of directive names that CLEAR can target (list-type directives).
const CLEARABLE_DIRECTIVES: &[&str] =
    &["ENV", "ENV_FILE", "PUBLISH", "VOLUME", "REQUIRES", "AFTER"];

/// Parse an Orchfile into a raw (unexpanded) intermediate representation.
///
/// No variable expansion, no type parsing (bools/numbers/enums), no constraint
/// validation (C1-C4). Syntax errors (format, unknown directives, within-file
/// duplicates) are still caught.
pub fn parse_raw(input: &str, file_index: usize) -> Result<RawOrchFile, Vec<OrchError>> {
    let mut args: HashMap<String, String> = HashMap::new();
    let mut services: Vec<RawService> = Vec::new();
    let mut errors: Vec<OrchError> = Vec::new();
    let mut current_service: Option<RawService> = None;
    let mut seen_service_names: HashMap<String, usize> = HashMap::new();
    let mut orch_version_line: Option<usize> = None;

    // Pass 1: collect ARG defaults (no override application — deferred to resolve)
    for (line_num_0, raw_line) in input.lines().enumerate() {
        let line_num = line_num_0 + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (directive, value) = split_directive(line);
        if directive == "ARG" {
            if let Some(value) = value {
                match parse_arg(value, line_num) {
                    Ok((name, default)) => {
                        args.insert(name, default);
                    }
                    Err(e) => errors.push(e.into()),
                }
            } else {
                errors.push(ParseError::new(line_num, "ARG requires name=value").into());
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // Pass 2: parse directives into RawService fields (no expansion, no validation)
    for (line_num_0, raw_line) in input.lines().enumerate() {
        let line_num = line_num_0 + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (directive, value) = split_directive(line);

        if directive == "ARG" {
            continue;
        }

        // ORCH_VERSION: file-global, must precede any SERVICE, asserts spec compatibility.
        if directive == "ORCH_VERSION" {
            if current_service.is_some() || !services.is_empty() {
                errors.push(
                    ParseError::new(line_num, "ORCH_VERSION must appear before any SERVICE").into(),
                );
                continue;
            }
            if let Some(prev) = orch_version_line {
                errors.push(
                    ParseError::new(
                        line_num,
                        format!(
                            "duplicate ORCH_VERSION directive (first defined at line {})",
                            prev
                        ),
                    )
                    .into(),
                );
                continue;
            }
            orch_version_line = Some(line_num);
            match value {
                Some(v) if !v.is_empty() => {
                    if v != ORCH_VERSION {
                        errors.push(
                            ParseError::new(
                                line_num,
                                format!(
                                    "unsupported Orchfile version '{}' (this parser supports {})",
                                    v, ORCH_VERSION
                                ),
                            )
                            .into(),
                        );
                    }
                }
                _ => {
                    errors.push(
                        ParseError::new(line_num, "ORCH_VERSION requires a version value").into(),
                    );
                }
            }
            continue;
        }

        if directive == "SERVICE" {
            // Finalize previous service
            if let Some(svc) = current_service.take() {
                services.push(svc);
            }

            let name = match value {
                Some(n) => n.to_string(),
                None => {
                    errors.push(ParseError::new(line_num, "SERVICE requires a name").into());
                    continue;
                }
            };

            if let Err(e) = validate_service_name(&name, line_num) {
                errors.push(e.into());
                continue;
            }

            if let Some(prev_line) = seen_service_names.get(&name) {
                errors.push(
                    ParseError::new(
                        line_num,
                        format!(
                            "duplicate service name '{}' (first defined at line {})",
                            name, prev_line
                        ),
                    )
                    .into(),
                );
                continue;
            }
            seen_service_names.insert(name.clone(), line_num);

            current_service = Some(RawService::new(name, file_index));
            continue;
        }

        // CLEAR directive
        if directive == "CLEAR" {
            let svc = match current_service.as_mut() {
                Some(s) => s,
                None => {
                    errors.push(ParseError::new(line_num, "CLEAR outside of SERVICE block").into());
                    continue;
                }
            };

            let target = match value {
                Some(t) if !t.is_empty() => t,
                _ => {
                    errors.push(
                        ParseError::new(line_num, "CLEAR requires a target directive name").into(),
                    );
                    continue;
                }
            };

            if !CLEARABLE_DIRECTIVES.contains(&target) {
                // Check if it's a known scalar directive
                let known_scalars = [
                    "FROM",
                    "RUN",
                    "ENTRYPOINT",
                    "CMD",
                    "USER",
                    "STOP",
                    "RELOAD",
                    "WORKDIR",
                    "HEALTHCHECK",
                    "READINESS_TIMEOUT",
                    "ONESHOT",
                    "DISABLED",
                    "RECREATE",
                    "RESTART",
                    "RESTART_DELAY",
                    "START_LIMIT_BURST",
                    "START_LIMIT_INTERVAL",
                    "TIMEOUT_START",
                    "TIMEOUT_STOP",
                    "MEMORY",
                    "CPUS",
                    "CPU_QUOTA",
                    "LIMIT_NOFILE",
                    "LIMIT_NPROC",
                    "TASKS_MAX",
                    "IO_WEIGHT",
                    "STDOUT",
                    "STDERR",
                ];
                if known_scalars.contains(&target) {
                    errors.push(
                        ParseError::new(
                            line_num,
                            format!(
                                "CLEAR cannot target scalar directive '{}' (scalars override by last-wins)",
                                target
                            ),
                        )
                        .into(),
                    );
                } else {
                    errors.push(
                        ParseError::new(
                            line_num,
                            format!("CLEAR targets unknown directive '{}'", target),
                        )
                        .into(),
                    );
                }
                continue;
            }

            match target {
                "ENV" => {
                    svc.env.cleared = true;
                    svc.env.values.clear();
                }
                "ENV_FILE" => {
                    svc.env_files.cleared = true;
                    svc.env_files.values.clear();
                }
                "PUBLISH" => {
                    svc.publish.cleared = true;
                    svc.publish.values.clear();
                }
                "VOLUME" => {
                    svc.volumes.cleared = true;
                    svc.volumes.values.clear();
                }
                "REQUIRES" => {
                    svc.requires.cleared = true;
                    svc.requires.values.clear();
                }
                "AFTER" => {
                    svc.after.cleared = true;
                    svc.after.values.clear();
                }
                _ => unreachable!(),
            }
            continue;
        }

        // All other directives must be inside a SERVICE block
        let svc = match current_service.as_mut() {
            Some(s) => s,
            None => {
                errors.push(
                    ParseError::new(
                        line_num,
                        format!("directive '{}' outside of SERVICE block", directive),
                    )
                    .into(),
                );
                continue;
            }
        };

        // Store raw (unexpanded) value
        let raw_val = value.map(|v| v.to_string());

        match directive {
            // -- Execution mode --
            "FROM" => {
                let val = require_raw_value(&raw_val, "FROM", line_num, &mut errors);
                if let Some(v) = val {
                    if svc.from.is_some() {
                        errors.push(ParseError::new(line_num, "duplicate FROM directive").into());
                    } else {
                        svc.from = Some(v);
                        svc.from_source = Some(Source::new(line_num, file_index));
                    }
                }
            }
            "RUN" => {
                let val = require_raw_value(&raw_val, "RUN", line_num, &mut errors);
                if let Some(v) = val {
                    if svc.run.is_some() {
                        errors.push(ParseError::new(line_num, "duplicate RUN directive").into());
                    } else {
                        svc.run = Some(v);
                        svc.run_source = Some(Source::new(line_num, file_index));
                    }
                }
            }

            // -- Container-only --
            "ENTRYPOINT" => {
                svc.container_directives_used.push((
                    "ENTRYPOINT".to_string(),
                    line_num,
                    file_index,
                ));
                let val = require_raw_value(&raw_val, "ENTRYPOINT", line_num, &mut errors);
                if let Some(v) = val {
                    svc.entrypoint = Some(v);
                }
            }
            "CMD" => {
                svc.container_directives_used
                    .push(("CMD".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "CMD", line_num, &mut errors);
                if let Some(v) = val {
                    svc.cmd = Some(v);
                }
            }
            "PUBLISH" => {
                svc.container_directives_used
                    .push(("PUBLISH".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "PUBLISH", line_num, &mut errors);
                if let Some(v) = val {
                    // `[address:]host_port:container_port`. Last two groups are
                    // the ports; rsplit keeps ':'-bearing addresses (IPv6) intact.
                    let parts: Vec<&str> = v.split(':').collect();
                    if parts.len() < 2 {
                        errors.push(
                            ParseError::new(
                                line_num,
                                format!(
                                    "invalid PUBLISH format '{}', expected [address:]host_port:container_port",
                                    v
                                ),
                            )
                            .into(),
                        );
                    } else {
                        let container = parts[parts.len() - 1].to_string();
                        let host_port = parts[parts.len() - 2].to_string();
                        let address = parts[..parts.len() - 2].join(":");
                        svc.publish.values.push((address, host_port, container));
                    }
                }
            }
            "VOLUME" => {
                svc.container_directives_used
                    .push(("VOLUME".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "VOLUME", line_num, &mut errors);
                if let Some(v) = val {
                    let parts: Vec<&str> = v.splitn(2, ':').collect();
                    if parts.len() != 2 {
                        errors.push(
                            ParseError::new(
                                line_num,
                                format!(
                                    "invalid VOLUME format '{}', expected source:destination",
                                    v
                                ),
                            )
                            .into(),
                        );
                    } else {
                        svc.volumes
                            .values
                            .push((parts[0].to_string(), parts[1].to_string()));
                    }
                }
            }

            // -- Host-only --
            "USER" => {
                svc.host_directives_used
                    .push(("USER".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "USER", line_num, &mut errors);
                if let Some(v) = val {
                    svc.user = Some(v);
                }
            }
            "STOP" => {
                svc.host_directives_used
                    .push(("STOP".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "STOP", line_num, &mut errors);
                if let Some(v) = val {
                    svc.stop_command = Some(v);
                }
            }
            "RELOAD" => {
                svc.host_directives_used
                    .push(("RELOAD".to_string(), line_num, file_index));
                let val = require_raw_value(&raw_val, "RELOAD", line_num, &mut errors);
                if let Some(v) = val {
                    svc.reload_command = Some(v);
                }
            }

            // -- Common --
            "WORKDIR" => {
                let val = require_raw_value(&raw_val, "WORKDIR", line_num, &mut errors);
                if let Some(v) = val {
                    svc.workdir = Some(v);
                }
            }
            "ENV" => {
                let val = require_raw_value(&raw_val, "ENV", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_env_raw(&v, line_num) {
                        Ok((k, ev)) => {
                            svc.env.values.insert(k, ev);
                        }
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "ENV_FILE" => {
                let val = require_raw_value(&raw_val, "ENV_FILE", line_num, &mut errors);
                if let Some(v) = val {
                    svc.env_files.values.push(v);
                }
            }
            "REQUIRES" => {
                let val = require_raw_value(&raw_val, "REQUIRES", line_num, &mut errors);
                if let Some(v) = val {
                    for dep in v.split_whitespace() {
                        svc.requires.values.push(dep.to_string());
                    }
                }
            }
            "AFTER" => {
                let val = require_raw_value(&raw_val, "AFTER", line_num, &mut errors);
                if let Some(v) = val {
                    for dep in v.split_whitespace() {
                        svc.after.values.push(dep.to_string());
                    }
                }
            }
            "HEALTHCHECK" => {
                let val = require_raw_value(&raw_val, "HEALTHCHECK", line_num, &mut errors);
                if let Some(v) = val {
                    svc.healthcheck = Some(v);
                }
            }
            "READINESS_TIMEOUT" => {
                let val = require_raw_value(&raw_val, "READINESS_TIMEOUT", line_num, &mut errors);
                if let Some(v) = val {
                    svc.readiness_timeout = Some(v);
                }
            }
            "ONESHOT" => {
                let val = require_raw_value(&raw_val, "ONESHOT", line_num, &mut errors);
                if let Some(v) = val {
                    svc.oneshot = Some(v);
                }
            }
            "DISABLED" => {
                let val = require_raw_value(&raw_val, "DISABLED", line_num, &mut errors);
                if let Some(v) = val {
                    svc.disabled = Some(v);
                }
            }
            "RECREATE" => {
                let val = require_raw_value(&raw_val, "RECREATE", line_num, &mut errors);
                if let Some(v) = val {
                    svc.recreate = Some(v);
                }
            }
            "RESTART" => {
                let val = require_raw_value(&raw_val, "RESTART", line_num, &mut errors);
                if let Some(v) = val {
                    svc.restart = Some(v);
                }
            }
            "RESTART_DELAY" => {
                let val = require_raw_value(&raw_val, "RESTART_DELAY", line_num, &mut errors);
                if let Some(v) = val {
                    svc.restart_delay = Some(v);
                }
            }
            "START_LIMIT_BURST" => {
                let val = require_raw_value(&raw_val, "START_LIMIT_BURST", line_num, &mut errors);
                if let Some(v) = val {
                    svc.start_limit_burst = Some(v);
                }
            }
            "START_LIMIT_INTERVAL" => {
                let val =
                    require_raw_value(&raw_val, "START_LIMIT_INTERVAL", line_num, &mut errors);
                if let Some(v) = val {
                    svc.start_limit_interval = Some(v);
                }
            }
            "TIMEOUT_START" => {
                let val = require_raw_value(&raw_val, "TIMEOUT_START", line_num, &mut errors);
                if let Some(v) = val {
                    svc.timeout_start = Some(v);
                }
            }
            "TIMEOUT_STOP" => {
                let val = require_raw_value(&raw_val, "TIMEOUT_STOP", line_num, &mut errors);
                if let Some(v) = val {
                    svc.timeout_stop = Some(v);
                }
            }
            "MEMORY" => {
                let val = require_raw_value(&raw_val, "MEMORY", line_num, &mut errors);
                if let Some(v) = val {
                    svc.memory = Some(v);
                }
            }
            "CPUS" => {
                let val = require_raw_value(&raw_val, "CPUS", line_num, &mut errors);
                if let Some(v) = val {
                    svc.cpus = Some(v);
                }
            }
            "CPU_QUOTA" => {
                let val = require_raw_value(&raw_val, "CPU_QUOTA", line_num, &mut errors);
                if let Some(v) = val {
                    svc.cpu_quota = Some(v);
                }
            }
            "LIMIT_NOFILE" => {
                let val = require_raw_value(&raw_val, "LIMIT_NOFILE", line_num, &mut errors);
                if let Some(v) = val {
                    svc.limit_nofile = Some(v);
                }
            }
            "LIMIT_NPROC" => {
                let val = require_raw_value(&raw_val, "LIMIT_NPROC", line_num, &mut errors);
                if let Some(v) = val {
                    svc.limit_nproc = Some(v);
                }
            }
            "TASKS_MAX" => {
                let val = require_raw_value(&raw_val, "TASKS_MAX", line_num, &mut errors);
                if let Some(v) = val {
                    svc.tasks_max = Some(v);
                }
            }
            "IO_WEIGHT" => {
                let val = require_raw_value(&raw_val, "IO_WEIGHT", line_num, &mut errors);
                if let Some(v) = val {
                    svc.io_weight = Some(v);
                }
            }
            "STDOUT" => {
                let val = require_raw_value(&raw_val, "STDOUT", line_num, &mut errors);
                if let Some(v) = val {
                    svc.stdout = Some(v);
                }
            }
            "STDERR" => {
                let val = require_raw_value(&raw_val, "STDERR", line_num, &mut errors);
                if let Some(v) = val {
                    svc.stderr = Some(v);
                }
            }

            _ => {
                errors.push(
                    ParseError::new(line_num, format!("unknown directive '{}'", directive)).into(),
                );
            }
        }
    }

    // Finalize last service
    if let Some(svc) = current_service.take() {
        services.push(svc);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(RawOrchFile { args, services })
}

/// Helper to require a raw (unexpanded) value for a directive.
fn require_raw_value(
    raw_val: &Option<String>,
    directive: &str,
    line: usize,
    errors: &mut Vec<OrchError>,
) -> Option<String> {
    match raw_val {
        Some(v) if !v.is_empty() => Some(v.clone()),
        _ => {
            errors.push(ParseError::new(line, format!("{} requires a value", directive)).into());
            None
        }
    }
}

/// Parse raw ENV "KEY=value" without variable expansion.
fn parse_env_raw(value: &str, line: usize) -> Result<(String, String), ParseError> {
    let eq_pos = value.find('=').ok_or_else(|| {
        ParseError::new(
            line,
            format!("invalid ENV format '{}', expected KEY=value", value),
        )
    })?;
    let key = value[..eq_pos].to_string();
    let val = value[eq_pos + 1..].to_string();
    if key.is_empty() {
        return Err(ParseError::new(line, "ENV key cannot be empty"));
    }
    Ok((key, val))
}

// =========================================================================
// Utility functions
// =========================================================================

/// Split a line into (DIRECTIVE, optional value).
fn split_directive(line: &str) -> (&str, Option<&str>) {
    match line.find(char::is_whitespace) {
        Some(pos) => (&line[..pos], Some(line[pos..].trim_start())),
        None => (line, None),
    }
}

/// Parse "name=default" from ARG value.
fn parse_arg(value: &str, line: usize) -> Result<(String, String), ParseError> {
    let eq_pos = value.find('=').ok_or_else(|| {
        ParseError::new(
            line,
            format!("invalid ARG format '{}', expected name=value", value),
        )
    })?;
    let name = value[..eq_pos].to_string();
    let default = value[eq_pos + 1..].to_string();
    if name.is_empty() {
        return Err(ParseError::new(line, "ARG name cannot be empty"));
    }
    Ok((name, default))
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn parse_ok(input: &str) -> OrchFile {
        parse(input, &HashMap::new()).expect("parse should succeed")
    }

    fn parse_ok_with_args(input: &str, overrides: &HashMap<String, String>) -> OrchFile {
        parse(input, overrides).expect("parse should succeed")
    }

    fn parse_err(input: &str) -> Vec<OrchError> {
        parse(input, &HashMap::new()).expect_err("parse should fail")
    }

    // =========================================================================
    // Tokenizer: comments and blank lines
    // =========================================================================

    #[test]
    fn test_tokenizer__ignores_comments_and_blank_lines() {
        let input = r#"
# This is a comment
ARG port=5432

# Another comment
SERVICE db
FROM postgres:15
    "#;
        let orch = parse_ok(input);
        assert_eq!(orch.services.len(), 1);
        assert_eq!(orch.args.get("port").unwrap(), "5432");
    }

    #[test]
    fn test_tokenizer__empty_input() {
        let orch = parse_ok("");
        assert!(orch.services.is_empty());
        assert!(orch.args.is_empty());
    }

    #[test]
    fn test_tokenizer__only_comments() {
        let orch = parse_ok("# comment\n# another\n");
        assert!(orch.services.is_empty());
    }

    // =========================================================================
    // ARG parsing
    // =========================================================================

    #[test]
    fn test_arg__basic_default() {
        let orch = parse_ok("ARG port=5432\nSERVICE db\nFROM postgres:15\n");
        assert_eq!(orch.args.get("port").unwrap(), "5432");
    }

    #[test]
    fn test_arg__multiple_args() {
        let input = "ARG a=1\nARG b=2\nARG c=hello\nSERVICE x\nFROM img\n";
        let orch = parse_ok(input);
        assert_eq!(orch.args.len(), 3);
        assert_eq!(orch.args["a"], "1");
        assert_eq!(orch.args["b"], "2");
        assert_eq!(orch.args["c"], "hello");
    }

    #[test]
    fn test_arg__empty_value() {
        let orch = parse_ok("ARG key=\nSERVICE x\nFROM img\n");
        assert_eq!(orch.args.get("key").unwrap(), "");
    }

    #[test]
    fn test_arg__cli_override() {
        let mut overrides = HashMap::new();
        overrides.insert("port".to_string(), "9999".to_string());
        let orch = parse_ok_with_args("ARG port=5432\nSERVICE db\nFROM postgres:15\n", &overrides);
        assert_eq!(orch.args["port"], "9999");
    }

    #[test]
    fn test_arg__missing_equals() {
        let errors = parse_err("ARG noequals\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_arg__missing_value_entirely() {
        let errors = parse_err("ARG\n");
        assert!(!errors.is_empty());
    }

    // =========================================================================
    // Variable expansion
    // =========================================================================

    #[test]
    fn test_expansion__arg_in_publish() {
        let input = "ARG port=5433\nSERVICE db\nFROM postgres:15\nPUBLISH ${port}:5432\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[0].publish[0].host, 5433);
    }

    #[test]
    fn test_expansion__arg_in_env() {
        let input = "ARG db=orch\nSERVICE app\nRUN echo hi\nENV DB_NAME=${db}\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[0].env["DB_NAME"], "orch");
    }

    #[test]
    fn test_expansion__unresolved_builtin_preserved() {
        let input = "SERVICE app\nRUN echo hi\nSTDOUT ${ORCH_STATE_DIR}/logs/app.log\n";
        let orch = parse_ok(input);
        assert_eq!(
            orch.services[0].logging.stdout.as_ref().unwrap(),
            "${ORCH_STATE_DIR}/logs/app.log"
        );
    }

    #[test]
    fn test_expansion__override_in_image() {
        let mut overrides = HashMap::new();
        overrides.insert("tag".to_string(), "16".to_string());
        let input = "ARG tag=15\nSERVICE db\nFROM postgres:${tag}\n";
        let orch = parse_ok_with_args(input, &overrides);
        assert_eq!(orch.services[0].image.as_ref().unwrap(), "postgres:16");
    }

    // =========================================================================
    // SERVICE declaration and naming
    // =========================================================================

    #[test]
    fn test_service_name__valid_names() {
        for name in &["db", "my-app", "a1", "a-b-c-123"] {
            let input = format!("SERVICE {}\nFROM img\n", name);
            let orch = parse_ok(&input);
            assert_eq!(orch.services[0].name, *name);
        }
    }

    #[test]
    fn test_service_name__starts_with_digit() {
        let errors = parse_err("SERVICE 1bad\nFROM img\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_service_name__starts_with_hyphen() {
        let errors = parse_err("SERVICE -bad\nFROM img\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_service_name__uppercase() {
        let errors = parse_err("SERVICE MyApp\nFROM img\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_service_name__too_long() {
        let name = "a".repeat(64);
        let input = format!("SERVICE {}\nFROM img\n", name);
        let errors = parse_err(&input);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_service_name__max_length_ok() {
        let name = "a".repeat(63);
        let input = format!("SERVICE {}\nFROM img\n", name);
        let orch = parse_ok(&input);
        assert_eq!(orch.services[0].name.len(), 63);
    }

    #[test]
    fn test_service_name__duplicate() {
        let errors = parse_err("SERVICE db\nFROM img\nSERVICE db\nFROM img2\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_service_name__missing() {
        let errors = parse_err("SERVICE\nFROM img\n");
        assert!(!errors.is_empty());
    }

    // =========================================================================
    // C1: FROM XOR RUN
    // =========================================================================

    #[test]
    fn test_c1__from_only() {
        let orch = parse_ok("SERVICE db\nFROM postgres:15\n");
        assert_eq!(orch.services[0].mode, ServiceMode::Container);
        assert_eq!(orch.services[0].image.as_ref().unwrap(), "postgres:15");
    }

    #[test]
    fn test_c1__run_only() {
        let orch = parse_ok("SERVICE app\nRUN python manage.py runserver\n");
        assert_eq!(orch.services[0].mode, ServiceMode::Host);
        assert_eq!(
            orch.services[0].run_command.as_ref().unwrap(),
            "python manage.py runserver"
        );
    }

    #[test]
    fn test_c1__both_from_and_run() {
        let errors = parse_err("SERVICE bad\nFROM img\nRUN cmd\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C1")));
    }

    #[test]
    fn test_c1__neither_from_nor_run() {
        let errors = parse_err("SERVICE empty\nENV FOO=bar\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C1")));
    }

    // =========================================================================
    // C2: Container-only directives
    // =========================================================================

    #[test]
    fn test_c2__entrypoint_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nENTRYPOINT /bin/sh\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C2")));
    }

    #[test]
    fn test_c2__cmd_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nCMD args\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C2")));
    }

    #[test]
    fn test_c2__publish_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nPUBLISH 80:80\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C2")));
    }

    #[test]
    fn test_c2__volume_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nVOLUME data:/data\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C2")));
    }

    // =========================================================================
    // C3: Host-only directives
    // =========================================================================

    #[test]
    fn test_c3__user_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nUSER postgres\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C3")));
    }

    #[test]
    fn test_c3__stop_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nSTOP kill -9\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C3")));
    }

    #[test]
    fn test_c3__reload_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nRELOAD nginx -s reload\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C3")));
    }

    // =========================================================================
    // C4: Dependency acyclicity
    // =========================================================================

    #[test]
    fn test_c4__simple_cycle() {
        let input = "SERVICE a\nFROM img\nREQUIRES b\nSERVICE b\nFROM img\nREQUIRES a\n";
        let errors = parse_err(input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("cycle")));
    }

    #[test]
    fn test_c4__self_cycle() {
        let input = "SERVICE a\nFROM img\nREQUIRES a\n";
        let errors = parse_err(input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("cycle")));
    }

    #[test]
    fn test_c4__three_node_cycle() {
        let input = "\
SERVICE a\nFROM img\nREQUIRES b\n\
SERVICE b\nFROM img\nREQUIRES c\n\
SERVICE c\nFROM img\nREQUIRES a\n";
        let errors = parse_err(input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("cycle")));
    }

    #[test]
    fn test_c4__after_cycle() {
        let input = "SERVICE a\nFROM img\nAFTER b\nSERVICE b\nFROM img\nAFTER a\n";
        let errors = parse_err(input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("cycle")));
    }

    #[test]
    fn test_c4__valid_dag() {
        let input = "\
SERVICE db\nFROM postgres:15\n\
SERVICE cache\nFROM redis:6\n\
SERVICE app\nRUN python run\nREQUIRES db cache\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services.len(), 3);
    }

    // =========================================================================
    // Container-mode directives
    // =========================================================================

    #[test]
    fn test_container__entrypoint() {
        let orch = parse_ok("SERVICE x\nFROM img\nENTRYPOINT /bin/sh\n");
        assert_eq!(orch.services[0].entrypoint.as_ref().unwrap(), "/bin/sh");
    }

    #[test]
    fn test_container__cmd() {
        let orch = parse_ok("SERVICE x\nFROM img\nCMD -c config.conf\n");
        assert_eq!(orch.services[0].cmd.as_ref().unwrap(), "-c config.conf");
    }

    #[test]
    fn test_container__publish_multiple() {
        let input = "SERVICE x\nFROM img\nPUBLISH 80:80\nPUBLISH 443:443\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[0].publish.len(), 2);
        assert_eq!(orch.services[0].publish[0].host, 80);
        assert_eq!(orch.services[0].publish[1].host, 443);
    }

    #[test]
    fn test_container__volume_named() {
        let orch = parse_ok("SERVICE x\nFROM img\nVOLUME data:/var/data\n");
        assert!(orch.services[0].volumes[0].is_named);
        assert_eq!(orch.services[0].volumes[0].source, "data");
    }

    #[test]
    fn test_container__volume_host_path() {
        let orch = parse_ok("SERVICE x\nFROM img\nVOLUME /host:/container\n");
        assert!(!orch.services[0].volumes[0].is_named);
    }

    #[test]
    fn test_container__volume_with_var() {
        let orch = parse_ok("SERVICE x\nFROM img\nVOLUME ${ORCH_DATA}/pg:/var/lib/data\n");
        assert!(!orch.services[0].volumes[0].is_named);
        assert_eq!(orch.services[0].volumes[0].source, "${ORCH_DATA}/pg");
    }

    #[test]
    fn test_container__recreate_always() {
        let orch = parse_ok("SERVICE x\nFROM img\nRECREATE always\n");
        assert_eq!(orch.services[0].recreate, RecreatePolicy::Always);
    }

    #[test]
    fn test_container__recreate_default() {
        let orch = parse_ok("SERVICE x\nFROM img\n");
        assert_eq!(orch.services[0].recreate, RecreatePolicy::Never);
    }

    // =========================================================================
    // Host-mode directives
    // =========================================================================

    #[test]
    fn test_host__user() {
        let orch = parse_ok("SERVICE x\nRUN cmd\nUSER postgres\n");
        assert_eq!(orch.services[0].user.as_ref().unwrap(), "postgres");
    }

    #[test]
    fn test_host__stop_command() {
        let orch = parse_ok("SERVICE x\nRUN cmd\nSTOP kill -SIGTERM $PID\n");
        assert_eq!(
            orch.services[0].stop_command.as_ref().unwrap(),
            "kill -SIGTERM $PID"
        );
    }

    #[test]
    fn test_host__reload_command() {
        let orch = parse_ok("SERVICE x\nRUN cmd\nRELOAD nginx -s reload\n");
        assert_eq!(
            orch.services[0].reload_command.as_ref().unwrap(),
            "nginx -s reload"
        );
    }

    // =========================================================================
    // Common directives
    // =========================================================================

    #[test]
    fn test_common__workdir() {
        let orch = parse_ok("SERVICE x\nFROM img\nWORKDIR /app\n");
        assert_eq!(orch.services[0].workdir.as_ref().unwrap(), "/app");
    }

    #[test]
    fn test_common__env_multiple() {
        let input = "SERVICE x\nFROM img\nENV A=1\nENV B=2\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[0].env["A"], "1");
        assert_eq!(orch.services[0].env["B"], "2");
    }

    #[test]
    fn test_common__env_with_equals_in_value() {
        let orch =
            parse_ok("SERVICE x\nFROM img\nENV URL=postgres://host:5432/db?sslmode=require\n");
        assert_eq!(
            orch.services[0].env["URL"],
            "postgres://host:5432/db?sslmode=require"
        );
    }

    #[test]
    fn test_common__env_file() {
        let orch = parse_ok("SERVICE x\nFROM img\nENV_FILE /path/.env\n");
        assert_eq!(orch.services[0].env_files[0], "/path/.env");
    }

    #[test]
    fn test_common__requires() {
        let input = "SERVICE db\nFROM img\nSERVICE app\nRUN cmd\nREQUIRES db\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[1].requires, vec!["db"]);
    }

    #[test]
    fn test_common__requires_multiple_on_one_line() {
        let input = "SERVICE db\nFROM img\nSERVICE cache\nFROM img\nSERVICE app\nRUN cmd\nREQUIRES db cache\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[2].requires, vec!["db", "cache"]);
    }

    #[test]
    fn test_common__after() {
        let input = "SERVICE db\nFROM img\nSERVICE app\nRUN cmd\nAFTER db\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[1].after, vec!["db"]);
    }

    #[test]
    fn test_common__healthcheck_command() {
        let orch = parse_ok("SERVICE x\nFROM img\nHEALTHCHECK pg_isready -h localhost\n");
        assert_eq!(
            orch.services[0].healthcheck.as_ref().unwrap(),
            "pg_isready -h localhost"
        );
    }

    #[test]
    fn test_common__healthcheck_http() {
        let orch = parse_ok("SERVICE x\nRUN cmd\nHEALTHCHECK http://localhost:8000/health\n");
        assert!(
            orch.services[0]
                .healthcheck
                .as_ref()
                .unwrap()
                .starts_with("http://")
        );
    }

    #[test]
    fn test_common__readiness_timeout() {
        let orch = parse_ok("SERVICE x\nFROM img\nREADINESS_TIMEOUT 120s\n");
        assert_eq!(orch.services[0].readiness_timeout.as_ref().unwrap(), "120s");
    }

    #[test]
    fn test_common__oneshot() {
        let orch = parse_ok("SERVICE x\nFROM img\nONESHOT true\n");
        assert!(orch.services[0].oneshot);
    }

    #[test]
    fn test_common__disabled() {
        let orch = parse_ok("SERVICE x\nFROM img\nDISABLED true\n");
        assert!(orch.services[0].disabled);
    }

    #[test]
    fn test_common__disabled_default() {
        let orch = parse_ok("SERVICE x\nFROM img\n");
        assert!(!orch.services[0].disabled);
    }

    // =========================================================================
    // Restart policy
    // =========================================================================

    #[test]
    fn test_restart__on_failure() {
        let orch = parse_ok("SERVICE x\nFROM img\nRESTART on-failure\n");
        assert_eq!(orch.services[0].restart.policy, RestartPolicy::OnFailure);
    }

    #[test]
    fn test_restart__always() {
        let orch = parse_ok("SERVICE x\nFROM img\nRESTART always\n");
        assert_eq!(orch.services[0].restart.policy, RestartPolicy::Always);
    }

    #[test]
    fn test_restart__delay() {
        let orch = parse_ok("SERVICE x\nFROM img\nRESTART_DELAY 5s\n");
        assert_eq!(orch.services[0].restart.delay.as_ref().unwrap(), "5s");
    }

    #[test]
    fn test_restart__start_limit_burst() {
        let orch = parse_ok("SERVICE x\nFROM img\nSTART_LIMIT_BURST 5\n");
        assert_eq!(orch.services[0].restart.start_limit_burst.unwrap(), 5);
    }

    #[test]
    fn test_restart__start_limit_interval() {
        let orch = parse_ok("SERVICE x\nFROM img\nSTART_LIMIT_INTERVAL 10s\n");
        assert_eq!(
            orch.services[0]
                .restart
                .start_limit_interval
                .as_ref()
                .unwrap(),
            "10s"
        );
    }

    #[test]
    fn test_restart__invalid_policy() {
        let errors = parse_err("SERVICE x\nFROM img\nRESTART maybe\n");
        assert!(!errors.is_empty());
    }

    // =========================================================================
    // Timeouts
    // =========================================================================

    #[test]
    fn test_timeout__start() {
        let orch = parse_ok("SERVICE x\nFROM img\nTIMEOUT_START 30s\n");
        assert_eq!(orch.services[0].timeouts.start.as_ref().unwrap(), "30s");
    }

    #[test]
    fn test_timeout__stop() {
        let orch = parse_ok("SERVICE x\nFROM img\nTIMEOUT_STOP 10s\n");
        assert_eq!(orch.services[0].timeouts.stop.as_ref().unwrap(), "10s");
    }

    // =========================================================================
    // Resource limits
    // =========================================================================

    #[test]
    fn test_resources__memory() {
        let orch = parse_ok("SERVICE x\nFROM img\nMEMORY 4G\n");
        assert_eq!(orch.services[0].resources.memory.as_ref().unwrap(), "4G");
    }

    #[test]
    fn test_resources__cpus() {
        let orch = parse_ok("SERVICE x\nFROM img\nCPUS 2\n");
        assert_eq!(orch.services[0].resources.cpus.unwrap(), 2.0);
    }

    #[test]
    fn test_resources__cpus_fractional() {
        let orch = parse_ok("SERVICE x\nFROM img\nCPUS 0.5\n");
        assert_eq!(orch.services[0].resources.cpus.unwrap(), 0.5);
    }

    #[test]
    fn test_resources__cpu_quota() {
        let orch = parse_ok("SERVICE x\nFROM img\nCPU_QUOTA 200%\n");
        assert_eq!(
            orch.services[0].resources.cpu_quota.as_ref().unwrap(),
            "200%"
        );
    }

    #[test]
    fn test_resources__limit_nofile() {
        let orch = parse_ok("SERVICE x\nFROM img\nLIMIT_NOFILE 65536\n");
        assert_eq!(orch.services[0].resources.limit_nofile.unwrap(), 65536);
    }

    #[test]
    fn test_resources__limit_nproc() {
        let orch = parse_ok("SERVICE x\nFROM img\nLIMIT_NPROC 4096\n");
        assert_eq!(orch.services[0].resources.limit_nproc.unwrap(), 4096);
    }

    #[test]
    fn test_resources__tasks_max() {
        let orch = parse_ok("SERVICE x\nFROM img\nTASKS_MAX 4096\n");
        assert_eq!(orch.services[0].resources.tasks_max.unwrap(), 4096);
    }

    #[test]
    fn test_resources__io_weight() {
        let orch = parse_ok("SERVICE x\nFROM img\nIO_WEIGHT 500\n");
        assert_eq!(orch.services[0].resources.io_weight.unwrap(), 500);
    }

    #[test]
    fn test_resources__io_weight_out_of_range() {
        let errors = parse_err("SERVICE x\nFROM img\nIO_WEIGHT 9\n");
        assert!(!errors.is_empty());

        let errors = parse_err("SERVICE x\nFROM img\nIO_WEIGHT 1001\n");
        assert!(!errors.is_empty());
    }

    // =========================================================================
    // Logging
    // =========================================================================

    #[test]
    fn test_logging__stdout() {
        let orch = parse_ok("SERVICE x\nFROM img\nSTDOUT /var/log/x.log\n");
        assert_eq!(
            orch.services[0].logging.stdout.as_ref().unwrap(),
            "/var/log/x.log"
        );
    }

    #[test]
    fn test_logging__stderr() {
        let orch = parse_ok("SERVICE x\nFROM img\nSTDERR /var/log/x.err\n");
        assert_eq!(
            orch.services[0].logging.stderr.as_ref().unwrap(),
            "/var/log/x.err"
        );
    }

    // =========================================================================
    // REQUIRES references validation
    // =========================================================================

    #[test]
    fn test_requires__unknown_service() {
        let errors = parse_err("SERVICE app\nRUN cmd\nREQUIRES nonexistent\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("unknown service")));
    }

    #[test]
    fn test_after__unknown_service_allowed() {
        // AFTER references are soft — missing targets are OK per spec
        let orch = parse_ok("SERVICE app\nRUN cmd\nAFTER nonexistent\n");
        assert_eq!(orch.services[0].after, vec!["nonexistent"]);
    }

    // =========================================================================
    // Unknown directive
    // =========================================================================

    #[test]
    fn test_unknown_directive() {
        let errors = parse_err("SERVICE x\nFROM img\nFOOBAR baz\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("unknown directive")));
    }

    // =========================================================================
    // Directive outside service block
    // =========================================================================

    #[test]
    fn test_directive_outside_service() {
        let errors = parse_err("FROM img\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("outside of SERVICE block")));
    }

    // =========================================================================
    // Full example from spec
    // =========================================================================

    #[test]
    fn test_full_spec_example() {
        let input = r#"
ARG prometheus_port=9091
ARG prometheus_memory=2G
ARG grafana_port=3001

SERVICE prometheus
FROM prom/prometheus:v2.47.0
MEMORY ${prometheus_memory}
CPUS 2
PUBLISH ${prometheus_port}:9090
VOLUME prometheus-data:/prometheus
ENV TSDB_PATH=/prometheus
ENV TSDB_RETENTION=15d
HEALTHCHECK promtool check health
RESTART on-failure
RESTART_DELAY 5s

SERVICE loki
FROM grafana/loki:2.9.2
MEMORY 1G
CPUS 1
PUBLISH 127.0.0.1:3100:3100
RECREATE always
HEALTHCHECK wget -q --spider http://localhost:3100/ready
RESTART always

SERVICE grafana
RUN grafana server --http-addr 0.0.0.0:${grafana_port}
WORKDIR /var/lib/grafana
ENV GF_PATHS_PROVISIONING=/etc/grafana/provisioning
ENV_FILE ${ORCH_PROJECT}/.env.local
REQUIRES prometheus loki
AFTER localstack
HEALTHCHECK http://localhost:${grafana_port}/api/health
RESTART on-failure
RESTART_DELAY 2s
MEMORY 2G
LIMIT_NOFILE 65536
TIMEOUT_START 60s

SERVICE config-check
FROM prom/prometheus:v2.47.0
CMD promtool check config /etc/prometheus/prometheus.yml
REQUIRES prometheus
ONESHOT true
"#;
        let orch = parse_ok(input);

        assert_eq!(orch.services.len(), 4);

        // prometheus
        let prom = &orch.services[0];
        assert_eq!(prom.name, "prometheus");
        assert_eq!(prom.mode, ServiceMode::Container);
        assert_eq!(prom.image.as_ref().unwrap(), "prom/prometheus:v2.47.0");
        assert_eq!(prom.resources.memory.as_ref().unwrap(), "2G");
        assert_eq!(prom.resources.cpus.unwrap(), 2.0);
        assert_eq!(prom.publish[0].host, 9091);
        assert_eq!(prom.publish[0].container, 9090);
        assert!(prom.volumes[0].is_named);
        assert_eq!(prom.env["TSDB_RETENTION"], "15d");
        assert_eq!(prom.restart.policy, RestartPolicy::OnFailure);
        assert_eq!(prom.restart.delay.as_ref().unwrap(), "5s");

        // loki
        let loki = &orch.services[1];
        assert_eq!(loki.name, "loki");
        assert_eq!(loki.recreate, RecreatePolicy::Always);
        assert_eq!(loki.restart.policy, RestartPolicy::Always);

        // grafana
        let gf = &orch.services[2];
        assert_eq!(gf.name, "grafana");
        assert_eq!(gf.mode, ServiceMode::Host);
        assert_eq!(
            gf.run_command.as_ref().unwrap(),
            "grafana server --http-addr 0.0.0.0:3001"
        );
        assert_eq!(gf.workdir.as_ref().unwrap(), "/var/lib/grafana");
        assert_eq!(gf.requires, vec!["prometheus", "loki"]);
        assert_eq!(gf.after, vec!["localstack"]);
        assert_eq!(gf.resources.limit_nofile.unwrap(), 65536);
        assert_eq!(gf.timeouts.start.as_ref().unwrap(), "60s");

        // config-check
        let chk = &orch.services[3];
        assert_eq!(chk.name, "config-check");
        assert!(chk.oneshot);
        assert_eq!(chk.requires, vec!["prometheus"]);
    }

    // =========================================================================
    // JSON serialization
    // =========================================================================

    #[test]
    fn test_json_serialization() {
        let orch = parse_ok("SERVICE db\nFROM postgres:15\nPUBLISH 5432:5432\n");
        let json = serde_json::to_string_pretty(&orch).unwrap();
        assert!(json.contains("\"version\": \"1.0.0-rc\""));
        assert!(json.contains("\"name\": \"db\""));
        assert!(json.contains("\"mode\": \"container\""));
        assert!(json.contains("\"host\": 5432"));
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_edge__duplicate_from() {
        let errors = parse_err("SERVICE x\nFROM img1\nFROM img2\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("duplicate FROM")));
    }

    #[test]
    fn test_edge__duplicate_run() {
        let errors = parse_err("SERVICE x\nRUN cmd1\nRUN cmd2\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("duplicate RUN")));
    }

    #[test]
    fn test_edge__multiple_services() {
        let input = "SERVICE a\nFROM img\nSERVICE b\nRUN cmd\nSERVICE c\nFROM img2\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services.len(), 3);
        assert_eq!(orch.services[0].name, "a");
        assert_eq!(orch.services[1].name, "b");
        assert_eq!(orch.services[2].name, "c");
    }

    #[test]
    fn test_edge__publish_invalid_format() {
        let errors = parse_err("SERVICE x\nFROM img\nPUBLISH 80\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_edge__publish_non_numeric() {
        let errors = parse_err("SERVICE x\nFROM img\nPUBLISH abc:80\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_edge__volume_no_colon() {
        let errors = parse_err("SERVICE x\nFROM img\nVOLUME /just/a/path\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_edge__cpus_non_numeric() {
        let errors = parse_err("SERVICE x\nFROM img\nCPUS abc\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_edge__oneshot_invalid() {
        let errors = parse_err("SERVICE x\nFROM img\nONESHOT yes\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_edge__indented_directives() {
        // Spec says line-oriented and we trim — indented directives should work
        let input = "SERVICE db\n  FROM postgres:15\n  PUBLISH 5432:5432\n";
        let orch = parse_ok(input);
        assert_eq!(orch.services[0].publish[0].host, 5432);
    }

    // =========================================================================
    // Multi-file integration tests (parse_files through full pipeline)
    // =========================================================================

    fn parse_files_ok(files: &[(&str, &str)]) -> OrchFile {
        parse_files(files, &HashMap::new()).expect("parse_files should succeed")
    }

    fn parse_files_ok_with_args(
        files: &[(&str, &str)],
        overrides: &HashMap<String, String>,
    ) -> OrchFile {
        parse_files(files, overrides).expect("parse_files should succeed")
    }

    fn parse_files_err(files: &[(&str, &str)]) -> Vec<OrchError> {
        parse_files(files, &HashMap::new()).expect_err("parse_files should fail")
    }

    #[test]
    fn test_multifile__scalar_override() {
        let base = r#"
SERVICE web
  FROM nginx:1.24
  WORKDIR /app
    "#;
        let overlay = r#"
SERVICE web
  FROM nginx:1.25
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].image.as_deref(), Some("nginx:1.25"));
        assert_eq!(orch.services[0].workdir.as_deref(), Some("/app"));
    }

    #[test]
    fn test_multifile__env_merge() {
        let base = r#"
SERVICE web
  FROM nginx
  ENV MODE=production
  ENV PORT=80
    "#;
        let overlay = r#"
SERVICE web
  ENV MODE=staging
  ENV DEBUG=true
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].env.get("MODE").unwrap(), "staging");
        assert_eq!(orch.services[0].env.get("PORT").unwrap(), "80");
        assert_eq!(orch.services[0].env.get("DEBUG").unwrap(), "true");
    }

    #[test]
    fn test_multifile__publish_merge_by_container_port() {
        let base = r#"
SERVICE web
  FROM nginx
  PUBLISH 8080:80
  PUBLISH 8443:443
    "#;
        let overlay = r#"
SERVICE web
  PUBLISH 9090:80
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].publish.len(), 2);
        let p80 = orch.services[0]
            .publish
            .iter()
            .find(|p| p.container == 80)
            .unwrap();
        assert_eq!(p80.host, 9090);
        let p443 = orch.services[0]
            .publish
            .iter()
            .find(|p| p.container == 443)
            .unwrap();
        assert_eq!(p443.host, 8443);
    }

    #[test]
    fn test_multifile__volume_merge_by_dest() {
        let base = r#"
SERVICE db
  FROM postgres:15
  VOLUME pgdata:/var/lib/postgresql/data
  VOLUME ./config:/etc/postgresql
    "#;
        let overlay = r#"
SERVICE db
  VOLUME ./local-config:/etc/postgresql
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].volumes.len(), 2);
        let etc = orch.services[0]
            .volumes
            .iter()
            .find(|v| v.destination == "/etc/postgresql")
            .unwrap();
        assert_eq!(etc.source, "./local-config");
        assert!(!etc.is_named);
    }

    #[test]
    fn test_multifile__requires_append_dedup() {
        let base = r#"
SERVICE db
  FROM postgres:15

SERVICE redis
  FROM redis:7

SERVICE cache
  FROM memcached

SERVICE web
  FROM nginx
  REQUIRES db redis
    "#;
        let overlay = r#"
SERVICE web
  REQUIRES redis cache
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        let web = orch.services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.requires, &["db", "redis", "cache"]);
    }

    #[test]
    fn test_multifile__clear_env() {
        let base = r#"
SERVICE web
  FROM nginx
  ENV OLD_KEY=old_value
  ENV KEEP=this
    "#;
        let overlay = r#"
SERVICE web
  CLEAR ENV
  ENV FRESH=new_value
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert!(!orch.services[0].env.contains_key("OLD_KEY"));
        assert!(!orch.services[0].env.contains_key("KEEP"));
        assert_eq!(orch.services[0].env.get("FRESH").unwrap(), "new_value");
    }

    #[test]
    fn test_multifile__clear_requires() {
        let base = r#"
SERVICE db
  FROM postgres:15

SERVICE redis
  FROM redis:7

SERVICE cache
  FROM memcached

SERVICE web
  FROM nginx
  REQUIRES db redis
    "#;
        let overlay = r#"
SERVICE web
  CLEAR REQUIRES
  REQUIRES cache
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        let web = orch.services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.requires, &["cache"]);
    }

    #[test]
    fn test_multifile__clear_publish() {
        let base = r#"
SERVICE web
  FROM nginx
  PUBLISH 8080:80
  PUBLISH 8443:443
    "#;
        let overlay = r#"
SERVICE web
  CLEAR PUBLISH
  PUBLISH 9090:80
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].publish.len(), 1);
        assert_eq!(orch.services[0].publish[0].host, 9090);
        assert_eq!(orch.services[0].publish[0].container, 80);
    }

    #[test]
    fn test_multifile__mode_switch_from_to_run() {
        let base = r#"
SERVICE svc
  FROM nginx:latest
  ENTRYPOINT /docker-entrypoint.sh
  PUBLISH 8080:80
    "#;
        let overlay = r#"
SERVICE svc
  RUN /usr/local/bin/myapp
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].mode, ServiceMode::Host);
        assert_eq!(
            orch.services[0].run_command.as_deref(),
            Some("/usr/local/bin/myapp")
        );
        assert!(orch.services[0].image.is_none());
        assert!(orch.services[0].entrypoint.is_none());
        assert!(orch.services[0].publish.is_empty());
    }

    #[test]
    fn test_multifile__mode_switch_run_to_from() {
        let base = r#"
SERVICE svc
  RUN /usr/local/bin/myapp
  USER nobody
  STOP kill $MAINPID
    "#;
        let overlay = r#"
SERVICE svc
  FROM myapp:latest
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].mode, ServiceMode::Container);
        assert_eq!(orch.services[0].image.as_deref(), Some("myapp:latest"));
        assert!(orch.services[0].run_command.is_none());
        assert!(orch.services[0].user.is_none());
        assert!(orch.services[0].stop_command.is_none());
    }

    #[test]
    fn test_multifile__new_service_in_overlay() {
        let base = r#"
SERVICE web
  FROM nginx
    "#;
        let overlay = r#"
SERVICE cache
  FROM redis:7
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services.len(), 2);
        assert_eq!(orch.services[0].name, "web");
        assert_eq!(orch.services[1].name, "cache");
    }

    #[test]
    fn test_multifile__arg_merge_and_expansion() {
        let base = r#"
ARG port=8080

SERVICE web
  FROM nginx
  PUBLISH ${port}:80
    "#;
        let overlay = r#"
ARG port=9090

SERVICE web
  ENV APP_PORT=${port}
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        // ARG last wins: port=9090
        assert_eq!(orch.args.get("port").unwrap(), "9090");
        assert_eq!(orch.services[0].publish[0].host, 9090);
        assert_eq!(orch.services[0].env.get("APP_PORT").unwrap(), "9090");
    }

    #[test]
    fn test_multifile__arg_cli_override_across_files() {
        let base = r#"
ARG port=8080

SERVICE web
  FROM nginx
  PUBLISH ${port}:80
    "#;
        let overlay = r#"
ARG port=9090

SERVICE web
  ENV APP_PORT=${port}
    "#;
        let mut overrides = HashMap::new();
        overrides.insert("port".to_string(), "3000".to_string());
        let orch = parse_files_ok_with_args(
            &[("base.orch", base), ("overlay.orch", overlay)],
            &overrides,
        );
        // CLI override wins over all file defaults
        assert_eq!(orch.services[0].publish[0].host, 3000);
        assert_eq!(orch.services[0].env.get("APP_PORT").unwrap(), "3000");
    }

    #[test]
    fn test_multifile__three_files() {
        let base = r#"
ARG port=80

SERVICE db
  FROM postgres:15

SERVICE web
  FROM nginx:1.24
  ENV MODE=prod
  PUBLISH ${port}:80
  REQUIRES db
    "#;
        let staging = r#"
ARG port=8080
SERVICE web
  FROM nginx:1.25
  ENV MODE=staging
    "#;
        let personal = r#"
SERVICE web
  ENV DEBUG=true
  MEMORY 512M
    "#;
        let orch = parse_files_ok(&[
            ("base.orch", base),
            ("staging.orch", staging),
            ("personal.orch", personal),
        ]);
        let web = orch.services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.image.as_deref(), Some("nginx:1.25"));
        assert_eq!(web.env.get("MODE").unwrap(), "staging");
        assert_eq!(web.env.get("DEBUG").unwrap(), "true");
        assert_eq!(web.publish[0].host, 8080);
        assert_eq!(web.resources.memory.as_deref(), Some("512M"));
        assert_eq!(web.requires, &["db"]);
    }

    #[test]
    fn test_multifile__c1_violation_after_merge() {
        // Base has FROM, overlay adds RUN without clearing FROM properly —
        // Actually with mode switching, overlay RUN clears FROM. So both won't remain.
        // This test verifies merge is correct: no C1 error.
        let base = r#"
SERVICE svc
  FROM nginx
    "#;
        let overlay = r#"
SERVICE svc
  RUN /usr/bin/myapp
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].mode, ServiceMode::Host);
    }

    #[test]
    fn test_multifile__c4_cycle_across_files() {
        let base = r#"
SERVICE a
  FROM nginx
  REQUIRES b
    "#;
        let overlay = r#"
SERVICE b
  FROM nginx
  REQUIRES a
    "#;
        let errors = parse_files_err(&[("base.orch", base), ("overlay.orch", overlay)]);
        let cycle_err = errors.iter().any(|e| format!("{}", e).contains("cycle"));
        assert!(cycle_err, "expected cycle error, got: {:?}", errors);
    }

    #[test]
    fn test_multifile__syntax_error_in_overlay() {
        let base = r#"
SERVICE web
  FROM nginx
    "#;
        let overlay = r#"
SERVICE web
  PUBLISH bad_format
    "#;
        let errors = parse_files_err(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_multifile__clear_error_on_scalar() {
        let base = r#"
SERVICE web
  FROM nginx
    "#;
        let overlay = r#"
SERVICE web
  CLEAR FROM
    "#;
        let errors = parse_files_err(&[("base.orch", base), ("overlay.orch", overlay)]);
        let has_clear_err = errors
            .iter()
            .any(|e| format!("{}", e).contains("CLEAR cannot target scalar"));
        assert!(
            has_clear_err,
            "expected CLEAR scalar error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_multifile__clear_unknown_directive() {
        let base = r#"
SERVICE web
  FROM nginx
    "#;
        let overlay = r#"
SERVICE web
  CLEAR BANANA
    "#;
        let errors = parse_files_err(&[("base.orch", base), ("overlay.orch", overlay)]);
        let has_err = errors
            .iter()
            .any(|e| format!("{}", e).contains("unknown directive"));
        assert!(
            has_err,
            "expected unknown directive error, got: {:?}",
            errors
        );
    }

    // =========================================================================
    // ORCH_VERSION directive
    // =========================================================================

    #[test]
    fn test_orch_version__matching_is_accepted() {
        let input = format!(
            "ORCH_VERSION {}\nSERVICE db\nFROM postgres:15\n",
            crate::types::ORCH_VERSION
        );
        let orch = parse_ok(&input);
        assert_eq!(orch.version, crate::types::ORCH_VERSION);
    }

    #[test]
    fn test_orch_version__mismatch_is_rejected() {
        let errors = parse_err("ORCH_VERSION 9.9.9\nSERVICE db\nFROM img\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("unsupported Orchfile version '9.9.9'"))
        );
    }

    #[test]
    fn test_orch_version__must_precede_service() {
        let input = format!(
            "SERVICE db\nFROM img\nORCH_VERSION {}\n",
            crate::types::ORCH_VERSION
        );
        let errors = parse_err(&input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("must appear before any SERVICE"))
        );
    }

    #[test]
    fn test_orch_version__duplicate_is_rejected() {
        let v = crate::types::ORCH_VERSION;
        let input = format!("ORCH_VERSION {v}\nORCH_VERSION {v}\nSERVICE db\nFROM img\n");
        let errors = parse_err(&input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("duplicate ORCH_VERSION")));
    }

    #[test]
    fn test_orch_version__requires_value() {
        let errors = parse_err("ORCH_VERSION\nSERVICE db\nFROM img\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("ORCH_VERSION requires a version value"))
        );
    }

    #[test]
    fn test_orch_version__absent_defaults_to_spec_version() {
        let orch = parse_ok("SERVICE db\nFROM img\n");
        assert_eq!(orch.version, crate::types::ORCH_VERSION);
    }

    // =========================================================================
    // PUBLISH optional host address
    // =========================================================================

    #[test]
    fn test_publish__no_address_yields_none() {
        let orch = parse_ok("SERVICE x\nFROM img\nPUBLISH 8080:80\n");
        let p = &orch.services[0].publish[0];
        assert_eq!(p.address, None);
        assert_eq!(p.host, 8080);
        assert_eq!(p.container, 80);
    }

    #[test]
    fn test_publish__with_host_address() {
        let orch = parse_ok("SERVICE x\nFROM img\nPUBLISH 127.0.0.1:8080:80\n");
        let p = &orch.services[0].publish[0];
        assert_eq!(p.address.as_deref(), Some("127.0.0.1"));
        assert_eq!(p.host, 8080);
        assert_eq!(p.container, 80);
    }

    #[test]
    fn test_publish__ipv6_host_address() {
        let orch = parse_ok("SERVICE x\nFROM img\nPUBLISH ::1:8080:80\n");
        let p = &orch.services[0].publish[0];
        assert_eq!(p.address.as_deref(), Some("::1"));
        assert_eq!(p.host, 8080);
        assert_eq!(p.container, 80);
    }

    #[test]
    fn test_publish__address_expands_vars() {
        let input = "ARG ip=127.0.0.1\nSERVICE x\nFROM img\nPUBLISH ${ip}:8080:80\n";
        let orch = parse_ok(input);
        assert_eq!(
            orch.services[0].publish[0].address.as_deref(),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn test_publish__address_omitted_from_json_when_none() {
        let orch = parse_ok("SERVICE x\nFROM img\nPUBLISH 8080:80\n");
        let json = serde_json::to_string(&orch).unwrap();
        assert!(!json.contains("address"));
    }

    #[test]
    fn test_publish__address_present_in_json() {
        let orch = parse_ok("SERVICE x\nFROM img\nPUBLISH 127.0.0.1:8080:80\n");
        let json = serde_json::to_string(&orch).unwrap();
        assert!(json.contains("\"address\":\"127.0.0.1\""));
    }
}
