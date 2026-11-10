use std::fmt::Display;

use crate::syntax::Directive;
use crate::types::{
    OrchFile, PortMapping, RecreatePolicy, RestartPolicy, Service, ServiceMode, VolumeMount,
};

pub fn render(orch: &OrchFile) -> String {
    let mut blocks = vec![format!("ORCH_VERSION {}\n", orch.version)];
    blocks.extend(orch.services.iter().map(service_block));
    blocks.join("\n")
}

fn service_block(service: &Service) -> String {
    let mut block = Block::named(&service.name);
    match service.mode {
        ServiceMode::Container => block.optional(Directive::From, &service.image),
        ServiceMode::Host => block.optional(Directive::Run, &service.run_command),
    }
    block.optional(Directive::Entrypoint, &service.entrypoint);
    block.optional(Directive::Cmd, &service.cmd);
    block.each(Directive::Publish, service.publish.iter().map(port_mapping));
    block.each(Directive::Volume, service.volumes.iter().map(volume_mount));
    block.optional(Directive::User, &service.user);
    block.optional(Directive::Stop, &service.stop_command);
    block.optional(Directive::Reload, &service.reload_command);
    block.optional(Directive::Workdir, &service.workdir);
    block.each(
        Directive::Env,
        service
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    block.each(Directive::EnvFile, &service.env_files);
    block.joined(Directive::Requires, &service.requires);
    block.joined(Directive::After, &service.after);
    block.optional(Directive::Healthcheck, &service.healthcheck);
    block.optional(Directive::ReadinessTimeout, &service.readiness_timeout);
    block.when(service.oneshot, Directive::Oneshot, "true");
    block.when(service.disabled, Directive::Disabled, "true");
    block.when(
        service.recreate == RecreatePolicy::Always,
        Directive::Recreate,
        "always",
    );
    block.optional(Directive::Restart, &restart_policy(&service.restart.policy));
    block.optional(Directive::RestartDelay, &service.restart.delay);
    block.optional(
        Directive::StartLimitBurst,
        &service.restart.start_limit_burst,
    );
    block.optional(
        Directive::StartLimitInterval,
        &service.restart.start_limit_interval,
    );
    block.optional(Directive::TimeoutStart, &service.timeouts.start);
    block.optional(Directive::TimeoutStop, &service.timeouts.stop);
    block.optional(Directive::Memory, &service.resources.memory);
    block.optional(Directive::Cpus, &service.resources.cpus);
    block.optional(Directive::CpuQuota, &service.resources.cpu_quota);
    block.optional(Directive::LimitNofile, &service.resources.limit_nofile);
    block.optional(Directive::LimitNproc, &service.resources.limit_nproc);
    block.optional(Directive::TasksMax, &service.resources.tasks_max);
    block.optional(Directive::IoWeight, &service.resources.io_weight);
    block.optional(Directive::Stdout, &service.logging.stdout);
    block.optional(Directive::Stderr, &service.logging.stderr);
    block.text
}

struct Block {
    text: String,
}

impl Block {
    fn named(service: &str) -> Self {
        Block {
            text: format!("SERVICE {service}\n"),
        }
    }

    fn line(&mut self, directive: Directive, value: impl Display) {
        let escaped = value.to_string().replace("${", "$${");
        self.text
            .push_str(&format!("{} {escaped}\n", directive.keyword()));
    }

    fn optional<V: Display>(&mut self, directive: Directive, value: &Option<V>) {
        if let Some(value) = value {
            self.line(directive, value);
        }
    }

    fn when(&mut self, condition: bool, directive: Directive, value: &str) {
        if condition {
            self.line(directive, value);
        }
    }

    fn each<V: Display>(&mut self, directive: Directive, values: impl IntoIterator<Item = V>) {
        for value in values {
            self.line(directive, value);
        }
    }

    fn joined(&mut self, directive: Directive, names: &[String]) {
        if !names.is_empty() {
            self.line(directive, names.join(" "));
        }
    }
}

fn port_mapping(mapping: &PortMapping) -> String {
    match &mapping.address {
        Some(address) => format!("{address}:{}:{}", mapping.host, mapping.container),
        None => format!("{}:{}", mapping.host, mapping.container),
    }
}

fn volume_mount(volume: &VolumeMount) -> String {
    let read_only_suffix = if volume.read_only { ":ro" } else { "" };
    format!("{}:{}{read_only_suffix}", volume.source, volume.destination)
}

fn restart_policy(policy: &RestartPolicy) -> Option<&'static str> {
    match policy {
        RestartPolicy::No => None,
        RestartPolicy::Always => Some("always"),
        RestartPolicy::OnFailure => Some("on-failure"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::parser::{parse, parse_files};

    fn canonical(input: &str) -> String {
        render(&parse(input, &HashMap::new()).expect("input should parse"))
    }

    const EVERY_DIRECTIVE: &str = "\
ARG tag=15
SERVICE db
FROM postgres:${tag}
ENTRYPOINT /docker-entrypoint.sh
CMD postgres -c shared_buffers=1G
PUBLISH 127.0.0.1:5433:5432
PUBLISH ::1:5433:5432
VOLUME pgdata:/var/lib/postgresql/data
VOLUME ~/config:/etc/postgresql:ro
WORKDIR /var/lib/postgresql
ENV POSTGRES_DB=orch
ENV EMPTY=
ENV_FILE ~/.env.local
HEALTHCHECK pg_isready
READINESS_TIMEOUT 30s
RECREATE always
RESTART on-failure
RESTART_DELAY 5s
START_LIMIT_BURST 3
START_LIMIT_INTERVAL 1m
TIMEOUT_START 60s
TIMEOUT_STOP 10s
MEMORY 2G
CPUS 1.5
CPU_QUOTA 150%
LIMIT_NOFILE 65536
LIMIT_NPROC 4096
TASKS_MAX 512
IO_WEIGHT 200
STDOUT /var/log/db.log
STDERR /var/log/db.err
SERVICE migrate
RUN sh -c 'echo $${HOME}'
USER _postgres
STOP kill -TERM $(cat /run/migrate.pid)
RELOAD kill -HUP $(cat /run/migrate.pid)
REQUIRES db
AFTER cache db
ONESHOT true
DISABLED true
";

    #[test]
    fn canonical_form_is_a_fixed_point() {
        let once = canonical(EVERY_DIRECTIVE);
        assert_eq!(canonical(&once), once);
    }

    #[test]
    fn canonical_form_parses_to_the_same_services() {
        let original = parse(EVERY_DIRECTIVE, &HashMap::new()).unwrap();
        let reparsed = parse(&render(&original), &HashMap::new()).unwrap();
        assert_eq!(
            serde_json::to_value(&reparsed.services).unwrap(),
            serde_json::to_value(&original.services).unwrap()
        );
    }

    #[test]
    fn variables_are_expanded_and_literal_references_stay_escaped() {
        let text = canonical(EVERY_DIRECTIVE);
        assert!(text.contains("FROM postgres:15\n"));
        assert!(text.contains("RUN sh -c 'echo $${HOME}'\n"));
        assert!(!text.contains("ARG "));
    }

    #[test]
    fn defaults_are_omitted_and_env_is_sorted() {
        let text = canonical("SERVICE a\nRUN x\nENV B=2\nENV A=1\nRESTART no\nONESHOT false\n");
        assert_eq!(
            text,
            "ORCH_VERSION 1.0.0-rc\n\nSERVICE a\nRUN x\nENV A=1\nENV B=2\n"
        );
    }

    #[test]
    fn overlays_collapse_into_one_block_per_service() {
        let merged = parse_files(
            &[
                ("base.orch", "SERVICE web\nFROM nginx\nPUBLISH 8080:80\n"),
                (
                    "over.orch",
                    "SERVICE web\nPUBLISH 9090:80\nENV MODE=staging\n",
                ),
            ],
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            render(&merged),
            "ORCH_VERSION 1.0.0-rc\n\nSERVICE web\nFROM nginx\nPUBLISH 9090:80\nENV MODE=staging\n"
        );
    }
}
