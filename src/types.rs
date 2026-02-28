use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Container,
    Host,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct VolumeMount {
    pub source: String,
    pub destination: String,
    pub is_named: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
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

#[derive(Debug, Clone, Serialize, Default)]
pub struct RestartConfig {
    pub policy: RestartPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_limit_burst: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_limit_interval: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TimeoutConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LogConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub publish: Vec<PortMapping>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<VolumeMount>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_command: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_files: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
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
}

#[derive(Debug, Clone, Serialize)]
pub struct OrchFile {
    pub version: String,
    pub args: HashMap<String, String>,
    pub services: Vec<Service>,
}

impl OrchFile {
    pub fn new() -> Self {
        OrchFile {
            version: "0.1.0".to_string(),
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
