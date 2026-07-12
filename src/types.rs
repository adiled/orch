use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The Orchfile **spec** version this parser implements.
///
/// This is the version of the Orchfile specification that the `ORCH_VERSION`
/// directive declares and is validated against, independent of the `orch`
/// library/CLI crate version in `Cargo.toml`. The spec and the tooling evolve
/// on separate tracks.
pub const ORCH_VERSION: &str = "1.0.0-rc";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Container,
    Host,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    No,
    Always,
    OnFailure,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::No
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecreatePolicy {
    Always,
    Never,
}

impl Default for RecreatePolicy {
    fn default() -> Self {
        RecreatePolicy::Never
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    /// Optional host bind address (e.g. `127.0.0.1`). `None` binds all interfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub host: u16,
    pub container: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub source: String,
    pub destination: String,
    pub is_named: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_quota: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_nofile: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_nproc: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks_max: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_weight: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RestartConfig {
    pub policy: RestartPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_limit_burst: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_limit_interval: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimeoutConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub mode: ServiceMode,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publish: Vec<PortMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<VolumeMount>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_command: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_files: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthcheck: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_timeout: Option<String>,

    pub oneshot: bool,
    pub disabled: bool,
    pub recreate: RecreatePolicy,

    pub restart: RestartConfig,
    pub timeouts: TimeoutConfig,
    pub resources: ResourceLimits,
    pub logging: LogConfig,
}

impl Service {
    #[allow(dead_code)]
    pub fn new(name: String, mode: ServiceMode) -> Self {
        Service {
            name,
            mode,
            image: None,
            run_command: None,
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
        }
    }

    /// Returns true if this service runs directly on the host (not in a container).
    pub fn is_host(&self) -> bool {
        self.mode == ServiceMode::Host
    }

    /// Returns true if this service runs in a container.
    pub fn is_container(&self) -> bool {
        self.mode == ServiceMode::Container
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchFile {
    pub version: String,
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub services: Vec<Service>,
}

impl OrchFile {
    pub fn new() -> Self {
        OrchFile {
            version: ORCH_VERSION.to_string(),
            args: HashMap::new(),
            services: Vec::new(),
        }
    }
}

impl Default for OrchFile {
    fn default() -> Self {
        Self::new()
    }
}

// =========================================================================
// Raw (unexpanded) intermediate types for multi-file merge pipeline
// =========================================================================

/// Source location for a directive, used in error reporting.
#[derive(Debug, Clone)]
pub struct Source {
    pub line: usize,
    pub file: usize,
}

impl Source {
    pub fn new(line: usize, file: usize) -> Self {
        Source { line, file }
    }
}

/// A clearable list field that tracks whether base values should be discarded.
#[derive(Debug, Clone)]
pub struct ClearableVec<T> {
    pub values: Vec<T>,
    pub cleared: bool,
}

impl<T> ClearableVec<T> {
    pub fn new() -> Self {
        ClearableVec {
            values: Vec::new(),
            cleared: false,
        }
    }
}

impl<T> Default for ClearableVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A clearable map field that tracks whether base values should be discarded.
#[derive(Debug, Clone)]
pub struct ClearableMap {
    pub values: HashMap<String, String>,
    pub cleared: bool,
}

impl ClearableMap {
    pub fn new() -> Self {
        ClearableMap {
            values: HashMap::new(),
            cleared: false,
        }
    }
}

impl Default for ClearableMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw service representation with unexpanded values and merge metadata.
/// All typed fields (bools, numbers, enums) are stored as raw strings.
/// Parsing and validation deferred to the resolve stage.
#[derive(Debug, Clone)]
pub struct RawService {
    pub name: String,
    pub file_index: usize,

    // Mode directives (scalars with provenance for C1 errors)
    pub from: Option<String>,
    pub from_source: Option<Source>,
    pub run: Option<String>,
    pub run_source: Option<Source>,

    // Container-only scalars
    pub entrypoint: Option<String>,
    pub cmd: Option<String>,

    // Container-only keyed lists (key: container_port / destination)
    pub publish: ClearableVec<(String, String, String)>, // (address_raw, host_port_raw, container_port_raw)
    pub volumes: ClearableVec<(String, String)>, // (source_raw, dest_raw)

    // Host-only scalars
    pub user: Option<String>,
    pub stop_command: Option<String>,
    pub reload_command: Option<String>,

    // Common scalars
    pub workdir: Option<String>,
    pub healthcheck: Option<String>,
    pub readiness_timeout: Option<String>,
    pub oneshot: Option<String>,
    pub disabled: Option<String>,
    pub recreate: Option<String>,

    // Common keyed map
    pub env: ClearableMap,

    // Common positional lists
    pub env_files: ClearableVec<String>,
    pub requires: ClearableVec<String>,
    pub after: ClearableVec<String>,

    // Restart config (all scalars, stored as raw strings)
    pub restart: Option<String>,
    pub restart_delay: Option<String>,
    pub start_limit_burst: Option<String>,
    pub start_limit_interval: Option<String>,

    // Timeouts (scalars)
    pub timeout_start: Option<String>,
    pub timeout_stop: Option<String>,

    // Resources (scalars, stored as raw strings)
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub cpu_quota: Option<String>,
    pub limit_nofile: Option<String>,
    pub limit_nproc: Option<String>,
    pub tasks_max: Option<String>,
    pub io_weight: Option<String>,

    // Logging (scalars)
    pub stdout: Option<String>,
    pub stderr: Option<String>,

    // RFC 0001: template support
    pub template: bool,  // true if service name ends with '@'

    // C2/C3 provenance tracking (directive_name, line, file_index)
    pub container_directives_used: Vec<(String, usize, usize)>,
    pub host_directives_used: Vec<(String, usize, usize)>,

    // RFC 0001: state machine membership
    pub state: ClearableVec<String>,
    pub group: Option<String>,
    pub slice: Option<String>,

    // RFC 0001: dependencies (REQUIRES_HEALTHY)
    pub requires_healthy: ClearableVec<String>,

    // RFC 0001: health and lifecycle probes (raw strings, stored as-is)
    pub startup: Option<String>,
    pub liveness: Option<String>,
    pub readiness: Option<String>,
    pub ready_mode: Option<String>,
    pub watchdog: Option<String>,
    pub lifecycle: Option<String>,
    pub session: Option<String>,

    // RFC 0001: conditions and failure actions (predicates stored as raw strings)
    pub condition: Option<String>,
    pub assert: Option<String>,
    pub window: Option<String>,
    pub on_failure: Option<String>,

    // RFC 0001: node-local requirements
    pub arch: Option<String>,
    pub device: Option<String>,
    pub requires_cap: ClearableVec<String>,

    // RFC 0001: security and trust
    pub capability: Option<String>,
    pub readonly_root: Option<String>,
    pub no_new_privileges: Option<String>,
    pub private_tmp: Option<String>,
    pub seccomp: Option<String>,
    pub ephemeral: Option<String>,
    pub on_tamper: Option<String>,
    pub secret: Option<String>,
    pub identity: Option<String>,
    pub audit: Option<String>,

    // RFC 0001: change and observability (key-value pairs as raw strings)
    pub update: ClearableVec<String>,
    pub rollout: ClearableVec<String>,
    pub metrics: Option<String>,
    pub traces: Option<String>,
    pub log_format: Option<String>,

    // RFC 0001: open facets (key-value pairs as raw strings)
    pub assurance: ClearableVec<String>,
    pub label: ClearableVec<String>,
    pub profile: Option<String>,

    // RFC 0001: scaling
    pub instances: Option<String>,
}

impl RawService {
    pub fn new(name: String, file_index: usize) -> Self {
        Self::new_template(name, file_index, false)
    }

    pub fn new_template(name: String, file_index: usize, template: bool) -> Self {
        RawService {
            name,
            file_index,
            from: None,
            from_source: None,
            run: None,
            run_source: None,
            entrypoint: None,
            cmd: None,
            publish: ClearableVec::new(),
            volumes: ClearableVec::new(),
            user: None,
            stop_command: None,
            reload_command: None,
            workdir: None,
            healthcheck: None,
            readiness_timeout: None,
            oneshot: None,
            disabled: None,
            recreate: None,
            env: ClearableMap::new(),
            env_files: ClearableVec::new(),
            requires: ClearableVec::new(),
            after: ClearableVec::new(),
            restart: None,
            restart_delay: None,
            start_limit_burst: None,
            start_limit_interval: None,
            timeout_start: None,
            timeout_stop: None,
            memory: None,
            cpus: None,
            cpu_quota: None,
            limit_nofile: None,
            limit_nproc: None,
            tasks_max: None,
            io_weight: None,
            stdout: None,
            stderr: None,
            template,
            state: ClearableVec::new(),
            group: None,
            slice: None,
            requires_healthy: ClearableVec::new(),
            startup: None,
            liveness: None,
            readiness: None,
            ready_mode: None,
            watchdog: None,
            lifecycle: None,
            session: None,
            condition: None,
            assert: None,
            window: None,
            on_failure: None,
            arch: None,
            device: None,
            requires_cap: ClearableVec::new(),
            capability: None,
            readonly_root: None,
            no_new_privileges: None,
            private_tmp: None,
            seccomp: None,
            ephemeral: None,
            on_tamper: None,
            secret: None,
            identity: None,
            audit: None,
            update: ClearableVec::new(),
            rollout: ClearableVec::new(),
            metrics: None,
            traces: None,
            log_format: None,
            assurance: ClearableVec::new(),
            label: ClearableVec::new(),
            profile: None,
            instances: None,
            container_directives_used: Vec::new(),
            host_directives_used: Vec::new(),
        }
    }
}

/// Raw parse result before variable expansion and validation.
#[derive(Debug, Clone)]
pub struct RawOrchFile {
    pub args: HashMap<String, String>,
    // RFC 0001: file-global machine state declarations
    pub machine_states: Vec<String>,
    pub default_state: Option<String>,
    pub services: Vec<RawService>,
}
