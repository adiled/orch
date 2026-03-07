use std::collections::HashMap;

use crate::error::{OrchError, ParseError};
use orch::types::*;

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
                    // Validate format (has colon) but store raw strings
                    let parts: Vec<&str> = v.split(':').collect();
                    if parts.len() != 2 {
                        errors.push(
                            ParseError::new(
                                line_num,
                                format!(
                                    "invalid PUBLISH format '{}', expected host_port:container_port",
                                    v
                                ),
                            )
                            .into(),
                        );
                    } else {
                        svc.publish
                            .values
                            .push((parts[0].to_string(), parts[1].to_string()));
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
mod tests;
