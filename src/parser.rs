use std::collections::{BTreeMap, HashMap};

use crate::error::{OrchError, ParseError};
use crate::lex;
use crate::merge::merged_services;
use crate::syntax::{self, Document};
use crate::types::*;
use crate::value::{TypedBlock, typed_block};

pub fn parse(input: &str, overrides: &HashMap<String, String>) -> Result<OrchFile, Vec<OrchError>> {
    parse_sources(
        &[Source {
            name: None,
            content: input,
        }],
        overrides,
    )
}

pub fn parse_files(
    files: &[(&str, &str)],
    overrides: &HashMap<String, String>,
) -> Result<OrchFile, Vec<OrchError>> {
    let sources: Vec<Source> = files
        .iter()
        .map(|&(name, content)| Source {
            name: Some(name),
            content,
        })
        .collect();
    parse_sources(&sources, overrides)
}

struct Source<'a> {
    name: Option<&'a str>,
    content: &'a str,
}

impl Source<'_> {
    fn located(&self, errors: Vec<ParseError>) -> Vec<OrchError> {
        errors
            .into_iter()
            .map(|error| match self.name {
                Some(name) => error.with_file(name).into(),
                None => error.into(),
            })
            .collect()
    }
}

fn parse_sources(
    sources: &[Source],
    overrides: &HashMap<String, String>,
) -> Result<OrchFile, Vec<OrchError>> {
    let documents = documents(sources)?;
    let args = resolved_args(&documents, overrides);
    let blocks = typed_blocks(sources, documents, &args)?;
    let file_names: Vec<Option<String>> = sources
        .iter()
        .map(|source| source.name.map(str::to_string))
        .collect();
    let services = merged_services(blocks, &file_names)?;
    Ok(OrchFile {
        version: ORCH_VERSION.to_string(),
        args,
        services,
    })
}

fn documents(sources: &[Source]) -> Result<Vec<Document>, Vec<OrchError>> {
    let mut documents = Vec::new();
    let mut errors = Vec::new();
    for source in sources {
        match lex::statements(source.content).and_then(syntax::document) {
            Ok(document) => documents.push(document),
            Err(found) => errors.extend(source.located(found)),
        }
    }
    if errors.is_empty() {
        Ok(documents)
    } else {
        Err(errors)
    }
}

fn resolved_args(
    documents: &[Document],
    overrides: &HashMap<String, String>,
) -> BTreeMap<String, String> {
    let mut args: BTreeMap<String, String> = documents
        .iter()
        .flat_map(|document| &document.args)
        .map(|arg| (arg.name.clone(), arg.default.clone()))
        .collect();
    for (name, value) in overrides {
        if let Some(declared) = args.get_mut(name) {
            *declared = value.clone();
        }
    }
    args
}

fn typed_blocks(
    sources: &[Source],
    documents: Vec<Document>,
    args: &BTreeMap<String, String>,
) -> Result<Vec<TypedBlock>, Vec<OrchError>> {
    let mut blocks = Vec::new();
    let mut errors = Vec::new();
    for (file, (source, document)) in sources.iter().zip(documents).enumerate() {
        for block in document.blocks {
            match typed_block(block, file, args) {
                Ok(block) => blocks.push(block),
                Err(found) => errors.extend(source.located(found)),
            }
        }
    }
    if errors.is_empty() {
        Ok(blocks)
    } else {
        Err(errors)
    }
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
    fn test_expansion__undeclared_var_is_error() {
        let input = "SERVICE app\nRUN echo hi\nSTDOUT ${ORCH_STATE_DIR}/logs/app.log\n";
        let errors = parse_err(input);
        assert_eq!(errors.len(), 1);
        let msg = errors[0].to_string();
        assert!(msg.contains("line 3"), "{msg}");
        assert!(
            msg.contains("undefined variable '${ORCH_STATE_DIR}'"),
            "{msg}"
        );
    }

    #[test]
    fn test_expansion__undeclared_var_reported_before_type_error() {
        let errors = parse_err("SERVICE x\nFROM img\nPUBLISH ${port}:80\n");
        assert!(
            errors[0]
                .to_string()
                .contains("undefined variable '${port}'")
        );
    }

    #[test]
    fn test_expansion__escape_is_literal() {
        let orch = parse_ok("SERVICE x\nRUN sh -c 'echo $${HOME}'\n");
        assert_eq!(
            orch.services[0].run_command.as_deref(),
            Some("sh -c 'echo ${HOME}'")
        );
    }

    #[test]
    fn test_expansion__plain_dollar_untouched() {
        let orch = parse_ok("SERVICE x\nRUN kill -HUP $MAINPID $$ $\n");
        assert_eq!(
            orch.services[0].run_command.as_deref(),
            Some("kill -HUP $MAINPID $$ $")
        );
    }

    #[test]
    fn test_expansion__unclosed_ref_is_parse_error_with_line() {
        let errors = parse_err("SERVICE x\nRUN echo ${oops\n");
        let msg = errors[0].to_string();
        assert!(msg.starts_with("parse error: line 2:"), "{msg}");
        assert!(msg.contains("unclosed"), "{msg}");
    }

    #[test]
    fn test_expansion__empty_ref_is_parse_error() {
        let errors = parse_err("SERVICE x\nRUN echo ${}\n");
        assert!(errors[0].to_string().contains("empty variable reference"));
    }

    #[test]
    fn test_expansion__override_value_is_literal() {
        let mut overrides = HashMap::new();
        overrides.insert("a".to_string(), "${not_a_ref}".to_string());
        let orch = parse_ok_with_args("ARG a=1\nSERVICE x\nRUN echo ${a}\n", &overrides);
        assert_eq!(
            orch.services[0].run_command.as_deref(),
            Some("echo ${not_a_ref}")
        );
    }

    #[test]
    fn test_arg__default_cannot_reference_var() {
        let errors = parse_err("ARG b=x\nARG a=${b}-y\nSERVICE x\nRUN echo ${a}\n");
        let msg = errors[0].to_string();
        assert!(msg.starts_with("parse error: line 2:"), "{msg}");
        assert!(
            msg.contains("ARG a: default cannot reference '${b}'"),
            "{msg}"
        );
    }

    #[test]
    fn test_arg__default_escape_is_literal() {
        let orch = parse_ok("ARG a=$${b}\nSERVICE x\nRUN echo ${a}\n");
        assert_eq!(orch.args["a"], "${b}");
        assert_eq!(orch.services[0].run_command.as_deref(), Some("echo ${b}"));
    }

    #[test]
    fn test_expansion__override_in_image() {
        let mut overrides = HashMap::new();
        overrides.insert("tag".to_string(), "16".to_string());
        let input = "ARG tag=15\nSERVICE db\nFROM postgres:${tag}\n";
        let orch = parse_ok_with_args(input, &overrides);
        assert_eq!(orch.services[0].image.as_ref().unwrap(), "postgres:16");
    }

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
        assert!(
            msgs.iter()
                .any(|m| m.contains("at most one of FROM and RUN"))
        );
    }

    #[test]
    fn test_c1__neither_from_nor_run() {
        let errors = parse_err("SERVICE empty\nENV FOO=bar\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("C1")));
    }

    #[test]
    fn test_c2__entrypoint_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nENTRYPOINT /bin/sh\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("container-only")));
    }

    #[test]
    fn test_c2__cmd_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nCMD args\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("container-only")));
    }

    #[test]
    fn test_c2__publish_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nPUBLISH 80:80\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("container-only")));
    }

    #[test]
    fn test_c2__volume_with_run() {
        let errors = parse_err("SERVICE bad\nRUN cmd\nVOLUME data:/data\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("container-only")));
    }

    #[test]
    fn test_c3__user_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nUSER postgres\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("host-only")));
    }

    #[test]
    fn test_c3__stop_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nSTOP kill -9\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("host-only")));
    }

    #[test]
    fn test_c3__reload_with_from() {
        let errors = parse_err("SERVICE bad\nFROM img\nRELOAD nginx -s reload\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("host-only")));
    }

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
        let orch =
            parse_ok("ARG data=/srv/data\nSERVICE x\nFROM img\nVOLUME ${data}/pg:/var/lib/data\n");
        assert!(!orch.services[0].volumes[0].is_named);
        assert_eq!(orch.services[0].volumes[0].source, "/srv/data/pg");
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
        let errors = parse_err("SERVICE x\nFROM img\nIO_WEIGHT 0\n");
        assert!(!errors.is_empty());

        let errors = parse_err("SERVICE x\nFROM img\nIO_WEIGHT 10001\n");
        assert!(!errors.is_empty());
    }

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

    #[test]
    fn test_requires__unknown_service() {
        let errors = parse_err("SERVICE app\nRUN cmd\nREQUIRES nonexistent\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("unknown service")));
    }

    #[test]
    fn test_after__unknown_service_allowed() {
        let orch = parse_ok("SERVICE app\nRUN cmd\nAFTER nonexistent\n");
        assert_eq!(orch.services[0].after, vec!["nonexistent"]);
    }

    #[test]
    fn test_unknown_directive() {
        let errors = parse_err("SERVICE x\nFROM img\nFOOBAR baz\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("unknown directive")));
    }

    #[test]
    fn test_directive_outside_service() {
        let errors = parse_err("FROM img\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(msgs.iter().any(|m| m.contains("outside of SERVICE block")));
    }

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
HEALTHCHECK http://localhost:9090/-/healthy
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
ENV_FILE ~/.env.local
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
        let loki = &orch.services[1];
        assert_eq!(loki.name, "loki");
        assert_eq!(loki.recreate, RecreatePolicy::Always);
        assert_eq!(loki.restart.policy, RestartPolicy::Always);
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
        let chk = &orch.services[3];
        assert_eq!(chk.name, "config-check");
        assert!(chk.oneshot);
        assert_eq!(chk.requires, vec!["prometheus"]);
    }

    #[test]
    fn test_json_serialization() {
        let orch = parse_ok("SERVICE db\nFROM postgres:15\nPUBLISH 5432:5432\n");
        let json = serde_json::to_string_pretty(&orch).unwrap();
        assert!(json.contains("\"version\": \"1.0.0-rc\""));
        assert!(json.contains("\"name\": \"db\""));
        assert!(json.contains("\"mode\": \"container\""));
        assert!(json.contains("\"host\": 5432"));
    }

    #[test]
    fn test_edge__duplicate_from() {
        let errors = parse_err("SERVICE x\nFROM img1\nFROM img2\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("at most one of FROM and RUN"))
        );
    }

    #[test]
    fn test_edge__duplicate_run() {
        let errors = parse_err("SERVICE x\nRUN cmd1\nRUN cmd2\n");
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("at most one of FROM and RUN"))
        );
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
    fn test_edge__indented_directives_are_rejected() {
        let errors = parse_err("SERVICE db\n  FROM postgres:15\n");
        assert!(errors[0].to_string().contains("column 0"));
    }

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
VOLUME ~/config:/etc/postgresql
    "#;
        let overlay = r#"
SERVICE db
VOLUME ~/local-config:/etc/postgresql
    "#;
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].volumes.len(), 2);
        let etc = orch.services[0]
            .volumes
            .iter()
            .find(|v| v.destination == "/etc/postgresql")
            .unwrap();
        assert_eq!(etc.source, "~/local-config");
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
        assert_eq!(orch.args.get("port").unwrap(), "9090");
        assert_eq!(orch.services[0].publish[0].host, 9090);
        assert_eq!(orch.services[0].env.get("APP_PORT").unwrap(), "9090");
    }

    #[test]
    fn test_multifile__arg_declared_only_in_overlay() {
        let base = "SERVICE web\nFROM nginx\nVOLUME ${data}/web:/data\n";
        let overlay = "ARG data=/srv/data\n";
        let orch = parse_files_ok(&[("base.orch", base), ("overlay.orch", overlay)]);
        assert_eq!(orch.services[0].volumes[0].source, "/srv/data/web");

        let errors = parse_files_err(&[("base.orch", base)]);
        assert!(
            errors[0]
                .to_string()
                .contains("undefined variable '${data}'")
        );
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
                .any(|m| m.contains("must be the first statement"))
        );
    }

    #[test]
    fn test_orch_version__duplicate_is_rejected() {
        let v = crate::types::ORCH_VERSION;
        let input = format!("ORCH_VERSION {v}\nORCH_VERSION {v}\nSERVICE db\nFROM img\n");
        let errors = parse_err(&input);
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("must be the first statement"))
        );
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
