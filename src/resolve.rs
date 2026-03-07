use std::collections::{HashMap, HashSet};

use crate::error::{OrchError, ValidationError};
use crate::types::*;

/// Resolve a merged RawOrchFile into a final OrchFile.
///
/// Pipeline:
/// 1. Apply CLI/env overrides to merged ARGs
/// 2. Expand all ${var_ref} in string fields
/// 3. Parse typed values (bools, numbers, enums, ports, volumes)
/// 4. Determine service mode
/// 5. Validate C1 (FROM XOR RUN)
/// 6. Validate C2/C3 (mode-specific directives)
/// 7. Validate C4 (DAG acyclicity)
/// 8. Validate REQUIRES references
pub fn resolve(
    raw: RawOrchFile,
    overrides: &HashMap<String, String>,
    file_names: &[String],
) -> Result<OrchFile, Vec<OrchError>> {
    let mut errors: Vec<OrchError> = Vec::new();

    // Step 1: Apply overrides to merged ARGs
    let mut args = raw.args;
    for (k, v) in overrides {
        if args.contains_key(k) {
            args.insert(k.clone(), v.clone());
        }
    }

    // Step 2-4: Expand and resolve each service
    let mut services: Vec<Service> = Vec::new();

    for raw_svc in &raw.services {
        match resolve_service(raw_svc, &args, file_names) {
            Ok(svc) => services.push(svc),
            Err(errs) => errors.extend(errs),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // Step 7: C4 - DAG acyclicity
    let orch = OrchFile {
        version: "0.2.0".to_string(),
        args,
        services,
    };

    if let Err(cycle_errors) = validate_dag(&orch) {
        return Err(cycle_errors.into_iter().map(OrchError::from).collect());
    }

    // Step 8: Validate REQUIRES references
    let service_names: HashSet<&str> = orch.services.iter().map(|s| s.name.as_str()).collect();
    let mut ref_errors = Vec::new();
    for svc in &orch.services {
        for req in &svc.requires {
            if !service_names.contains(req.as_str()) {
                ref_errors.push(OrchError::from(ValidationError::new(
                    &svc.name,
                    format!("REQUIRES references unknown service '{}'", req),
                )));
            }
        }
    }
    if !ref_errors.is_empty() {
        return Err(ref_errors);
    }

    Ok(orch)
}

/// Resolve a single RawService into a Service.
/// Handles expansion, type parsing, and C1/C2/C3 validation.
fn resolve_service(
    raw: &RawService,
    args: &HashMap<String, String>,
    file_names: &[String],
) -> Result<Service, Vec<OrchError>> {
    let mut errors: Vec<OrchError> = Vec::new();

    // Expand ${var_ref} in all string fields
    let from = raw.from.as_ref().map(|v| expand_vars(v, args));
    let run = raw.run.as_ref().map(|v| expand_vars(v, args));
    let entrypoint = raw.entrypoint.as_ref().map(|v| expand_vars(v, args));
    let cmd = raw.cmd.as_ref().map(|v| expand_vars(v, args));
    let user = raw.user.as_ref().map(|v| expand_vars(v, args));
    let stop_command = raw.stop_command.as_ref().map(|v| expand_vars(v, args));
    let reload_command = raw.reload_command.as_ref().map(|v| expand_vars(v, args));
    let workdir = raw.workdir.as_ref().map(|v| expand_vars(v, args));
    let healthcheck = raw.healthcheck.as_ref().map(|v| expand_vars(v, args));
    let readiness_timeout = raw.readiness_timeout.as_ref().map(|v| expand_vars(v, args));
    let stdout = raw.stdout.as_ref().map(|v| expand_vars(v, args));
    let stderr = raw.stderr.as_ref().map(|v| expand_vars(v, args));

    // Expand and parse ENV
    let mut env: HashMap<String, String> = HashMap::new();
    for (k, v) in &raw.env.values {
        env.insert(expand_vars(k, args), expand_vars(v, args));
    }

    // Expand ENV_FILE paths
    let env_files: Vec<String> = raw
        .env_files
        .values
        .iter()
        .map(|v| expand_vars(v, args))
        .collect();

    // Expand REQUIRES/AFTER (service names shouldn't have vars, but be consistent)
    let requires: Vec<String> = raw
        .requires
        .values
        .iter()
        .map(|v| expand_vars(v, args))
        .collect();
    let after: Vec<String> = raw
        .after
        .values
        .iter()
        .map(|v| expand_vars(v, args))
        .collect();

    // Expand and parse PUBLISH
    let mut publish: Vec<PortMapping> = Vec::new();
    for (host_raw, container_raw) in &raw.publish.values {
        let host_str = expand_vars(host_raw, args);
        let container_str = expand_vars(container_raw, args);
        match (host_str.parse::<u16>(), container_str.parse::<u16>()) {
            (Ok(host), Ok(container)) => publish.push(PortMapping { host, container }),
            (Err(_), _) => {
                errors.push(
                    ValidationError::new(
                        &raw.name,
                        format!("invalid host port '{}' in PUBLISH", host_str),
                    )
                    .into(),
                );
            }
            (_, Err(_)) => {
                errors.push(
                    ValidationError::new(
                        &raw.name,
                        format!("invalid container port '{}' in PUBLISH", container_str),
                    )
                    .into(),
                );
            }
        }
    }

    // Expand and parse VOLUME
    let mut volumes: Vec<VolumeMount> = Vec::new();
    for (source_raw, dest_raw) in &raw.volumes.values {
        let source = expand_vars(source_raw, args);
        let destination = expand_vars(dest_raw, args);
        let is_named = !source.starts_with('/')
            && !source.starts_with("./")
            && !source.starts_with("../")
            && !source.starts_with("${");
        volumes.push(VolumeMount {
            source,
            destination,
            is_named,
        });
    }

    // Parse typed scalars
    let oneshot = parse_bool_field(&raw.oneshot, "ONESHOT", &raw.name, args, &mut errors);
    let disabled = parse_bool_field(&raw.disabled, "DISABLED", &raw.name, args, &mut errors);

    let recreate = match &raw.recreate {
        Some(v) => {
            let expanded = expand_vars(v, args);
            match expanded.as_str() {
                "always" => RecreatePolicy::Always,
                "never" => RecreatePolicy::Never,
                _ => {
                    errors.push(
                        ValidationError::new(
                            &raw.name,
                            format!(
                                "invalid RECREATE value '{}', expected 'always' or 'never'",
                                expanded
                            ),
                        )
                        .into(),
                    );
                    RecreatePolicy::default()
                }
            }
        }
        None => RecreatePolicy::default(),
    };

    let restart_policy = match &raw.restart {
        Some(v) => {
            let expanded = expand_vars(v, args);
            match expanded.as_str() {
                "no" => RestartPolicy::No,
                "always" => RestartPolicy::Always,
                "on-failure" => RestartPolicy::OnFailure,
                _ => {
                    errors.push(
                        ValidationError::new(
                            &raw.name,
                            format!(
                                "invalid RESTART value '{}', expected 'no', 'always', or 'on-failure'",
                                expanded
                            ),
                        )
                        .into(),
                    );
                    RestartPolicy::default()
                }
            }
        }
        None => RestartPolicy::default(),
    };

    let restart_delay = raw.restart_delay.as_ref().map(|v| expand_vars(v, args));
    let start_limit_burst = parse_u32_field(
        &raw.start_limit_burst,
        "START_LIMIT_BURST",
        &raw.name,
        args,
        &mut errors,
    );
    let start_limit_interval = raw
        .start_limit_interval
        .as_ref()
        .map(|v| expand_vars(v, args));

    let timeout_start = raw.timeout_start.as_ref().map(|v| expand_vars(v, args));
    let timeout_stop = raw.timeout_stop.as_ref().map(|v| expand_vars(v, args));

    let memory = raw.memory.as_ref().map(|v| expand_vars(v, args));
    let cpus = parse_f64_field(&raw.cpus, "CPUS", &raw.name, args, &mut errors);
    let cpu_quota = raw.cpu_quota.as_ref().map(|v| expand_vars(v, args));
    let limit_nofile = parse_u64_field(
        &raw.limit_nofile,
        "LIMIT_NOFILE",
        &raw.name,
        args,
        &mut errors,
    );
    let limit_nproc = parse_u64_field(
        &raw.limit_nproc,
        "LIMIT_NPROC",
        &raw.name,
        args,
        &mut errors,
    );
    let tasks_max = parse_u64_field(&raw.tasks_max, "TASKS_MAX", &raw.name, args, &mut errors);
    let io_weight = parse_io_weight(&raw.io_weight, &raw.name, args, &mut errors);

    // C1: FROM XOR RUN
    let mode = match (&from, &run) {
        (Some(_), Some(_)) => {
            let mut msg = "cannot specify both FROM and RUN (C1)".to_string();
            if let (Some(from_src), Some(run_src)) = (&raw.from_source, &raw.run_source) {
                let from_file = file_name_or_default(file_names, from_src.file);
                let run_file = file_name_or_default(file_names, run_src.file);
                msg = format!(
                    "cannot specify both FROM ({}:{}) and RUN ({}:{}) (C1)",
                    from_file, from_src.line, run_file, run_src.line
                );
            }
            errors.push(ValidationError::new(&raw.name, msg).into());
            ServiceMode::Container
        }
        (None, None) => {
            errors.push(
                ValidationError::new(&raw.name, "must specify either FROM or RUN (C1)").into(),
            );
            ServiceMode::Container
        }
        (Some(_), None) => ServiceMode::Container,
        (None, Some(_)) => ServiceMode::Host,
    };

    // C2: Container-only directives with host mode
    if mode == ServiceMode::Host {
        for (directive, line, file_idx) in &raw.container_directives_used {
            let file = file_name_or_default(file_names, *file_idx);
            errors.push(
                ValidationError::new(
                    &raw.name,
                    format!(
                        "{} is only valid with FROM (container mode), found at {}:{} (C2)",
                        directive, file, line
                    ),
                )
                .into(),
            );
        }
    }

    // C3: Host-only directives with container mode
    if mode == ServiceMode::Container {
        for (directive, line, file_idx) in &raw.host_directives_used {
            let file = file_name_or_default(file_names, *file_idx);
            errors.push(
                ValidationError::new(
                    &raw.name,
                    format!(
                        "{} is only valid with RUN (host mode), found at {}:{} (C3)",
                        directive, file, line
                    ),
                )
                .into(),
            );
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Service {
        name: raw.name.clone(),
        mode,
        image: from,
        run_command: run,
        entrypoint,
        cmd,
        publish,
        volumes,
        user,
        stop_command,
        reload_command,
        workdir,
        env,
        env_files,
        requires,
        after,
        healthcheck,
        readiness_timeout,
        oneshot,
        disabled,
        recreate,
        restart: RestartConfig {
            policy: restart_policy,
            delay: restart_delay,
            start_limit_burst,
            start_limit_interval,
        },
        timeouts: TimeoutConfig {
            start: timeout_start,
            stop: timeout_stop,
        },
        resources: ResourceLimits {
            memory,
            cpus,
            cpu_quota,
            limit_nofile,
            limit_nproc,
            tasks_max,
            io_weight,
        },
        logging: LogConfig { stdout, stderr },
    })
}

// =========================================================================
// Variable expansion
// =========================================================================

/// Expand `${VAR}` references in a string using the provided args map.
/// Unresolved variables are left as-is (for runtime built-in vars).
fn expand_vars(input: &str, args: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                var_name.push(ch);
            }
            if let Some(val) = args.get(&var_name) {
                result.push_str(val);
            } else {
                result.push_str("${");
                result.push_str(&var_name);
                result.push('}');
            }
        } else {
            result.push(c);
        }
    }

    result
}

// =========================================================================
// Type parsing helpers
// =========================================================================

fn parse_bool_field(
    raw: &Option<String>,
    directive: &str,
    service: &str,
    args: &HashMap<String, String>,
    errors: &mut Vec<OrchError>,
) -> bool {
    match raw {
        Some(v) => {
            let expanded = expand_vars(v, args);
            match expanded.as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    errors.push(
                        ValidationError::new(
                            service,
                            format!(
                                "{} expects 'true' or 'false', got '{}'",
                                directive, expanded
                            ),
                        )
                        .into(),
                    );
                    false
                }
            }
        }
        None => false,
    }
}

fn parse_u32_field(
    raw: &Option<String>,
    directive: &str,
    service: &str,
    args: &HashMap<String, String>,
    errors: &mut Vec<OrchError>,
) -> Option<u32> {
    raw.as_ref().map(|v| {
        let expanded = expand_vars(v, args);
        match expanded.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                errors.push(
                    ValidationError::new(
                        service,
                        format!("{} must be a number, got '{}'", directive, expanded),
                    )
                    .into(),
                );
                0
            }
        }
    })
}

fn parse_u64_field(
    raw: &Option<String>,
    directive: &str,
    service: &str,
    args: &HashMap<String, String>,
    errors: &mut Vec<OrchError>,
) -> Option<u64> {
    raw.as_ref().map(|v| {
        let expanded = expand_vars(v, args);
        match expanded.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                errors.push(
                    ValidationError::new(
                        service,
                        format!("{} must be a number, got '{}'", directive, expanded),
                    )
                    .into(),
                );
                0
            }
        }
    })
}

fn parse_f64_field(
    raw: &Option<String>,
    directive: &str,
    service: &str,
    args: &HashMap<String, String>,
    errors: &mut Vec<OrchError>,
) -> Option<f64> {
    raw.as_ref().map(|v| {
        let expanded = expand_vars(v, args);
        match expanded.parse::<f64>() {
            Ok(n) => n,
            Err(_) => {
                errors.push(
                    ValidationError::new(
                        service,
                        format!("{} must be a number, got '{}'", directive, expanded),
                    )
                    .into(),
                );
                0.0
            }
        }
    })
}

fn parse_io_weight(
    raw: &Option<String>,
    service: &str,
    args: &HashMap<String, String>,
    errors: &mut Vec<OrchError>,
) -> Option<u32> {
    raw.as_ref().map(|v| {
        let expanded = expand_vars(v, args);
        match expanded.parse::<u32>() {
            Ok(n) if (10..=1000).contains(&n) => n,
            Ok(n) => {
                errors.push(
                    ValidationError::new(service, format!("IO_WEIGHT must be 10-1000, got {}", n))
                        .into(),
                );
                n
            }
            Err(_) => {
                errors.push(
                    ValidationError::new(
                        service,
                        format!("IO_WEIGHT must be a number, got '{}'", expanded),
                    )
                    .into(),
                );
                0
            }
        }
    })
}

/// Get file name for error messages, defaulting to "line" when no files registered.
fn file_name_or_default(file_names: &[String], file_index: usize) -> String {
    file_names
        .get(file_index)
        .cloned()
        .unwrap_or_else(|| format!("file {}", file_index))
}

// =========================================================================
// C4: DAG validation (moved from parser.rs)
// =========================================================================

/// Validate that REQUIRES + AFTER form a DAG (no cycles).
fn validate_dag(orch: &OrchFile) -> Result<(), Vec<ValidationError>> {
    let service_names: HashSet<&str> = orch.services.iter().map(|s| s.name.as_str()).collect();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for svc in &orch.services {
        let deps: Vec<&str> = svc
            .requires
            .iter()
            .chain(svc.after.iter())
            .filter(|d| service_names.contains(d.as_str()))
            .map(|d| d.as_str())
            .collect();
        adj.insert(svc.name.as_str(), deps);
    }

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        Visiting,
        Visited,
    }

    let mut state: HashMap<&str, State> = HashMap::new();
    for name in &service_names {
        state.insert(name, State::Unvisited);
    }

    let mut path: Vec<&str> = Vec::new();
    let mut errors: Vec<ValidationError> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, State>,
        path: &mut Vec<&'a str>,
        errors: &mut Vec<ValidationError>,
    ) {
        state.insert(node, State::Visiting);
        path.push(node);

        if let Some(deps) = adj.get(node) {
            for &dep in deps {
                match state.get(dep) {
                    Some(State::Visiting) => {
                        let cycle_start = path.iter().position(|&n| n == dep).unwrap();
                        let cycle: Vec<&str> = path[cycle_start..].to_vec();
                        errors.push(ValidationError::new(
                            dep,
                            format!(
                                "dependency cycle detected: {} -> {}",
                                cycle.join(" -> "),
                                dep
                            ),
                        ));
                    }
                    Some(State::Unvisited) => {
                        dfs(dep, adj, state, path, errors);
                    }
                    _ => {}
                }
            }
        }

        path.pop();
        state.insert(node, State::Visited);
    }

    for &name in &service_names {
        if state[name] == State::Unvisited {
            dfs(name, &adj, &mut state, &mut path, &mut errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
