# Orchfile Specification

**Version**: 1.0.0-rc  
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

The formal grammar is defined in W3C EBNF in [`grammar.ebnf`](grammar.ebnf). A typeset version with railroad diagrams is available in [`docs/grammar.pdf`](docs/grammar.pdf).

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

#### ORCH_VERSION

Asserts the Orchfile spec version the file targets. Optional, but if present it must be the first directive and appear before any `SERVICE`. A version that the parser does not support is a parse error.

```
ORCH_VERSION 1.0.0-rc
```

When omitted, the file is assumed to target the parser's current spec version. In multi-file composition, any file may declare it.

This is the version of the **Orchfile specification** (currently `1.0.0-rc`), which evolves independently of the `orch` library/CLI release version.

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

Map host port to container port, optionally bound to a specific host address.

```
PUBLISH 5433:5432
PUBLISH 8080:80
PUBLISH 127.0.0.1:8080:80
```

**Format**: `[address:]host_port:container_port`

**Host address**: Optional. When given, the published port binds only to that address (e.g. `127.0.0.1`); when omitted, the platform default applies (typically all interfaces). IPv4, IPv6, and hostnames are accepted.

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

**Semantics (RFC 0001)**: `HEALTHCHECK` is defined as exactly a `READINESS` probe.
It gates dependents and traffic routing but never restarts the service, which is
parity with the base behavior where `RESTART` fires only on process exit and a
hung process is never replaced. For restart-on-unhealthy use `LIVENESS`
explicitly; for hang/deadlock detection use `WATCHDOG`. See the Lifecycle &
Health extensions below.

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

## RFC 0001 Extensions

The directives below extend the **node unit spec** along three axes: time
(lifecycle, health, states, conditions, change), space (node-local requirements,
templated instances), and trust (sandbox, secrets, identity). Normative syntax is
in [`grammar.ebnf`](grammar.ebnf).

Out of scope, by design: cross-node placement, affinity, and redundancy
(`PLACEMENT`/`AFFINITY`/`ANTI_AFFINITY`/`REDUNDANCY`) are a separate
fleet-composition layer; and the assurance/compliance *vocabulary*
(`sil`/`asil`/`dal`/`pci-dss`/...) lives in external profile documents, not in
this grammar. The core interprets exactly one assurance key, the boolean `vital`.

### Lifecycle & Health

Splits the single `HEALTHCHECK` into distinct signals and adds a push channel,
a watchdog, and a managed lifecycle.

- `STARTUP <probe>`: gate that must pass before liveness/readiness are evaluated (slow boots).
- `LIVENESS <probe>`: if it fails, the service is **restarted**.
- `READINESS <probe>`: if it fails, dependents wait and traffic is withheld, but the service is **not** restarted.
- `READY notify | poll`: push readiness over the orchd notify socket (`notify`) instead of being polled.
- `WATCHDOG <duration>`: the service must emit a keep-alive within the window; a miss is treated as a hang and the service is restarted (catches deadlock/livelock).
- `LIFECYCLE managed | simple`: `managed` exposes configure/activate/deactivate transitions (claim and configure hardware before acting); `simple` (default) is start/stop. On a managed service, a readiness failure deactivates (Active to Inactive), it does not restart; readiness loss is not a failure and never triggers `ON_FAILURE`.

```
STARTUP exec test -S /run/opcua.sock
LIVENESS http://localhost:9100/livez
READINESS http://localhost:9100/readyz
READY notify
WATCHDOG 20s
LIFECYCLE managed
```

### Machine States

File-global state set plus per-service membership. The active state determines
which services run (the Adaptive AUTOSAR function-group model, generalized).

- `MACHINE_STATES <name>...` (file-global): declares the machine's operational states.
- `DEFAULT_STATE <name>` (file-global): state entered at boot.
- `STATE <name>...`: the states in which a service is active (omitted = active in all states).
- `GROUP <name>`: logical membership for bulk operations.
- `SLICE <name>`: hierarchical resource-accounting group (cgroup subtree).

```
MACHINE_STATES running maintenance update
DEFAULT_STATE running

SERVICE collector
STATE running
SLICE data
```

Flat states only; concurrent function groups are a deferred non-goal. A future
multi-group model must treat today's `MACHINE_STATES`/`STATE` as the single
default group, so flat files remain valid.

### Node-local Requirements

Admission checks on every backend. Unmet requirements fail admission; pair with a
`CONDITION` to make a requirement optional (then it skips instead of failing).

- `ARCH <arch>`: required CPU architecture (e.g. `arm64`).
- `DEVICE <path>`: required device node.
- `REQUIRES_CAP <cap>[,<cap>...]`: required node capability (`gpu`, `npu`, `hugepages`, `sriov`, ...).

```
ARCH arm64
DEVICE /dev/ttyS1
REQUIRES_CAP hugepages,sriov
```

### Conditions, Windows & Failure

`CONDITION`, `ASSERT`, and `WINDOW` take a **predicate** (`provider[:arg]`)
resolved by an orchd provider; the provider set is a registry, not grammar.
Standard providers include `network-online`, `network-trusted`, `time-synced`,
`path-exists:<path>`, `power-state:<state>`, `arg:<name>`, `contact-pass`,
`tariff:<name>`. Custom providers are reverse-DNS namespaced (`com.vendor.x`).

- `CONDITION <predicate>`: start only if it holds, otherwise **skip** (not an error).
- `ASSERT <predicate>`: like `CONDITION`, but unmet is a hard **failure**.
- `WINDOW <predicate>`: active only while the (recurring) window is open.
- `REQUIRES_HEALTHY <service>...`: wait until a dependency is *ready/healthy*, not merely started.
- `ON_FAILURE <action>`: escalate on failure: `restart`, `state:<name>`, or `unit:<service>`.

```
CONDITION network-online
CONDITION path-exists:/dev/gps
ASSERT path-exists:/dev/irrig-interlock
WINDOW tariff:offpeak
REQUIRES_HEALTHY postgres
ON_FAILURE state:safe-mode
```

### Security, Identity & Trust

Least-privilege sandboxing, secret references (never values), attested identity,
and audit. `SECRET` records a reference URI only; the secret value is never read,
resolved, or emitted in `orch parse` output. This is the secure replacement for
placing secrets in `ENV`, where the value would appear in plaintext.

- `CAPABILITY drop:<set> add:<set>`: Linux capability bounding set.
- `READONLY_ROOT <bool>`, `NO_NEW_PRIVILEGES <bool>`, `PRIVATE_TMP <bool>`: sandbox toggles.
- `SECCOMP <profile>`: syscall filter (`default`, `strict`, or a named profile).
- `EPHEMERAL <bool>`: state lives only in volatile memory; nothing survives power loss.
- `ON_TAMPER zeroize | unit:<service>`: action on a tamper/capture signal.
- `SECRET <NAME> from <reference>`: bind a secret by reference (`vault://`, `tpm://`, `file://`, or a reverse-DNS-namespaced scheme). Resolution happens at runtime in platform tooling. Merged by name; clearable via `CLEAR SECRET`.
- `IDENTITY <spiffe-id> [scope=service|session]`: attested workload identity (SPIFFE SVID); `scope=session` mints per-transaction.
- `AUDIT lifecycle | access`: mark events for the tamper-evident audit log.

```
CAPABILITY drop:ALL add:NET_BIND_SERVICE
READONLY_ROOT true
NO_NEW_PRIVILEGES true
SECCOMP strict
SECRET DB_PASSWORD from vault://canary/db
IDENTITY spiffe://canary/web scope=session
AUDIT lifecycle
```

### Change

Health-gated, reversible change. On-node only; cross-node/fleet rollout is out of
scope (fleet layer).

- `UPDATE strategy=<ab|inplace> [rollback=on-health-fail] [signature=required]`: how a revision is applied; A/B with automatic rollback if the new revision fails its readiness gate.
- `ROLLOUT strategy=<rolling|blue-green|canary> [max-unavailable=<n>] [gate=readiness]`: how an on-node rollout proceeds across a template's instances (or a single service's blue-green).

```
UPDATE strategy=ab rollback=on-health-fail signature=required
ROLLOUT strategy=canary gate=readiness
```

### Observability

- `METRICS <url> [format=prometheus]`: scrapeable metrics endpoint.
- `TRACES <otlp-endpoint>`: trace export target (OpenTelemetry).
- `LOG_FORMAT text | json`: structured log contract.

### Lifecycle taxonomy

- `SESSION daemon | oneshot | transactional`: `daemon` (default) is long-running; `oneshot` runs to completion; `transactional` is a stateful, possibly-paid session. `ONESHOT true` is exact sugar for `SESSION oneshot` and is retained for backward compatibility; new files should prefer `SESSION oneshot`.

### Templated instances

- `SERVICE <name>@`: declares a template service.
- `INSTANCES <n>`: instantiate `name@1 .. name@n`.
- `%i` / `%0Ni`: the instance index in directive values; `%i` is unpadded, `%0Ni` is zero-padded to width N. When composing an index into a numeric token (a port), use a fixed-width form so width does not shift past a power of ten.

```
SERVICE worker@
INSTANCES 8
ENV SHARD=%i
READINESS http://localhost:96%02i/readyz
```

### Open assurance / label / profile facets

Opaque, namespaced facets the core carries verbatim. Their vocabulary is
interpreted by external **profile documents**, which expand them to concrete
directives; that expansion is pinned in output. The one core-interpreted
assurance key is the boolean `vital`, which marks a safety-critical path (it does
not propagate across `REQUIRES`).

- `ASSURANCE <k>=<v>...`: e.g. `vital=true sil=4 cert=en50128`. Only `vital` is core-interpreted; the rest is profile vocabulary.
- `LABEL <k>=<v>...`: handling labels, e.g. `classification=secret`.
- `PROFILE <name>[@<version>] [digest=<hash>]`: apply a named profile document.

```
ASSURANCE vital=true sil=4 cert=en50128
LABEL classification=secret
PROFILE pci-dss@1.2 digest=sha256:...
```

---

## File Composition (Overlay Model)

Orchfile supports multi-file composition using a systemd drop-in inspired overlay model. A base Orchfile can be overlaid with environment-specific files (staging, CI, personal) that tune resources, ports, env vars, and dependencies without forking the entire file.

### Usage

```
orch parse base.orch staging.orch personal.orch [--arg ...]
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
- **SECRET**: Merged by secret name (overlay wins on conflict)
- **PUBLISH**: Merged by container port (overlay replaces host address and port for same container port)
- **VOLUME**: Merged by destination path (overlay replaces source for same destination)

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

The `CLEAR` directive resets a list-type field before applying overlay values. Without `CLEAR`, overlay values merge with (append to) base values.

```
CLEAR ENV
CLEAR ENV_FILE
CLEAR SECRET
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

ARGs are merged left-to-right (last wins). Variable expansion is deferred until all files are merged, so overlays can redefine ARG defaults that affect the base file's `${var}` references.

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

CLI `--arg` overrides take highest priority, above all file defaults.

### Validation

Constraints (C1-C4) are validated on the **merged result**, not on individual files. An overlay file that only sets `ENV` without `FROM` or `RUN` is valid — it inherits the mode from the base file.

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


