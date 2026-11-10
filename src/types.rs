use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ORCH_VERSION: &str = "1.0.0-rc";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Container,
    Host,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    No,
    Always,
    OnFailure,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecreatePolicy {
    Always,
    #[default]
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
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
    #[serde(default)]
    pub read_only: bool,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
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
            env: BTreeMap::new(),
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

    pub fn is_host(&self) -> bool {
        self.mode == ServiceMode::Host
    }

    pub fn is_container(&self) -> bool {
        self.mode == ServiceMode::Container
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchFile {
    pub version: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    #[serde(default)]
    pub services: Vec<Service>,
}

impl OrchFile {
    pub fn new() -> Self {
        OrchFile {
            version: ORCH_VERSION.to_string(),
            args: BTreeMap::new(),
            services: Vec::new(),
        }
    }
}

impl Default for OrchFile {
    fn default() -> Self {
        Self::new()
    }
}
