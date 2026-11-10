use std::collections::{BTreeMap, HashMap, HashSet};

use crate::error::{OrchError, ValidationError};
use crate::syntax::{Directive, ListField, mode_keyword};
use crate::types::{Service, ServiceMode, VolumeMount};
use crate::value::{Setting, TypedBlock, TypedEntry};

#[derive(Clone, Copy)]
struct Origin {
    file: usize,
    line: usize,
}

struct Draft {
    service: Service,
    mode: Option<ServiceMode>,
    first_block_file: usize,
    mode_bound_origins: BTreeMap<Directive, Origin>,
}

pub(crate) fn merged_services(
    blocks: Vec<TypedBlock>,
    file_names: &[Option<String>],
) -> Result<Vec<Service>, Vec<OrchError>> {
    let mut drafts: Vec<Draft> = Vec::new();
    let mut draft_index: HashMap<String, usize> = HashMap::new();

    for block in blocks {
        let index = *draft_index.entry(block.service.clone()).or_insert_with(|| {
            drafts.push(Draft::new(&block.service, block.file));
            drafts.len() - 1
        });
        drafts[index].apply(block);
    }

    let mut services = Vec::new();
    let mut errors: Vec<OrchError> = Vec::new();
    for draft in drafts {
        match draft.finish(file_names) {
            Ok(service) => services.push(service),
            Err(found) => errors.extend(found.into_iter().map(OrchError::from)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let graph_errors: Vec<OrchError> = dependency_cycles(&services)
        .into_iter()
        .chain(unknown_requirements(&services))
        .map(OrchError::from)
        .collect();
    if graph_errors.is_empty() {
        Ok(services)
    } else {
        Err(graph_errors)
    }
}

impl Draft {
    fn new(name: &str, first_block_file: usize) -> Self {
        Draft {
            service: Service::new(name.to_string(), ServiceMode::Container),
            mode: None,
            first_block_file,
            mode_bound_origins: BTreeMap::new(),
        }
    }

    fn apply(&mut self, block: TypedBlock) {
        if let Some(mode) = block.mode {
            self.switch_mode(mode);
        }
        for entry in &block.entries {
            if let Setting::Clear(field) = entry.setting {
                self.clear(field);
            }
        }
        self.drop_republished_ports(&block.entries);
        for entry in block.entries {
            self.remember_mode_bound_origin(&entry, block.file);
            self.set(entry.setting);
        }
    }

    fn switch_mode(&mut self, mode: ServiceMode) {
        let service = &mut self.service;
        match mode {
            ServiceMode::Container => {
                service.user = None;
                service.stop_command = None;
                service.reload_command = None;
            }
            ServiceMode::Host => {
                service.entrypoint = None;
                service.cmd = None;
                service.publish.clear();
                service.volumes.clear();
            }
        }
        self.mode_bound_origins
            .retain(|directive, _| !directive.placement().excludes(mode));
        self.mode = Some(mode);
    }

    fn clear(&mut self, field: ListField) {
        let service = &mut self.service;
        match field {
            ListField::Env => service.env.clear(),
            ListField::EnvFile => service.env_files.clear(),
            ListField::Publish => service.publish.clear(),
            ListField::Volume => service.volumes.clear(),
            ListField::Requires => service.requires.clear(),
            ListField::After => service.after.clear(),
        }
    }

    fn drop_republished_ports(&mut self, entries: &[TypedEntry]) {
        let republished: HashSet<u16> = entries
            .iter()
            .filter_map(|entry| match &entry.setting {
                Setting::Publish(mapping) => Some(mapping.container),
                _ => None,
            })
            .collect();
        self.service
            .publish
            .retain(|mapping| !republished.contains(&mapping.container));
    }

    fn remember_mode_bound_origin(&mut self, entry: &TypedEntry, file: usize) {
        let is_mode_bound = [ServiceMode::Container, ServiceMode::Host]
            .into_iter()
            .any(|mode| entry.directive.placement().excludes(mode));
        if is_mode_bound {
            self.mode_bound_origins.insert(
                entry.directive,
                Origin {
                    file,
                    line: entry.line,
                },
            );
        }
    }

    fn set(&mut self, setting: Setting) {
        let service = &mut self.service;
        match setting {
            Setting::Image(image) => {
                service.image = Some(image);
                service.run_command = None;
            }
            Setting::RunCommand(command) => {
                service.run_command = Some(command);
                service.image = None;
            }
            Setting::Entrypoint(entrypoint) => service.entrypoint = Some(entrypoint),
            Setting::Cmd(cmd) => service.cmd = Some(cmd),
            Setting::Publish(mapping) => service.publish.push(mapping),
            Setting::Volume(volume) => mount(&mut service.volumes, volume),
            Setting::User(user) => service.user = Some(user),
            Setting::StopCommand(command) => service.stop_command = Some(command),
            Setting::ReloadCommand(command) => service.reload_command = Some(command),
            Setting::Workdir(workdir) => service.workdir = Some(workdir),
            Setting::Env { key, value } => {
                service.env.insert(key, value);
            }
            Setting::EnvFile(path) => push_unique(&mut service.env_files, path),
            Setting::Requires(names) => names
                .into_iter()
                .for_each(|name| push_unique(&mut service.requires, name)),
            Setting::After(names) => names
                .into_iter()
                .for_each(|name| push_unique(&mut service.after, name)),
            Setting::Healthcheck(check) => service.healthcheck = Some(check),
            Setting::ReadinessTimeout(timeout) => service.readiness_timeout = Some(timeout),
            Setting::Oneshot(oneshot) => service.oneshot = oneshot,
            Setting::Disabled(disabled) => service.disabled = disabled,
            Setting::Recreate(policy) => service.recreate = policy,
            Setting::Restart(policy) => service.restart.policy = policy,
            Setting::RestartDelay(delay) => service.restart.delay = Some(delay),
            Setting::StartLimitBurst(burst) => service.restart.start_limit_burst = Some(burst),
            Setting::StartLimitInterval(interval) => {
                service.restart.start_limit_interval = Some(interval)
            }
            Setting::TimeoutStart(timeout) => service.timeouts.start = Some(timeout),
            Setting::TimeoutStop(timeout) => service.timeouts.stop = Some(timeout),
            Setting::Memory(memory) => service.resources.memory = Some(memory),
            Setting::Cpus(cpus) => service.resources.cpus = Some(cpus),
            Setting::CpuQuota(quota) => service.resources.cpu_quota = Some(quota),
            Setting::LimitNofile(limit) => service.resources.limit_nofile = Some(limit),
            Setting::LimitNproc(limit) => service.resources.limit_nproc = Some(limit),
            Setting::TasksMax(max) => service.resources.tasks_max = Some(max),
            Setting::IoWeight(weight) => service.resources.io_weight = Some(weight),
            Setting::Stdout(path) => service.logging.stdout = Some(path),
            Setting::Stderr(path) => service.logging.stderr = Some(path),
            Setting::Clear(_) => {}
        }
    }

    fn finish(mut self, file_names: &[Option<String>]) -> Result<Service, Vec<ValidationError>> {
        let name = self.service.name.clone();
        let Some(mode) = self.mode else {
            let error = ValidationError::new(
                &name,
                "no block sets FROM or RUN, so the service has no mode (C1)",
            );
            return Err(vec![with_file(error, file_names, self.first_block_file)]);
        };

        let misplaced: Vec<ValidationError> = self
            .mode_bound_origins
            .iter()
            .filter(|(directive, _)| directive.placement().excludes(mode))
            .map(|(directive, origin)| {
                let constraint = match mode {
                    ServiceMode::Host => "C2",
                    ServiceMode::Container => "C3",
                };
                ValidationError::new(
                    &name,
                    format!(
                        "{} at {} is {}, but the merged service sets {} ({constraint})",
                        directive.keyword(),
                        location(file_names, *origin),
                        directive.placement().describe(),
                        mode_keyword(mode),
                    ),
                )
            })
            .collect();
        if !misplaced.is_empty() {
            return Err(misplaced);
        }

        self.service.mode = mode;
        Ok(self.service)
    }
}

fn mount(volumes: &mut Vec<VolumeMount>, volume: VolumeMount) {
    match volumes
        .iter_mut()
        .find(|mounted| mounted.destination == volume.destination)
    {
        Some(mounted) => *mounted = volume,
        None => volumes.push(volume),
    }
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn with_file(
    error: ValidationError,
    file_names: &[Option<String>],
    file: usize,
) -> ValidationError {
    match file_names.get(file).and_then(Option::as_deref) {
        Some(name) => error.with_file(name),
        None => error,
    }
}

fn location(file_names: &[Option<String>], origin: Origin) -> String {
    match file_names.get(origin.file).and_then(Option::as_deref) {
        Some(name) => format!("{name}:{}", origin.line),
        None => format!("line {}", origin.line),
    }
}

fn unknown_requirements(services: &[Service]) -> Vec<ValidationError> {
    let known: HashSet<&str> = services.iter().map(|s| s.name.as_str()).collect();
    services
        .iter()
        .flat_map(|service| {
            service
                .requires
                .iter()
                .filter(|required| !known.contains(required.as_str()))
                .map(|required| {
                    ValidationError::new(
                        &service.name,
                        format!("REQUIRES references unknown service '{required}'"),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Visit {
    Unvisited,
    InProgress,
    Done,
}

struct CycleSearch<'a> {
    dependencies: HashMap<&'a str, Vec<&'a str>>,
    visits: HashMap<&'a str, Visit>,
    path: Vec<&'a str>,
    cycles: Vec<ValidationError>,
}

fn dependency_cycles(services: &[Service]) -> Vec<ValidationError> {
    let known: HashSet<&str> = services.iter().map(|s| s.name.as_str()).collect();
    let dependencies = services
        .iter()
        .map(|service| {
            let targets = service
                .requires
                .iter()
                .chain(&service.after)
                .map(String::as_str)
                .filter(|target| known.contains(target))
                .collect();
            (service.name.as_str(), targets)
        })
        .collect();
    let mut search = CycleSearch {
        dependencies,
        visits: known.iter().map(|&name| (name, Visit::Unvisited)).collect(),
        path: Vec::new(),
        cycles: Vec::new(),
    };
    for service in services {
        if search.visits[service.name.as_str()] == Visit::Unvisited {
            search.visit(&service.name);
        }
    }
    search.cycles
}

impl<'a> CycleSearch<'a> {
    fn visit(&mut self, node: &'a str) {
        self.visits.insert(node, Visit::InProgress);
        self.path.push(node);

        for target in self.dependencies.get(node).cloned().unwrap_or_default() {
            match self.visits[target] {
                Visit::InProgress => self.report_cycle_closing_at(target),
                Visit::Unvisited => self.visit(target),
                Visit::Done => {}
            }
        }

        self.path.pop();
        self.visits.insert(node, Visit::Done);
    }

    fn report_cycle_closing_at(&mut self, target: &'a str) {
        let start = self
            .path
            .iter()
            .position(|&node| node == target)
            .unwrap_or(0);
        let cycle = self.path[start..].join(" -> ");
        self.cycles.push(ValidationError::new(
            target,
            format!("dependency cycle detected: {cycle} -> {target}"),
        ));
    }
}
