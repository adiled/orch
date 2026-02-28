use std::collections::{HashMap, HashSet};

use crate::error::{OrchError, ParseError, ValidationError};
use crate::types::*;

/// Expand `${VAR}` references in a string using the provided args map.
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
                // Leave unresolved variables as-is (built-in vars resolved at runtime)
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

/// Parse a PUBLISH directive value like "5433:5432".
fn parse_port_mapping(value: &str, line: usize) -> Result<PortMapping, ParseError> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 2 {
        return Err(ParseError::new(
            line,
            format!("invalid PUBLISH format '{}', expected host_port:container_port", value),
        ));
    }
    let host: u16 = parts[0].parse().map_err(|_| {
        ParseError::new(line, format!("invalid host port '{}' in PUBLISH", parts[0]))
    })?;
    let container: u16 = parts[1].parse().map_err(|_| {
        ParseError::new(
            line,
            format!("invalid container port '{}' in PUBLISH", parts[1]),
        )
    })?;
    Ok(PortMapping { host, container })
}

/// Parse a VOLUME directive value like "/host/path:/container/path"
/// or "named-vol:/container/path".
fn parse_volume_mount(value: &str, line: usize) -> Result<VolumeMount, ParseError> {
    let parts: Vec<&str> = value.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(ParseError::new(
            line,
            format!(
                "invalid VOLUME format '{}', expected source:destination",
                value
            ),
        ));
    }
    let source = parts[0].to_string();
    let destination = parts[1].to_string();
    let is_named = !source.starts_with('/') && !source.starts_with("${");
    Ok(VolumeMount {
        source,
        destination,
        is_named,
    })
}

/// Parse a boolean value from directive.
fn parse_bool(value: &str, directive: &str, line: usize) -> Result<bool, ParseError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ParseError::new(
            line,
            format!("{} expects 'true' or 'false', got '{}'", directive, value),
        )),
    }
}

/// Parse a RESTART policy value.
fn parse_restart_policy(value: &str, line: usize) -> Result<RestartPolicy, ParseError> {
    match value {
        "no" => Ok(RestartPolicy::No),
        "always" => Ok(RestartPolicy::Always),
        "on-failure" => Ok(RestartPolicy::OnFailure),
        _ => Err(ParseError::new(
            line,
            format!(
                "invalid RESTART value '{}', expected 'no', 'always', or 'on-failure'",
                value
            ),
        )),
    }
}

/// Parse a RECREATE policy value.
fn parse_recreate_policy(value: &str, line: usize) -> Result<RecreatePolicy, ParseError> {
    match value {
        "always" => Ok(RecreatePolicy::Always),
        "never" => Ok(RecreatePolicy::Never),
        _ => Err(ParseError::new(
            line,
            format!(
                "invalid RECREATE value '{}', expected 'always' or 'never'",
                value
            ),
        )),
    }
}

/// Parse an ENV directive value like "KEY=value".
fn parse_env(value: &str, line: usize) -> Result<(String, String), ParseError> {
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

// ---------------------------------------------------------------------------
// Intermediate builder: we don't know the mode until we see FROM or RUN
// ---------------------------------------------------------------------------

struct ServiceBuilder {
    name: String,
    from: Option<String>,
    run: Option<String>,
    entrypoint: Option<String>,
    cmd: Option<String>,
    publish: Vec<PortMapping>,
    volumes: Vec<VolumeMount>,
    user: Option<String>,
    stop_command: Option<String>,
    reload_command: Option<String>,
    workdir: Option<String>,
    env: HashMap<String, String>,
    env_files: Vec<String>,
    requires: Vec<String>,
    after: Vec<String>,
    healthcheck: Option<String>,
    readiness_timeout: Option<String>,
    oneshot: bool,
    disabled: bool,
    recreate: RecreatePolicy,
    restart: RestartConfig,
    timeouts: TimeoutConfig,
    resources: ResourceLimits,
    logging: LogConfig,
    // Track which container-only and host-only directives were used (for error messages)
    container_directives_used: Vec<(String, usize)>,
    host_directives_used: Vec<(String, usize)>,
}

impl ServiceBuilder {
    fn new(name: String) -> Self {
        ServiceBuilder {
            name,
            from: None,
            run: None,
            entrypoint: None,
            cmd: None,
            publish: Vec::new(),
            volumes: Vec::new(),
            user: None,
            stop_command: None,
            reload_command: None,
            workdir: None,
            env: HashMap::new(),
            env_files: Vec::new(),
            requires: Vec::new(),
            after: Vec::new(),
            healthcheck: None,
            readiness_timeout: None,
            oneshot: false,
            disabled: false,
            recreate: RecreatePolicy::default(),
            restart: RestartConfig::default(),
            timeouts: TimeoutConfig::default(),
            resources: ResourceLimits::default(),
            logging: LogConfig::default(),
            container_directives_used: Vec::new(),
            host_directives_used: Vec::new(),
        }
    }

    /// Finalize into a Service, validating C1/C2/C3.
    fn build(self) -> Result<Service, Vec<ValidationError>> {
        let mut errors = Vec::new();

        // C1: FROM XOR RUN
        let mode = match (&self.from, &self.run) {
            (Some(_), Some(_)) => {
                errors.push(ValidationError::new(
                    &self.name,
                    "cannot specify both FROM and RUN (C1)",
                ));
                ServiceMode::Container // pick one to continue validation
            }
            (None, None) => {
                errors.push(ValidationError::new(
                    &self.name,
                    "must specify either FROM or RUN (C1)",
                ));
                ServiceMode::Container
            }
            (Some(_), None) => ServiceMode::Container,
            (None, Some(_)) => ServiceMode::Host,
        };

        // C2: Container-only directives with RUN
        if mode == ServiceMode::Host {
            for (directive, line) in &self.container_directives_used {
                errors.push(ValidationError::new(
                    &self.name,
                    format!(
                        "{} is only valid with FROM (container mode), found at line {} (C2)",
                        directive, line
                    ),
                ));
            }
        }

        // C3: Host-only directives with FROM
        if mode == ServiceMode::Container {
            for (directive, line) in &self.host_directives_used {
                errors.push(ValidationError::new(
                    &self.name,
                    format!(
                        "{} is only valid with RUN (host mode), found at line {} (C3)",
                        directive, line
                    ),
                ));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Service {
            name: self.name,
            mode,
            image: self.from,
            run_command: self.run,
            entrypoint: self.entrypoint,
            cmd: self.cmd,
            publish: self.publish,
            volumes: self.volumes,
            user: self.user,
            stop_command: self.stop_command,
            reload_command: self.reload_command,
            workdir: self.workdir,
            env: self.env,
            env_files: self.env_files,
            requires: self.requires,
            after: self.after,
            healthcheck: self.healthcheck,
            readiness_timeout: self.readiness_timeout,
            oneshot: self.oneshot,
            disabled: self.disabled,
            recreate: self.recreate,
            restart: self.restart,
            timeouts: self.timeouts,
            resources: self.resources,
            logging: self.logging,
        })
    }
}

// ---------------------------------------------------------------------------
// Top-level parser
// ---------------------------------------------------------------------------

/// Parse an Orchfile from string content.
///
/// `overrides` are CLI/env arg overrides that take precedence over file defaults.
pub fn parse(input: &str, overrides: &HashMap<String, String>) -> Result<OrchFile, Vec<OrchError>> {
    let mut orch = OrchFile::new();
    let mut errors: Vec<OrchError> = Vec::new();
    let mut current_service: Option<ServiceBuilder> = None;
    let mut seen_service_names: HashMap<String, usize> = HashMap::new();

    // First pass: collect ARG defaults, then apply overrides
    for (line_num_0, raw_line) in input.lines().enumerate() {
        let line_num = line_num_0 + 1;
        let line = raw_line.trim();

        // Skip blank lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (directive, value) = split_directive(line);
        if directive == "ARG" {
            if let Some(value) = value {
                match parse_arg(value, line_num) {
                    Ok((name, default)) => {
                        // Override priority: overrides > file default
                        if let Some(ov) = overrides.get(&name) {
                            orch.args.insert(name, ov.clone());
                        } else {
                            orch.args.insert(name, default);
                        }
                    }
                    Err(e) => errors.push(e.into()),
                }
            } else {
                errors.push(ParseError::new(line_num, "ARG requires name=value").into());
            }
        }
    }

    // Bail early if ARG parsing had errors
    if !errors.is_empty() {
        return Err(errors);
    }

    // Second pass: parse everything using resolved args for variable expansion
    for (line_num_0, raw_line) in input.lines().enumerate() {
        let line_num = line_num_0 + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (directive, value) = split_directive(line);

        // Skip ARG on second pass (already handled)
        if directive == "ARG" {
            continue;
        }

        if directive == "SERVICE" {
            // Finalize previous service
            if let Some(builder) = current_service.take() {
                match builder.build() {
                    Ok(svc) => orch.services.push(svc),
                    Err(errs) => errors.extend(errs.into_iter().map(OrchError::from)),
                }
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

            current_service = Some(ServiceBuilder::new(name));
            continue;
        }

        // All other directives must be inside a SERVICE block
        let builder = match current_service.as_mut() {
            Some(b) => b,
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

        // Expand variables in value
        let expanded = value.map(|v| expand_vars(v, &orch.args));

        match directive {
            // -- Execution mode --
            "FROM" => {
                let val = require_value(&expanded, "FROM", line_num, &mut errors);
                if let Some(v) = val {
                    if builder.from.is_some() {
                        errors.push(
                            ParseError::new(line_num, "duplicate FROM directive").into(),
                        );
                    } else {
                        builder.from = Some(v);
                    }
                }
            }
            "RUN" => {
                let val = require_value(&expanded, "RUN", line_num, &mut errors);
                if let Some(v) = val {
                    if builder.run.is_some() {
                        errors.push(
                            ParseError::new(line_num, "duplicate RUN directive").into(),
                        );
                    } else {
                        builder.run = Some(v);
                    }
                }
            }

            // -- Container-only --
            "ENTRYPOINT" => {
                builder
                    .container_directives_used
                    .push(("ENTRYPOINT".to_string(), line_num));
                let val = require_value(&expanded, "ENTRYPOINT", line_num, &mut errors);
                if let Some(v) = val {
                    builder.entrypoint = Some(v);
                }
            }
            "CMD" => {
                builder
                    .container_directives_used
                    .push(("CMD".to_string(), line_num));
                let val = require_value(&expanded, "CMD", line_num, &mut errors);
                if let Some(v) = val {
                    builder.cmd = Some(v);
                }
            }
            "PUBLISH" => {
                builder
                    .container_directives_used
                    .push(("PUBLISH".to_string(), line_num));
                let val = require_value(&expanded, "PUBLISH", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_port_mapping(&v, line_num) {
                        Ok(pm) => builder.publish.push(pm),
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "VOLUME" => {
                builder
                    .container_directives_used
                    .push(("VOLUME".to_string(), line_num));
                let val = require_value(&expanded, "VOLUME", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_volume_mount(&v, line_num) {
                        Ok(vm) => builder.volumes.push(vm),
                        Err(e) => errors.push(e.into()),
                    }
                }
            }

            // -- Host-only --
            "USER" => {
                builder
                    .host_directives_used
                    .push(("USER".to_string(), line_num));
                let val = require_value(&expanded, "USER", line_num, &mut errors);
                if let Some(v) = val {
                    builder.user = Some(v);
                }
            }
            "STOP" => {
                builder
                    .host_directives_used
                    .push(("STOP".to_string(), line_num));
                let val = require_value(&expanded, "STOP", line_num, &mut errors);
                if let Some(v) = val {
                    builder.stop_command = Some(v);
                }
            }
            "RELOAD" => {
                builder
                    .host_directives_used
                    .push(("RELOAD".to_string(), line_num));
                let val = require_value(&expanded, "RELOAD", line_num, &mut errors);
                if let Some(v) = val {
                    builder.reload_command = Some(v);
                }
            }

            // -- Common --
            "WORKDIR" => {
                let val = require_value(&expanded, "WORKDIR", line_num, &mut errors);
                if let Some(v) = val {
                    builder.workdir = Some(v);
                }
            }
            "ENV" => {
                let val = require_value(&expanded, "ENV", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_env(&v, line_num) {
                        Ok((k, ev)) => {
                            builder.env.insert(k, ev);
                        }
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "ENV_FILE" => {
                let val = require_value(&expanded, "ENV_FILE", line_num, &mut errors);
                if let Some(v) = val {
                    builder.env_files.push(v);
                }
            }
            "REQUIRES" => {
                let val = require_value(&expanded, "REQUIRES", line_num, &mut errors);
                if let Some(v) = val {
                    for dep in v.split_whitespace() {
                        builder.requires.push(dep.to_string());
                    }
                }
            }
            "AFTER" => {
                let val = require_value(&expanded, "AFTER", line_num, &mut errors);
                if let Some(v) = val {
                    for dep in v.split_whitespace() {
                        builder.after.push(dep.to_string());
                    }
                }
            }
            "HEALTHCHECK" => {
                let val = require_value(&expanded, "HEALTHCHECK", line_num, &mut errors);
                if let Some(v) = val {
                    builder.healthcheck = Some(v);
                }
            }
            "READINESS_TIMEOUT" => {
                let val = require_value(&expanded, "READINESS_TIMEOUT", line_num, &mut errors);
                if let Some(v) = val {
                    builder.readiness_timeout = Some(v);
                }
            }
            "ONESHOT" => {
                let val = require_value(&expanded, "ONESHOT", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_bool(&v, "ONESHOT", line_num) {
                        Ok(b) => builder.oneshot = b,
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "DISABLED" => {
                let val = require_value(&expanded, "DISABLED", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_bool(&v, "DISABLED", line_num) {
                        Ok(b) => builder.disabled = b,
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "RECREATE" => {
                let val = require_value(&expanded, "RECREATE", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_recreate_policy(&v, line_num) {
                        Ok(p) => builder.recreate = p,
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "RESTART" => {
                let val = require_value(&expanded, "RESTART", line_num, &mut errors);
                if let Some(v) = val {
                    match parse_restart_policy(&v, line_num) {
                        Ok(p) => builder.restart.policy = p,
                        Err(e) => errors.push(e.into()),
                    }
                }
            }
            "RESTART_DELAY" => {
                let val = require_value(&expanded, "RESTART_DELAY", line_num, &mut errors);
                if let Some(v) = val {
                    builder.restart.delay = Some(v);
                }
            }
            "START_LIMIT_BURST" => {
                let val = require_value(&expanded, "START_LIMIT_BURST", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<u32>() {
                        Ok(n) => builder.restart.start_limit_burst = Some(n),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("START_LIMIT_BURST must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "START_LIMIT_INTERVAL" => {
                let val =
                    require_value(&expanded, "START_LIMIT_INTERVAL", line_num, &mut errors);
                if let Some(v) = val {
                    builder.restart.start_limit_interval = Some(v);
                }
            }
            "TIMEOUT_START" => {
                let val = require_value(&expanded, "TIMEOUT_START", line_num, &mut errors);
                if let Some(v) = val {
                    builder.timeouts.start = Some(v);
                }
            }
            "TIMEOUT_STOP" => {
                let val = require_value(&expanded, "TIMEOUT_STOP", line_num, &mut errors);
                if let Some(v) = val {
                    builder.timeouts.stop = Some(v);
                }
            }
            "MEMORY" => {
                let val = require_value(&expanded, "MEMORY", line_num, &mut errors);
                if let Some(v) = val {
                    builder.resources.memory = Some(v);
                }
            }
            "CPUS" => {
                let val = require_value(&expanded, "CPUS", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<f64>() {
                        Ok(n) => builder.resources.cpus = Some(n),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("CPUS must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "CPU_QUOTA" => {
                let val = require_value(&expanded, "CPU_QUOTA", line_num, &mut errors);
                if let Some(v) = val {
                    builder.resources.cpu_quota = Some(v);
                }
            }
            "LIMIT_NOFILE" => {
                let val = require_value(&expanded, "LIMIT_NOFILE", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<u64>() {
                        Ok(n) => builder.resources.limit_nofile = Some(n),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("LIMIT_NOFILE must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "LIMIT_NPROC" => {
                let val = require_value(&expanded, "LIMIT_NPROC", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<u64>() {
                        Ok(n) => builder.resources.limit_nproc = Some(n),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("LIMIT_NPROC must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "TASKS_MAX" => {
                let val = require_value(&expanded, "TASKS_MAX", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<u64>() {
                        Ok(n) => builder.resources.tasks_max = Some(n),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("TASKS_MAX must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "IO_WEIGHT" => {
                let val = require_value(&expanded, "IO_WEIGHT", line_num, &mut errors);
                if let Some(v) = val {
                    match v.parse::<u32>() {
                        Ok(n) if (10..=1000).contains(&n) => {
                            builder.resources.io_weight = Some(n)
                        }
                        Ok(n) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("IO_WEIGHT must be 10-1000, got {}", n),
                            )
                            .into(),
                        ),
                        Err(_) => errors.push(
                            ParseError::new(
                                line_num,
                                format!("IO_WEIGHT must be a number, got '{}'", v),
                            )
                            .into(),
                        ),
                    }
                }
            }
            "STDOUT" => {
                let val = require_value(&expanded, "STDOUT", line_num, &mut errors);
                if let Some(v) = val {
                    builder.logging.stdout = Some(v);
                }
            }
            "STDERR" => {
                let val = require_value(&expanded, "STDERR", line_num, &mut errors);
                if let Some(v) = val {
                    builder.logging.stderr = Some(v);
                }
            }

            _ => {
                errors.push(
                    ParseError::new(
                        line_num,
                        format!("unknown directive '{}'", directive),
                    )
                    .into(),
                );
            }
        }
    }

    // Finalize last service
    if let Some(builder) = current_service.take() {
        match builder.build() {
            Ok(svc) => orch.services.push(svc),
            Err(errs) => errors.extend(errs.into_iter().map(OrchError::from)),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // C4: Dependency acyclicity
    if let Err(cycle_errors) = validate_dag(&orch) {
        return Err(cycle_errors.into_iter().map(OrchError::from).collect());
    }

    // Validate that REQUIRES/AFTER reference existing services
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
        // AFTER references are soft — service may be disabled or absent.
        // Spec says "if they exist", so we don't error on missing AFTER targets.
    }
    if !ref_errors.is_empty() {
        return Err(ref_errors);
    }

    Ok(orch)
}

/// C4: Validate that REQUIRES + AFTER form a DAG (no cycles).
fn validate_dag(orch: &OrchFile) -> Result<(), Vec<ValidationError>> {
    // Build adjacency: service -> set of services it depends on
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

    // DFS-based cycle detection
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
                        // Found a cycle — extract the cycle from path
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

/// Helper to require a value for a directive, pushing an error if missing.
fn require_value(
    expanded: &Option<String>,
    directive: &str,
    line: usize,
    errors: &mut Vec<OrchError>,
) -> Option<String> {
    match expanded {
        Some(v) if !v.is_empty() => Some(v.clone()),
        _ => {
            errors.push(
                ParseError::new(line, format!("{} requires a value", directive)).into(),
            );
            None
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests;
