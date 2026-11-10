# Orchfile Specification

**Version**: 1.0.0-rc

## Overview

Orchfile is a declarative specification for service orchestration. It describes *what* services exist, their relationships, and resource requirements. Platform-specific tooling implements *how* to run them.

## Design Principles

1. **Platform-agnostic specification** - Orchfile defines intent, not implementation
2. **Explicit over implicit** - Every default is stated in this spec; what a directive leaves to the runtime is stated too
3. **Container XOR Host** - A service is either containerized or host-native, never both
4. **Dependency clarity** - Distinguish between ordering and requirements
5. **Resource-aware** - A service declares its resource limits

## File Format

- Plain text, line-oriented
- UTF-8 encoding. An optional byte-order mark at the start of the file is ignored.
- Lines end with LF (`\n`) or CRLF (`\r\n`). The final line ending is optional.
- No control characters other than tab.
- A line whose first non-blank character is `#` is a comment. A `#` anywhere else is part of the value: `ENV COLOR=#ff0000 # red` sets `COLOR` to `#ff0000 # red`.
- Blank lines ignored
- Directives start at the beginning of the line (no indentation)
- Trailing whitespace on a line is ignored and is never part of a value
- Case-sensitive directives (uppercase)
- No continuation lines (each directive is one line)

## Grammar (EBNF)

The formal grammar is defined in W3C EBNF in [`grammar.ebnf`](grammar.ebnf). A typeset version with railroad diagrams is available in [`docs/grammar.pdf`](docs/grammar.pdf).

**Notes**: The grammar describes each value *after* variable expansion. `var_ref` (`${name}`) may appear anywhere in the value of a directive inside a SERVICE block (not in `SERVICE`, `ARG`, `ORCH_VERSION` or `CLEAR` lines), and `var_escape` (`$${`) stands for a literal `${`. With `ARG port=8080`, `PUBLISH ${port}:80` is checked as `PUBLISH 8080:80`. Expansion rules are under [ARG](#arg).

## Constraints

The grammar fixes the shape of each file. A service block sets at most one of `FROM` or `RUN`, and container-only or host-only directives appear only in a block of the matching mode or in an overlay block (one that sets neither). The constraints below need information a single file's syntax does not carry: other files, declared names, the dependency graph, or whether a value repeats.

### C1: Every Service Has a Mode

After all files are merged, every service MUST have a mode: some block for it sets `FROM` (container mode) or `RUN` (host mode).

```
# base.orch
SERVICE web
FROM nginx

# overlay.orch
SERVICE web
ENV MODE=staging
```

Merging `base.orch` and `overlay.orch` is valid: `web` is a container service. Parsing `overlay.orch` alone fails C1.

### C2: Container-Only Directives

`ENTRYPOINT`, `CMD`, `PUBLISH` and `VOLUME` in an overlay block require the merged service to be in container mode.

### C3: Host-Only Directives

`USER`, `STOP` and `RELOAD` in an overlay block require the merged service to be in host mode.

### C4: Dependency Acyclicity

Service dependencies defined by `REQUIRES` and `AFTER` MUST form a directed acyclic graph (DAG). Cycles are a parse error.

### C5: Declared Variables

Every `${name}` MUST name an ARG declared in one of the merged files. An undeclared name is a parse error. See [ARG](#arg).

### C6: No Repeats Within a File

Within one file, a service sets each scalar directive (`CMD`, `MEMORY`, `WORKDIR`, ...) at most once, each `ENV` key at most once, and each `VOLUME` destination at most once. A repeat is a parse error. Across files, overlays replace values.

---

## Directives Reference

### Global Directives

#### ORCH_VERSION

Asserts the Orchfile spec version the file targets. Optional. If present, it is the first statement of the file: only blank lines and comments may precede it. A version that the parser does not support is a parse error.

```
ORCH_VERSION 1.0.0-rc
```

When omitted, the file is assumed to target the parser's current spec version. In multi-file composition, any file may declare it.

This is the version of the **Orchfile specification** (currently `1.0.0-rc`), which evolves independently of the `orch` library/CLI release version.

#### ARG

Defines a variable with default value, overridable at parse time. An ARG may appear before the first service block or inside one; either way it applies to all merged files.

```
ARG postgres_port=5433
ARG memory=4G
```

**Override mechanisms** (in priority order):
1. CLI: `--arg name=value`
2. Environment: `ORCH_ARG_name=value`
3. Orchfile default

An override applies only to an ARG that one of the files declares. An override for an undeclared name is ignored.

**Variable expansion**: `${name}` in any directive value is replaced with that ARG's value.

- `name` MUST be an ARG declared in one of the merged files (C5).
- `$${` produces a literal `${`. `RUN sh -c 'echo $${HOME}'` runs `sh -c 'echo ${HOME}'`. Any other `$` is ordinary text, so `$HOME` and `$$` need no escape.
- An unclosed `${` or an empty `${}` is a parse error.
- ARG defaults are literal. They may use `$${` but cannot reference other variables. Override values are used exactly as given.
- ARG values from all files and overrides are combined first. Every value is then expanded before services are merged, so overlays merge on expanded values: with `ARG p=80`, a base `PUBLISH 8080:${p}` and an overlay `PUBLISH 9090:80` refer to the same container port. The parser's output contains no unexpanded references; any `${` in it came from a `$${` escape.
- The expanded value is what must match the directive's grammar. `ARG web=8080:80` makes `PUBLISH ${web}` valid, and `ARG deps=db cache` makes `REQUIRES ${deps}` name two services.

**Values supplied by a runtime**: A runtime that needs to provide a value, such as a data directory, passes it as an override. The Orchfile declares the ARG with a default, so it still parses on its own.

```
ARG data_dir=~/.orch/data

SERVICE postgres
FROM postgres:15
VOLUME ${data_dir}/postgres:/var/lib/postgresql/data
```

With `--arg data_dir=/srv/data`, the volume source becomes `/srv/data/postgres`.

---

### Service Declaration

#### SERVICE

Begins a new service block. All subsequent directives until the next `SERVICE` apply to this service. Every directive other than `ORCH_VERSION` and `ARG` belongs to a service block.

```
SERVICE postgres
SERVICE my-app
SERVICE socat-proxy
```

**Naming rules**:
- Lowercase alphanumeric and hyphens only
- Must start with letter
- Maximum 63 characters

**Repeats**: Within one file, a service sets each scalar directive, each `ENV` key and each `VOLUME` destination at most once (C6).

---

### Execution Mode (Mutually Exclusive)

#### FROM

Declares a container-based service using the specified image.

```
FROM postgres:15
FROM docker.io/library/nginx:alpine
FROM public.ecr.aws/localstack/localstack:4.2
```

**Image resolution**: Platform tooling handles registry authentication and pulling.

#### RUN

Declares a host-based service with the specified command.

```
RUN python manage.py runserver 0.0.0.0:9090
RUN uvicorn --host 0.0.0.0 --port 8000 app:main
RUN /usr/bin/redis-server /etc/redis.conf
```

---

### Container-Mode Directives

#### ENTRYPOINT

Override the container's entrypoint.

```
ENTRYPOINT /usr/sbin/nginx
```

#### CMD

Override the container's default command/arguments.

```
CMD postgres -c config_file=/etc/postgresql.conf
CMD -g 'daemon off;'
```

#### PUBLISH

Map host port to container port, optionally bound to a specific host address.

```
PUBLISH 5433:5432
PUBLISH 8080:80
PUBLISH 127.0.0.1:8080:80
```

**Format**: `[address:]host_port:container_port`

**Host address**: Optional. When given, the published port binds only to that address (e.g. `127.0.0.1`); when omitted, the platform default applies (typically all interfaces). The address is an IPv4 address, an IPv6 address written without brackets (`PUBLISH ::1:8080:80`), or a hostname. A hostname is not resolved during parsing; it appears in the output as written.

**Multiple allowed**: Specify multiple PUBLISH directives for multiple port mappings. A container port may be published more than once, for example on both `127.0.0.1:8080:80` and `::1:8080:80`.

#### VOLUME

Mount host path or named volume into container.

```
VOLUME /host/path:/container/path
VOLUME my-named-volume:/var/lib/data
VOLUME ~/data/postgres:/var/lib/postgresql/data
VOLUME ~/config:/etc/app:ro
```

**Format**: `source:destination[:mode]`. Source and destination are non-empty and may not contain `:`.

**Mode**: `ro` mounts read-only; `rw` (the default) mounts read-write. Other mount options are not supported.

**Source**: A [path](#paths) on the host, or the name of a volume. A volume name starts with a letter or digit and continues with letters, digits, `_`, `.` and `-`; the volume is created if missing.

**Destination**: A [path](#paths) inside the container. A trailing `/` is ignored, so `/data` and `/data/` are the same destination.

**Multiple allowed**: Specify multiple VOLUME directives. Each destination is mounted at most once per service (C6).

---

### Host-Mode Directives

#### USER

Run the service as specified user.

```
USER postgres
USER www-data
```

**Default**: Current user

#### STOP

Custom command to stop the service gracefully.

```
STOP kill -SIGTERM $(cat /var/run/myapp.pid)
STOP /usr/local/bin/myapp --shutdown
```

**Default**: Send SIGTERM to process group

#### RELOAD

Command to reload service configuration without restart.

```
RELOAD kill -HUP $(cat /run/myapp.pid)
RELOAD nginx -s reload
```

---

### Common Directives

#### WORKDIR

Working directory for service execution.

```
WORKDIR /app
WORKDIR ~/app
```

A [path](#paths). In a container service it is a path inside the container; in a host service it is a path on the host.

#### ENV

Set environment variable.

```
ENV SETTINGS_MODULE=orch.settings.dev
ENV DEBUG=1
ENV DATABASE_URL=postgres://localhost:5433/orch
```

**Format**: `KEY=value`

**Multiple allowed**: Specify multiple ENV directives.

**Variable expansion**: Values can use `${ARG_NAME}` syntax.

#### ENV_FILE

Load environment variables from file.

```
ENV_FILE ~/.env.local
ENV_FILE /etc/myapp/secrets.env
```

A [path](#paths) on the host.

**Format**: One `KEY=value` per line, `#` comments supported.

**Multiple allowed**: Files loaded in order, later values override earlier.

---

### Dependency Directives

#### REQUIRES

Hard dependency - service fails to start if required services are unavailable.

```
REQUIRES postgres redis
REQUIRES refresh-backend-deps
```

**Behavior**: 
- Required services are started first
- If required service fails to become healthy, dependent service does not start
- If a required service is `DISABLED`, the dependent service does not start
- Creates both ordering AND requirement relationship

#### AFTER

Ordering dependency - start after specified services, but don't require them.

```
AFTER localstack
AFTER nginx
```

**Behavior**:
- Specified services are started first if they exist
- If specified service is disabled or fails, dependent still starts
- Creates ordering only, NOT requirement

---

### Health & Readiness

#### HEALTHCHECK

Command or URL to verify service health.

```
HEALTHCHECK pg_isready -h localhost -p 5433
HEALTHCHECK http://localhost:8000/health
HEALTHCHECK redis-cli -h localhost ping
```

**Type detection**:
- Starts with `http://` or `https://` → HTTP check (expect 2xx)
- Otherwise → Execute command (expect exit code 0)

The runtime chooses where the check runs (inside the service's environment or outside it), how often it runs, and how many failures it tolerates.

#### READINESS_TIMEOUT

Maximum time for `HEALTHCHECK` to pass once the service's process has started. Without a `HEALTHCHECK` it has no effect.

```
READINESS_TIMEOUT 120s
READINESS_TIMEOUT 30s
```

**Default**: `90s`

**Format**: Integer with unit suffix (s, m)

---

### Lifecycle Control

#### ONESHOT

Service runs once and exits (not a daemon).

```
ONESHOT true
```

**Default**: `false`

**Behavior**:
- Service runs to completion
- It succeeds when its process exits with code 0
- Dependent services start after it succeeds

#### DISABLED

Service is defined but not started by default.

```
DISABLED true
```

**Default**: `false`

**Behavior**:
- Parsed and validated like any other service
- Not started unless the runtime is asked to start it by name

#### RECREATE

Whether the runtime keeps what it created for the service.

```
RECREATE always
RECREATE never
```

**Default**: `never`

**Values**:
- `always`: Whatever the runtime created for the service (for a container service, its container) is discarded and created again each time the service is set up
- `never`: What already exists is kept

---

### Restart Policy

#### RESTART

Automatic restart behavior on service failure.

```
RESTART no
RESTART always
RESTART on-failure
```

**Default**: `no`

**Values**:
- `no`: Never restart automatically
- `always`: Always restart when process exits
- `on-failure`: Restart only on non-zero exit code

#### RESTART_DELAY

Time to wait before restarting failed service.

```
RESTART_DELAY 5s
RESTART_DELAY 1m
```

**Default**: `1s`

#### START_LIMIT_BURST

Maximum restart attempts within interval before giving up.

```
START_LIMIT_BURST 5
```

**Default**: `5`

#### START_LIMIT_INTERVAL

Time window for counting restart attempts.

```
START_LIMIT_INTERVAL 10s
```

**Default**: `10s`

**Behavior**: If service fails `START_LIMIT_BURST` times within `START_LIMIT_INTERVAL`, stop attempting restarts.

---

### Timeouts

#### TIMEOUT_START

Maximum time for the service's process to start. Becoming healthy afterwards is bounded by `READINESS_TIMEOUT`.

```
TIMEOUT_START 30s
TIMEOUT_START 5m
```

**Default**: `90s`

#### TIMEOUT_STOP

Maximum time to wait for service to stop gracefully before force killing.

```
TIMEOUT_STOP 10s
TIMEOUT_STOP 30s
```

**Default**: `10s`

---

### Resource Limits

Resource limits apply to BOTH container and host services.

#### MEMORY

Maximum memory allocation.

```
MEMORY 4G
MEMORY 512M
```

**Format**: Integer with unit suffix: `K` = 1024 bytes, `M` = 1024 K, `G` = 1024 M

#### CPUS

CPU core allocation.

```
CPUS 2
CPUS 0.5
```

**Format**: Number (can be fractional)

#### CPU_QUOTA

CPU percentage limit (more precise than CPUS for host services).

```
CPU_QUOTA 50%
CPU_QUOTA 200%
```

**Format**: Percentage (>100% allowed for multi-core)

#### LIMIT_NOFILE

Maximum open file descriptors.

```
LIMIT_NOFILE 65536
```

**Default**: System default

#### LIMIT_NPROC

Maximum number of processes/threads.

```
LIMIT_NPROC 4096
```

**Default**: System default

#### TASKS_MAX

Maximum number of tasks (threads + processes).

```
TASKS_MAX 4096
```

#### IO_WEIGHT

IO scheduling weight relative to other services.

```
IO_WEIGHT 100
IO_WEIGHT 500
```

**Format**: 1-10000 (default 100)

---

### Logging

#### STDOUT

Destination for standard output.

```
STDOUT ~/logs/myapp.log
STDOUT /var/log/myapp.log
```

**Default**: Chosen by the runtime.

#### STDERR

Destination for standard error.

```
STDERR ~/logs/myapp.err
STDERR /var/log/myapp.err
```

**Default**: Chosen by the runtime.

---

## File Composition (Overlay Model)

Orchfile supports multi-file composition using an overlay model. A base Orchfile can be overlaid with environment-specific files (staging, CI, personal) that tune resources, ports, env vars, and dependencies without forking the entire file.

### Usage

```
orch resolve base.orch staging.orch personal.orch [--arg ...]
orch validate base.orch staging.orch personal.orch [--arg ...]
```

Files are merged left-to-right. The leftmost file is the base; each subsequent file is an overlay.

### Merge Semantics

#### Scalars: Last Wins

Any scalar directive (`FROM`, `RUN`, `WORKDIR`, `MEMORY`, `CPUS`, etc.) in an overlay replaces the base value. Omitting a scalar in an overlay preserves the base value.

```
# base.orch
SERVICE web
FROM nginx:1.24
MEMORY 1G

# staging.orch — overrides image, preserves MEMORY
SERVICE web
FROM nginx:1.25
```

#### Keyed Lists: Merge by Key

- **ENV**: Merged by variable name (overlay wins on conflict)
- **PUBLISH**: Merged by container port (an overlay's mappings for a container port replace all base mappings for that port)
- **VOLUME**: Merged by destination path (overlay replaces source and mode for same destination)

```
# base.orch
SERVICE web
FROM nginx
ENV MODE=production
PUBLISH 8080:80

# staging.orch — MODE overridden, DEBUG added, port 80 remapped
SERVICE web
ENV MODE=staging
ENV DEBUG=true
PUBLISH 9090:80
```

#### Positional Lists: Append + Dedup

- **REQUIRES**: Appended, duplicates removed
- **AFTER**: Appended, duplicates removed
- **ENV_FILE**: Appended, duplicates removed

```
# base.orch
SERVICE web
FROM nginx
REQUIRES db

# staging.orch — adds cache dependency
SERVICE web
REQUIRES cache
# Result: REQUIRES db cache
```

#### New Services

Services in overlays that don't exist in the base are appended.

#### Mode Switching

Setting `FROM` in an overlay on a service that had `RUN` clears all host-only directives (`USER`, `STOP`, `RELOAD`) and switches to container mode. Setting `RUN` clears all container-only directives (`ENTRYPOINT`, `CMD`, `PUBLISH`, `VOLUME`) and switches to host mode.

### CLEAR Directive

The `CLEAR` directive resets a list-type field before applying the block's values. It applies to the whole block wherever it appears in it. Without `CLEAR`, overlay values merge with (append to) base values.

```
CLEAR ENV
CLEAR ENV_FILE
CLEAR PUBLISH
CLEAR VOLUME
CLEAR REQUIRES
CLEAR AFTER
```

**Valid targets**: Only list-type directives can be cleared. Using `CLEAR` on scalar directives (e.g., `CLEAR FROM`) is a parse error — scalars override by last-wins naturally.

```
# base.orch
SERVICE web
FROM nginx
ENV OLD_KEY=old_value
ENV KEEP=this

# overlay.orch — wipe all env, start fresh
SERVICE web
CLEAR ENV
ENV FRESH=new_value
# Result: only FRESH=new_value (OLD_KEY and KEEP are gone)
```

### ARG Scoping

ARGs are merged left-to-right (last wins). ARG values from every file are combined before any value is expanded, so overlays can redefine ARG defaults that affect the base file's `${var}` references.

```
# base.orch
ARG port=8080
SERVICE web
FROM nginx
PUBLISH ${port}:80

# staging.orch
ARG port=9090
# Result: port=9090, so PUBLISH becomes 9090:80
```

An ARG may be declared only in an overlay and used in the base file.

CLI `--arg` overrides take highest priority, above all file defaults.

### Validation

Constraints C1–C5 are checked on the **merged result**; C6 is checked on each file. An overlay block that sets neither `FROM` nor `RUN` takes its mode from another file.

---

## Paths

`WORKDIR`, `ENV_FILE`, `STDOUT`, `STDERR`, a `VOLUME` destination, and a `VOLUME` source that is not a volume name are paths. A path is absolute (`/var/log/app.log`) or starts with `~` (`~` or `~/logs/app.log`).

`~` is the home directory of the user the service runs as. For a path on the host that belongs to a container service, it is the home directory of the user the runtime runs the service as. Paths appear in the output as written.

---

## Example Orchfile

```
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
```

---

## Output

Resolving a set of Orchfiles merges them, expands every variable, checks every constraint, and yields one result. That result has a canonical form, which is itself an Orchfile:

- `ORCH_VERSION` first, then one block per service, separated by blank lines, in the order the services first appear.
- No `ARG` or `CLEAR` lines, and no variable references. A literal `${` is written `$${`.
- In each block, `FROM` or `RUN` first, then the service's directives in this order: `ENTRYPOINT`, `CMD`, `PUBLISH`, `VOLUME`, `USER`, `STOP`, `RELOAD`, `WORKDIR`, `ENV`, `ENV_FILE`, `REQUIRES`, `AFTER`, `HEALTHCHECK`, `READINESS_TIMEOUT`, `ONESHOT`, `DISABLED`, `RECREATE`, `RESTART`, `RESTART_DELAY`, `START_LIMIT_BURST`, `START_LIMIT_INTERVAL`, `TIMEOUT_START`, `TIMEOUT_STOP`, `MEMORY`, `CPUS`, `CPU_QUOTA`, `LIMIT_NOFILE`, `LIMIT_NPROC`, `TASKS_MAX`, `IO_WEIGHT`, `STDOUT`, `STDERR`.
- `ENV` lines are sorted by key. `REQUIRES` and `AFTER` are each one line. Other repeated directives keep their merged order.
- A directive whose value equals its default is omitted (`ONESHOT false`, `DISABLED false`, `RECREATE never`, `RESTART no`, and `rw` on a `VOLUME`).
- Ports are written without leading zeros, and a `VOLUME` destination without a trailing `/`.

Resolving the canonical form yields the same canonical form. Two sets of Orchfiles are equivalent exactly when their canonical forms are byte-for-byte equal.

```
ORCH_VERSION 1.0.0-rc

SERVICE web
FROM nginx
PUBLISH 9090:80
ENV MODE=staging
```

The [`conformance/`](conformance/) suite holds inputs with their expected canonical forms, and inputs that must be rejected.
