# Orchfile Specification

**Target**: Local, ephemeral, development, and staging environments  
**Not for**: Production deployments at scale

## Overview

Orchfile is a declarative specification for service orchestration. It describes *what* services exist, their relationships, and resource requirements. Platform-specific tooling implements *how* to run them.

Orchfile is inspired by Containerfile's declarative simplicity, not docker-compose's configuration verbosity.

## Design Principles

1. **Platform-agnostic specification** - Orchfile defines intent, not implementation
2. **Explicit over implicit** - No magic defaults that vary by platform
3. **Container XOR Host** - A service is either containerized or host-native, never both
4. **Dependency clarity** - Distinguish between ordering and requirements
5. **Resource-aware** - Development environments need resource optimization too

## File Format

- Plain text, line-oriented
- UTF-8 encoding
- Comments start with `#`
- Blank lines ignored
- Case-sensitive directives (uppercase)
- No continuation lines (each directive is one line)

## Grammar (EBNF)

ISO 14977 Extended Backus-Naur Form. Each line in the file is processed independently.

```ebnf
(* ---- Document structure ---- *)

orchfile       = { line , newline } ;
line           = blank | comment | arg | service_decl | directive ;
blank          = { ws } ;
comment        = { ws } , "#" , { any } ;

(* ---- Top-level ---- *)

arg            = "ARG" , ws , identifier , "=" , value ;
service_decl   = "SERVICE" , ws , service_name ;

(* ---- Directives ---- *)

directive      = mode_directive
               | container_directive
               | host_directive
               | common_directive ;

(* Execution mode — mutually exclusive per service *)
mode_directive       = "FROM" , ws , image_ref
                     | "RUN" , ws , command_string ;

(* Container-only *)
container_directive  = "ENTRYPOINT" , ws , command_string
                     | "CMD" , ws , command_string
                     | "PUBLISH" , ws , port , ":" , port
                     | "VOLUME" , ws , volume_source , ":" , path ;

(* Host-only *)
host_directive       = "USER" , ws , identifier
                     | "STOP" , ws , command_string
                     | "RELOAD" , ws , command_string ;

(* Common *)
common_directive     = "WORKDIR" , ws , path
                     | "ENV" , ws , env_key , "=" , value
                     | "ENV_FILE" , ws , path
                     | "REQUIRES" , ws , service_name , { ws , service_name }
                     | "AFTER" , ws , service_name , { ws , service_name }
                     | "HEALTHCHECK" , ws , ( url | command_string )
                     | "READINESS_TIMEOUT" , ws , duration
                     | "ONESHOT" , ws , bool
                     | "DISABLED" , ws , bool
                     | "RECREATE" , ws , recreate_policy
                     | "RESTART" , ws , restart_policy
                     | "RESTART_DELAY" , ws , duration
                     | "TIMEOUT_START" , ws , duration
                     | "TIMEOUT_STOP" , ws , duration
                     | "START_LIMIT_BURST" , ws , integer
                     | "START_LIMIT_INTERVAL" , ws , duration
                     | "MEMORY" , ws , memory_size
                     | "CPUS" , ws , number
                     | "CPU_QUOTA" , ws , percentage
                     | "LIMIT_NOFILE" , ws , integer
                     | "LIMIT_NPROC" , ws , integer
                     | "TASKS_MAX" , ws , integer
                     | "IO_WEIGHT" , ws , integer         (* 10-1000 *)
                     | "STDOUT" , ws , path
                     | "STDERR" , ws , path ;

(* ---- Terminals ---- *)

service_name   = letter , { letter | digit | "-" } ;       (* max 63 chars *)
identifier     = letter , { letter | digit | "_" } ;
env_key        = ( letter | "_" ) , { letter | digit | "_" } ;

image_ref      = { any - newline } ;                        (* registry/image:tag *)
command_string = { any - newline } ;                        (* free-form command *)
path           = { any - newline } ;                        (* may contain var_ref *)
value          = { any - newline } ;                        (* may contain var_ref *)
url            = ( "http://" | "https://" ) , { any - newline } ;

var_ref        = "${" , identifier , "}" ;

bool           = "true" | "false" ;
restart_policy = "no" | "always" | "on-failure" ;
recreate_policy = "always" | "never" ;

duration       = integer , ( "s" | "m" ) ;
memory_size    = integer , ( "K" | "M" | "G" ) ;
percentage     = integer , "%" ;
number         = integer , [ "." , digit , { digit } ] ;
integer        = digit , { digit } ;
port           = digit , { digit } ;                        (* 1-65535 *)

letter         = "a" | "b" | ... | "z" ;                   (* lowercase only *)
digit          = "0" | "1" | ... | "9" ;
ws             = " " | "\t" , { " " | "\t" } ;
newline        = "\n" ;
any            = ? any UTF-8 character ? ;
```

**Notes**: `var_ref` (`${name}`) may appear within any `value`, `path`, `image_ref`, or `command_string`. Expansion is performed at parse time using resolved ARG values. Unresolved references to built-in variables (e.g. `${ORCH_PROJECT}`) are preserved for runtime resolution.

## Constraints

### C1: FROM XOR RUN

A service MUST specify exactly one of `FROM` (container mode) or `RUN` (host mode). Specifying both is a parse error. Specifying neither is a parse error.

```
SERVICE valid-container
FROM postgres:15
CMD -c config_file=/etc/postgres.conf

SERVICE valid-host
RUN python manage.py runserver

SERVICE invalid
FROM postgres:15
RUN postgres           # ERROR: Cannot specify both FROM and RUN
```

### C2: Container-Only Directives

The following directives are only valid in container mode (with `FROM`):
- `ENTRYPOINT`
- `CMD`
- `PUBLISH`
- `VOLUME`

Using these with `RUN` is a parse error.

### C3: Host-Only Directives

The following directives are only valid in host mode (with `RUN`):
- `USER`
- `STOP`
- `RELOAD`

Using these with `FROM` is a parse error.

### C4: Dependency Acyclicity

Service dependencies defined by `REQUIRES` and `AFTER` MUST form a directed acyclic graph (DAG). Cycles are a parse error.

---

## Directives Reference

### Global Directives

#### ARG

Defines a variable with default value, overridable at parse time.

```
ARG postgres_port=5433
ARG memory=4G
```

**Override mechanisms** (in priority order):
1. CLI: `--arg name=value`
2. Environment: `ORCH_ARG_name=value`
3. Orchfile default

**Variable expansion**: Use `${name}` syntax in any directive value.

---

### Service Declaration

#### SERVICE

Begins a new service block. All subsequent directives until the next `SERVICE` apply to this service.

```
SERVICE postgres
SERVICE my-app
SERVICE socat-proxy
```

**Naming rules**:
- Lowercase alphanumeric and hyphens only
- Must start with letter
- Maximum 63 characters

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

**Command execution**: Run via platform's process supervisor (launchd, systemd).

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

Map host port to container port.

```
PUBLISH 5433:5432
PUBLISH 8080:80
```

**Format**: `host_port:container_port`

**Multiple allowed**: Specify multiple PUBLISH directives for multiple port mappings.

#### VOLUME

Mount host path or named volume into container.

```
VOLUME /host/path:/container/path
VOLUME my-named-volume:/var/lib/data
VOLUME ${ORCH_DATA}/postgres:/var/lib/postgresql/data
```

**Format**: `source:destination`

**Named volumes**: If source doesn't start with `/`, it's treated as a named volume (created if missing).

**Multiple allowed**: Specify multiple VOLUME directives.

---

### Host-Mode Directives

#### USER

Run the service as specified user.

```
USER postgres
USER www-data
```

**Default**: Current user

**Platform mapping**:
- launchd: `UserName` key in plist
- systemd: `User=` directive

#### STOP

Custom command to stop the service gracefully.

```
STOP kill -SIGTERM $(cat /var/run/myapp.pid)
STOP /usr/local/bin/myapp --shutdown
```

**Default**: Send SIGTERM to process group

**Platform mapping**:
- launchd: Not directly supported (uses process termination)
- systemd: `ExecStop=`

#### RELOAD

Command to reload service configuration without restart.

```
RELOAD kill -SIGHUP $MAINPID
RELOAD nginx -s reload
```

**Platform mapping**:
- launchd: Not directly supported
- systemd: `ExecReload=`

---

### Common Directives

#### WORKDIR

Working directory for service execution.

```
WORKDIR backend/canary
WORKDIR /app
```

**Relative paths**: Resolved against `${ORCH_PROJECT}`

**Container mode**: Sets container working directory

**Host mode**: Sets process working directory

#### ENV

Set environment variable.

```
ENV DJANGO_SETTINGS_MODULE=canary.settings.dev
ENV DEBUG=1
ENV DATABASE_URL=postgres://localhost:5433/canary
```

**Format**: `KEY=value`

**Multiple allowed**: Specify multiple ENV directives.

**Variable expansion**: Values can use `${ARG_NAME}` syntax.

#### ENV_FILE

Load environment variables from file.

```
ENV_FILE ${ORCH_PROJECT}/.env.local
ENV_FILE ${ORCH_DATA}/secrets.env
```

**Format**: One `KEY=value` per line, `#` comments supported.

**Multiple allowed**: Files loaded in order, later values override earlier.

**Platform mapping**:
- launchd: Parsed and inlined into plist
- systemd: `EnvironmentFile=`

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

**Migration from DEPENDS**: The legacy `DEPENDS` directive was ambiguous and has been removed. Convert as follows:
- `DEPENDS foo` where foo MUST succeed → `REQUIRES foo`
- `DEPENDS foo` where foo is optional → `AFTER foo`

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

#### READINESS_TIMEOUT

How long to wait for HEALTHCHECK to pass during startup.

```
READINESS_TIMEOUT 120s
READINESS_TIMEOUT 30s
```

**Default**: `90s`

**Format**: Duration with unit suffix (s, m)

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
- Success determined by exit code
- Creates ready marker file on success
- Dependent services wait for completion

#### DISABLED

Service is defined but not started by default.

```
DISABLED true
```

**Default**: `false`

**Behavior**:
- Parsed and validated
- Skipped during `orch up` unless explicitly named
- Can be started explicitly: `orch up disabled-service`

#### RECREATE

Container recreation policy.

```
RECREATE always
RECREATE never
```

**Default**: `never`

**Values**:
- `always`: Destroy and recreate container on every `orch create`
- `never`: Keep existing container if present

**Note**: Only applies to container-mode services.

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

**Platform mapping**:
- launchd: `KeepAlive` / `SuccessfulExit`
- systemd: `Restart=`

#### RESTART_DELAY

Time to wait before restarting failed service.

```
RESTART_DELAY 5s
RESTART_DELAY 1m
```

**Default**: `1s`

**Platform mapping**:
- launchd: `ThrottleInterval`
- systemd: `RestartSec=`

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

Maximum time to wait for service to start/become ready.

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

**Platform mapping**:
- launchd: `ExitTimeOut`
- systemd: `TimeoutStopSec=`

---

### Resource Limits

Resource limits apply to BOTH container and host services.

#### MEMORY

Maximum memory allocation.

```
MEMORY 4G
MEMORY 512M
```

**Format**: Number with unit suffix (K, M, G)

**Platform mapping**:
- Container: `--memory` flag
- launchd host: Not enforced (advisory)
- systemd host: `MemoryMax=`

#### CPUS

CPU core allocation.

```
CPUS 2
CPUS 0.5
```

**Format**: Number (can be fractional)

**Platform mapping**:
- Container: `--cpus` flag
- launchd host: Not enforced (advisory)
- systemd host: `CPUQuota=` (converted to percentage)

#### CPU_QUOTA

CPU percentage limit (more precise than CPUS for host services).

```
CPU_QUOTA 50%
CPU_QUOTA 200%
```

**Format**: Percentage (>100% allowed for multi-core)

**Platform mapping**:
- launchd: Not enforced
- systemd: `CPUQuota=`

#### LIMIT_NOFILE

Maximum open file descriptors.

```
LIMIT_NOFILE 65536
```

**Default**: System default

**Platform mapping**:
- launchd: `SoftResourceLimits` / `HardResourceLimits` → `NumberOfFiles`
- systemd: `LimitNOFILE=`

#### LIMIT_NPROC

Maximum number of processes/threads.

```
LIMIT_NPROC 4096
```

**Default**: System default

**Platform mapping**:
- launchd: `SoftResourceLimits` / `HardResourceLimits` → `NumberOfProcesses`
- systemd: `LimitNPROC=`

#### TASKS_MAX

Maximum number of tasks (threads + processes).

```
TASKS_MAX 4096
```

**Platform mapping**:
- launchd: Not enforced
- systemd: `TasksMax=`

#### IO_WEIGHT

IO scheduling weight relative to other services.

```
IO_WEIGHT 100
IO_WEIGHT 500
```

**Format**: 10-1000 (default 100)

**Platform mapping**:
- launchd: Not enforced
- systemd: `IOWeight=`

---

### Logging

#### STDOUT

Destination for standard output.

```
STDOUT ${ORCH_STATE_DIR}/logs/myapp.log
STDOUT /var/log/myapp.log
```

**Default**: Platform-specific
- launchd: `${ORCH_STATE_DIR}/logs/${SERVICE_NAME}.log`
- systemd: journal

#### STDERR

Destination for standard error.

```
STDERR ${ORCH_STATE_DIR}/logs/myapp.err
STDERR /var/log/myapp.err
```

**Default**: Platform-specific
- launchd: `${ORCH_STATE_DIR}/logs/${SERVICE_NAME}.err`
- systemd: journal

---

## Built-in Variables

Available for expansion in directive values:

| Variable | Description |
|----------|-------------|
| `${ORCH_PROJECT}` | Project root directory |
| `${ORCH_DATA}` | Data directory for persistent storage |
| `${ORCH_STATE_DIR}` | Orchestrator state directory |
| `${ORCH_CONTAINERS_DIR}` | Containers scripts directory |
| `${SERVICE_NAME}` | Current service name (in STDOUT/STDERR) |
| `${PORT_OFFSET}` | Port offset for parallel environments |
| `${CONTAINER_PREFIX}` | Container name prefix |

---

## Example Orchfile

```
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
```

---

## Platform Implementation Notes

### macOS (launchd + Apple Containers)

- Container services: Managed via `/usr/local/bin/container`
- Host services: Managed via launchd plists in `~/Library/LaunchAgents`
- Resource limits: Limited enforcement for host services (MEMORY/CPUS advisory only)
- Logging: File-based in `${ORCH_STATE_DIR}/logs/`

### Linux (systemd + podman)

- Container services: Managed via podman with systemd integration
- Host services: Managed via systemd user units in `~/.config/systemd/user/`
- Resource limits: Full cgroup v2 enforcement
- Logging: journald by default, file override supported

---


