# orch

A parser for Orchfiles -- declarative, platform-agnostic service orchestration specifications.

`orch` reads an Orchfile and outputs structured JSON. It validates all constraints, expands variables, and detects dependency cycles. The JSON output is designed for consumption by platform-specific generators (launchd, systemd, etc.) or shell scripts.

## Install

```sh
cargo build --release
cp target/release/orch /usr/local/bin/
```

## Usage

```
orch parse <file> [<file> ...] [--arg name=value ...]
orch validate <file> [<file> ...] [--arg name=value ...]
```

### Commands

| Command    | Output   | Exit Code                          |
|------------|----------|------------------------------------|
| `parse`    | JSON to stdout | 0 on success, 1 on errors     |
| `validate` | "valid" to stderr | 0 if valid, 1 if errors    |

Exit code 2 indicates usage errors (bad arguments, missing file).

### Multi-file Composition

Multiple files are merged left-to-right using a systemd drop-in overlay model:

```sh
orch parse base.orch staging.orch personal.orch
```

Merge rules:
- **Scalars** (FROM, MEMORY, CPUS, ...): last wins
- **Keyed lists** (ENV, PUBLISH, VOLUME): merge by key, overlay wins on conflict
- **Positional lists** (REQUIRES, AFTER, ENV_FILE): append + dedup
- **CLEAR directive**: resets list fields before applying overlay values

See [SPEC.md](SPEC.md) for full composition semantics.

### ARG Overrides

Override Orchfile `ARG` defaults via CLI flags or environment variables:

```sh
# CLI flag (highest priority)
orch parse Orchfile --arg postgres_port=9999

# Multiple overrides
orch parse Orchfile --arg postgres_port=9999 --arg memory=8G

# Environment variable
ORCH_ARG_postgres_port=9999 orch parse Orchfile
```

**Priority order:** `--arg` flag > `ORCH_ARG_*` env var > Orchfile default.

## Example Orchfile

```
ORCH_VERSION 1.0.0-rc

ARG postgres_port=5433
ARG django_port=9090

SERVICE postgres
FROM pgvector/pgvector:pg15
MEMORY 4G
CPUS 2
PUBLISH ${postgres_port}:5432
VOLUME postgres-data:/var/lib/postgresql/data
ENV POSTGRES_USER=postgres
HEALTHCHECK pg_isready -h localhost -p ${postgres_port}
RESTART on-failure

SERVICE django
RUN python manage.py runserver 0.0.0.0:${django_port}
WORKDIR backend/canary
REQUIRES postgres
HEALTHCHECK http://localhost:${django_port}/health
```

## JSON Output

```sh
orch parse Orchfile
```

```json
{
  "version": "1.0.0-rc",
  "args": {
    "postgres_port": "5433",
    "django_port": "9090"
  },
  "services": [
    {
      "name": "postgres",
      "mode": "container",
      "image": "pgvector/pgvector:pg15",
      "publish": [{ "host": 5433, "container": 5432 }],
      "volumes": [{ "source": "postgres-data", "destination": "/var/lib/postgresql/data", "is_named": true }],
      "env": { "POSTGRES_USER": "postgres" },
      "healthcheck": "pg_isready -h localhost -p 5433",
      "oneshot": false,
      "disabled": false,
      "recreate": "never",
      "restart": { "policy": "on_failure" },
      "timeouts": {},
      "resources": { "memory": "4G", "cpus": 2.0 },
      "logging": {}
    },
    {
      "name": "django",
      "mode": "host",
      "run_command": "python manage.py runserver 0.0.0.0:9090",
      "workdir": "backend/canary",
      "requires": ["postgres"],
      "healthcheck": "http://localhost:9090/health",
      "oneshot": false,
      "disabled": false,
      "recreate": "never",
      "restart": { "policy": "no" },
      "timeouts": {},
      "resources": {},
      "logging": {}
    }
  ]
}
```

## Bash Script Integration

The JSON output is designed for `jq`-based consumption in shell scripts. Here are practical examples:

### List all service names

```sh
orch parse Orchfile | jq -r '.services[].name'
```

### Get port mappings for a service

```sh
orch parse Orchfile | jq '.services[] | select(.name == "postgres") | .publish'
```

### Generate a container run command

```sh
#!/bin/bash
set -euo pipefail

ORCH_JSON=$(orch parse Orchfile)

# Iterate over container services
echo "$ORCH_JSON" | jq -c '.services[] | select(.mode == "container")' | while read -r svc; do
    name=$(echo "$svc" | jq -r '.name')
    image=$(echo "$svc" | jq -r '.image')

    # Build port flags
    ports=$(echo "$svc" | jq -r '.publish[]? | "-p \(.host):\(.container)"' | tr '\n' ' ')

    # Build volume flags
    vols=$(echo "$svc" | jq -r '.volumes[]? | "-v \(.source):\(.destination)"' | tr '\n' ' ')

    # Build env flags
    envs=$(echo "$svc" | jq -r '.env // {} | to_entries[] | "-e \(.key)=\(.value)"' | tr '\n' ' ')

    # Build memory flag
    mem=$(echo "$svc" | jq -r '.resources.memory // empty' | sed 's/^/--memory /')

    echo "docker run -d --name $name $ports $vols $envs $mem $image"
done
```

### Check if an Orchfile is valid in CI

```sh
if orch validate Orchfile 2>/dev/null; then
    echo "Orchfile is valid"
else
    echo "Orchfile has errors:" >&2
    orch validate Orchfile
    exit 1
fi
```

### Extract all required dependencies for a service

```sh
orch parse Orchfile | jq -r '.services[] | select(.name == "django") | .requires[]'
```

### Build a dependency-ordered start list

```sh
#!/bin/bash
# Start services in dependency order using the parsed requires/after fields
set -euo pipefail

ORCH_JSON=$(orch parse Orchfile)
started=()

start_service() {
    local name=$1

    # Skip if already started
    for s in "${started[@]:-}"; do
        [[ "$s" == "$name" ]] && return
    done

    # Start required dependencies first
    for dep in $(echo "$ORCH_JSON" | jq -r --arg n "$name" '.services[] | select(.name == $n) | .requires[]? // empty'); do
        start_service "$dep"
    done

    echo "Starting: $name"
    started+=("$name")
}

# Start all non-disabled services
for svc in $(echo "$ORCH_JSON" | jq -r '.services[] | select(.disabled == false) | .name'); do
    start_service "$svc"
done
```

### Filter services by mode

```sh
# Container services only
orch parse Orchfile | jq '[.services[] | select(.mode == "container")]'

# Host services only
orch parse Orchfile | jq '[.services[] | select(.mode == "host")]'
```

### Get the healthcheck for a specific service

```sh
orch parse Orchfile | jq -r '.services[] | select(.name == "postgres") | .healthcheck // "none"'
```

## Error Messages

Errors include line numbers (for parse errors) or service names (for validation errors):

```
$ orch validate bad.orch
error: parse error: line 5: cannot specify both FROM and RUN
error: validation error: service 'app': ENTRYPOINT is only valid with FROM (container mode) (C2)
error: validation error: service 'a': dependency cycle detected: a -> b -> a
```

## Grammar

The formal EBNF grammar is in [`grammar.ebnf`](grammar.ebnf). A typeset version with railroad diagrams is in [`docs/grammar.pdf`](docs/grammar.pdf).

## Specification

See [SPEC.md](SPEC.md) for the complete Orchfile specification.

## Constraints Enforced

| Constraint | Description |
|------------|-------------|
| C1 | Each service must have exactly one of `FROM` or `RUN` |
| C2 | `ENTRYPOINT`, `CMD`, `PUBLISH`, `VOLUME` only with `FROM` |
| C3 | `USER`, `STOP`, `RELOAD` only with `RUN` |
| C4 | `REQUIRES` + `AFTER` must form a DAG (no cycles) |

Additional validations:
- Service names: lowercase alphanumeric + hyphens, starts with letter, max 63 chars
- No duplicate service names
- `REQUIRES` must reference defined services
- `AFTER` is soft -- undefined targets are allowed (per spec)
- Numeric fields validated (`CPUS`, `LIMIT_NOFILE`, `IO_WEIGHT` range 10-1000, etc.)

## Tests

```sh
cargo test
```

140 tests covering all directives, all constraints (C1-C4), variable expansion, multi-file composition, merge semantics, CLEAR directive, error cases, edge cases, and the full spec example.

## License

Apache 2.0
