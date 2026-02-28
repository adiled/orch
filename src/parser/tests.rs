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
    let input = "ARG db=canary\nSERVICE app\nRUN echo hi\nENV DB_NAME=${db}\n";
    let orch = parse_ok(input);
    assert_eq!(orch.services[0].env["DB_NAME"], "canary");
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
    let orch = parse_ok("SERVICE x\nFROM img\nENV URL=postgres://host:5432/db?sslmode=require\n");
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
    assert!(orch.services[0]
        .healthcheck
        .as_ref()
        .unwrap()
        .starts_with("http://"));
}

#[test]
fn test_common__readiness_timeout() {
    let orch = parse_ok("SERVICE x\nFROM img\nREADINESS_TIMEOUT 120s\n");
    assert_eq!(
        orch.services[0].readiness_timeout.as_ref().unwrap(),
        "120s"
    );
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
        orch.services[0].restart.start_limit_interval.as_ref().unwrap(),
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
ARG postgres_port=5433
ARG postgres_memory=4G
ARG django_port=9090

SERVICE postgres
FROM pgvector/pgvector:pg15
MEMORY ${postgres_memory}
CPUS 2
PUBLISH ${postgres_port}:5432
VOLUME postgres-data:/var/lib/postgresql/data
ENV POSTGRES_USER=postgres
ENV POSTGRES_PASSWORD=canary
HEALTHCHECK pg_isready -h localhost -p ${postgres_port}
RESTART on-failure
RESTART_DELAY 5s

SERVICE redis
FROM redis:6.2.0-alpine
MEMORY 1G
CPUS 1
PUBLISH 6380:6379
RECREATE always
HEALTHCHECK redis-cli -h localhost -p 6380 ping
RESTART always

SERVICE django
RUN python manage.py runserver 0.0.0.0:${django_port}
WORKDIR backend/canary
ENV DJANGO_SETTINGS_MODULE=canary.settings.dev
ENV_FILE ${ORCH_PROJECT}/.env.local
REQUIRES postgres redis
AFTER localstack
HEALTHCHECK http://localhost:${django_port}/health
RESTART on-failure
RESTART_DELAY 2s
MEMORY 2G
LIMIT_NOFILE 65536
TIMEOUT_START 60s

SERVICE db-migrate
FROM flyway/flyway:latest
CMD -url=jdbc:postgresql://postgres/canary migrate
REQUIRES postgres
ONESHOT true
"#;
    let orch = parse_ok(input);

    assert_eq!(orch.services.len(), 4);

    // postgres
    let pg = &orch.services[0];
    assert_eq!(pg.name, "postgres");
    assert_eq!(pg.mode, ServiceMode::Container);
    assert_eq!(pg.image.as_ref().unwrap(), "pgvector/pgvector:pg15");
    assert_eq!(pg.resources.memory.as_ref().unwrap(), "4G");
    assert_eq!(pg.resources.cpus.unwrap(), 2.0);
    assert_eq!(pg.publish[0].host, 5433);
    assert_eq!(pg.publish[0].container, 5432);
    assert!(pg.volumes[0].is_named);
    assert_eq!(pg.env["POSTGRES_USER"], "postgres");
    assert_eq!(pg.restart.policy, RestartPolicy::OnFailure);
    assert_eq!(pg.restart.delay.as_ref().unwrap(), "5s");

    // redis
    let redis = &orch.services[1];
    assert_eq!(redis.name, "redis");
    assert_eq!(redis.recreate, RecreatePolicy::Always);
    assert_eq!(redis.restart.policy, RestartPolicy::Always);

    // django
    let dj = &orch.services[2];
    assert_eq!(dj.name, "django");
    assert_eq!(dj.mode, ServiceMode::Host);
    assert_eq!(
        dj.run_command.as_ref().unwrap(),
        "python manage.py runserver 0.0.0.0:9090"
    );
    assert_eq!(dj.workdir.as_ref().unwrap(), "backend/canary");
    assert_eq!(dj.requires, vec!["postgres", "redis"]);
    assert_eq!(dj.after, vec!["localstack"]);
    assert_eq!(dj.resources.limit_nofile.unwrap(), 65536);
    assert_eq!(dj.timeouts.start.as_ref().unwrap(), "60s");

    // db-migrate
    let mig = &orch.services[3];
    assert_eq!(mig.name, "db-migrate");
    assert!(mig.oneshot);
    assert_eq!(mig.requires, vec!["postgres"]);
}

// =========================================================================
// JSON serialization
// =========================================================================

#[test]
fn test_json_serialization() {
    let orch = parse_ok("SERVICE db\nFROM postgres:15\nPUBLISH 5432:5432\n");
    let json = serde_json::to_string_pretty(&orch).unwrap();
    assert!(json.contains("\"version\": \"0.1.0\""));
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
