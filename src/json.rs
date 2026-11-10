use serde::Serialize;

use crate::types::OrchFile;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Document<'a> {
    schema_version: u32,
    #[serde(flatten)]
    orch: &'a OrchFile,
}

pub fn render(orch: &OrchFile) -> String {
    let document = Document {
        schema_version: SCHEMA_VERSION,
        orch,
    };
    serde_json::to_string_pretty(&document).expect("an OrchFile always serializes to JSON")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::Value;

    use super::*;
    use crate::parser::parse;

    const SCHEMA: &str = include_str!("../schema/orch.schema.json");

    const EVERY_FIELD: &str = "\
ARG tag=15
SERVICE db
FROM postgres:${tag}
ENTRYPOINT /docker-entrypoint.sh
CMD postgres
PUBLISH 127.0.0.1:5433:5432
VOLUME pgdata:/var/lib/postgresql/data:ro
WORKDIR /var/lib/postgresql
ENV POSTGRES_DB=orch
ENV_FILE ~/.env.local
AFTER cache
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
RUN migrate up
USER _postgres
STOP kill -TERM 1
RELOAD kill -HUP 1
REQUIRES db
ONESHOT true
DISABLED true
";

    fn rendered(input: &str) -> Value {
        let orch = parse(input, &HashMap::new()).expect("input should parse");
        serde_json::from_str(&render(&orch)).expect("render emits JSON")
    }

    fn violations(value: &Value, schema: &Value, path: &str) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(expected) = schema.get("const")
            && value != expected
        {
            found.push(format!("{path}: expected {expected}, got {value}"));
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
            && !allowed.contains(value)
        {
            found.push(format!("{path}: {value} is not one of {allowed:?}"));
        }
        if let Some(expected) = schema.get("type").and_then(Value::as_str) {
            let matches = match expected {
                "object" => value.is_object(),
                "array" => value.is_array(),
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "integer" => value.is_u64() || value.is_i64(),
                "number" => value.is_number(),
                other => panic!("schema uses unsupported type {other}"),
            };
            if !matches {
                found.push(format!("{path}: expected {expected}, got {value}"));
            }
        }
        if let Some(fields) = value.as_object() {
            let properties = schema.get("properties").and_then(Value::as_object);
            for (name, field) in fields {
                let field_path = format!("{path}.{name}");
                match properties.and_then(|known| known.get(name)) {
                    Some(field_schema) => {
                        found.extend(violations(field, field_schema, &field_path))
                    }
                    None => match schema.get("additionalProperties") {
                        Some(Value::Bool(false)) | None => {
                            found.push(format!("{field_path}: not in the schema"))
                        }
                        Some(extra_schema) => {
                            found.extend(violations(field, extra_schema, &field_path))
                        }
                    },
                }
            }
            for required in schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !fields.contains_key(required) {
                    found.push(format!("{path}: missing {required}"));
                }
            }
        }
        if let (Some(items), Some(item_schema)) = (value.as_array(), schema.get("items")) {
            for (index, item) in items.iter().enumerate() {
                found.extend(violations(item, item_schema, &format!("{path}[{index}]")));
            }
        }
        found
    }

    fn schema_properties(schema: &Value, path: &str, into: &mut Vec<String>) {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                let property_path = format!("{path}.{name}");
                into.push(property_path.clone());
                schema_properties(property, &property_path, into);
            }
        }
        if let Some(items) = schema.get("items") {
            schema_properties(items, &format!("{path}[]"), into);
        }
    }

    fn emitted_fields(value: &Value, path: &str, into: &mut Vec<String>) {
        match value {
            Value::Object(fields) => {
                for (name, field) in fields {
                    let field_path = format!("{path}.{name}");
                    into.push(field_path.clone());
                    emitted_fields(field, &field_path, into);
                }
            }
            Value::Array(items) => {
                for item in items {
                    emitted_fields(item, &format!("{path}[]"), into);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn output_conforms_to_the_published_schema() {
        let schema: Value = serde_json::from_str(SCHEMA).expect("schema is JSON");
        let found = violations(&rendered(EVERY_FIELD), &schema, "$");
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn schema_describes_no_field_the_output_never_emits() {
        let schema: Value = serde_json::from_str(SCHEMA).expect("schema is JSON");
        let mut described = Vec::new();
        schema_properties(&schema, "$", &mut described);
        let mut emitted = Vec::new();
        emitted_fields(&rendered(EVERY_FIELD), "$", &mut emitted);
        let never_emitted: Vec<&String> = described
            .iter()
            .filter(|path| !emitted.contains(path))
            .collect();
        assert!(never_emitted.is_empty(), "{never_emitted:#?}");
    }

    #[test]
    fn output_carries_the_schema_version() {
        assert_eq!(
            rendered("SERVICE a\nRUN x\n")["schema_version"],
            SCHEMA_VERSION
        );
    }
}
