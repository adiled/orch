# RFC 0001 Parser Implementation Plan (in rfc0001-parser branch)

## Current State
- Branch: `rfc0001-parser` (created from main)
- PR #6 (`feat/orchfile-rfc-0001`) contains grammar updates only
- No type changes in RFC branch - parser handles new directives via existing RawService fields

## New EBNF Grammar Directives (from PR/feat/orchfile-rfc-0001)

### File-Global (before SERVICE)
- `MACHINE_STATES state_name*` 
- `DEFAULT_STATE state_name`

### Service Block Directives

#### State Machine
- `STATE state_name*`
- `GROUP identifier`
- `SLICE identifier`

#### Dependencies
- `REQUIRES service_name*` (existing, extended)
- `REQUIRES_HEALTHY service_name*` (NEW)
- `AFTER service_name*` (existing)

#### Health & Lifecycle
- `HEALTHCHECK url | command_string` (existing, = READINESS only now)
- `STARTUP probe`
- `LIVENESS probe`
- `READINESS probe` (distinct from existing HEALTHCHECK variant)
- `READY notify | poll`
- `WATCHDOG duration`
- `LIFECYCLE managed | simple`
- `READINESS_TIMEOUT duration` (existing)
- `SESSION daemon | oneshot | transactional`

#### Conditions/Failure
- `CONDITION predicate`
- `ASSERT predicate`
- `WINDOW predicate`
- `ON_FAILURE fail_action`

#### Requirements (Admission)
- `ARCH identifier`
- `DEVICE path`
- `REQUIRES_CAP cap (, cap)*`

#### Security/Trust
- `CAPABILITY (drop:capset)? (add:capset)?`
- `READONLY_ROOT bool`
- `NO_NEW_PRIVILEGES bool`
- `PRIVATE_TMP bool`
- `SECCOMP identifier`
- `EPHEMERAL bool`
- `ON_TAMPER tamper_action`
- `SECRET env_key from secret_ref`
- `IDENTITY spiffe_id (scope=identity_scope)?`
- `AUDIT audit_kind`

#### Change
- `UPDATE kv*`
- `ROLLOUT kv*`

#### Observability
- `METRICS url (format=identifier)?`
- `TRACES url`
- `LOG_FORMAT text | json`

#### Open Facets
- `ASSURANCE kv*`
- `LABEL kv*`
- `PROFILE identifier (@token)? (digest=token)?`

#### Scaling
- `INSTANCES integer | ${var}`

## Implementation Approach

### Phase 1: Parse New Directives into RawService Fields
All new directives store raw values in existing `RawService` fields (strings, Vecs, HashMaps):
- Use new struct fields *or* extensible storage for key-value pairs
- Most can use existing string/duration/bool parsing helpers

### Phase 2: Add Helper Parsers
- Parse probes (`'exec' command_string | url | command_string`)
- Parse predicates (provider syntax only - values opaque to orch)
- Parse capset format (`drop:ALL` or `add:NET_BIND_SERVICE, ...`)
- Parse spiffe_id format

### Phase 3: Update CLEAR Targets
Add `'SECRET'` to `CLEARABLE_DIRECTIVES`

### Phase 4: Template Service Names
- Allow `SERVICE name@` in grammar
- Store template flag in RawService

### Phase 5: Implement Resolve-Time Type Coercion
Convert raw strings to typed values:
- bool: "true"/"false"
- duration: parse integer + 's'/'m'
- etc.

## Files to Modify (in rfc0001-parser branch)

1. **src/parser.rs**: Add matching match arms for all new directives
2. **src/types.rs** (optional): If we need typed fields, add them
3. **tests**: Add test cases for RFC 0001 directives

## Notes from PR Description

> This is a grammar + spec diff only. The hand-rolled parser is deliberately untouched; `grammar.ebnf` is the normative artifact and is self-sufficient for parser implementation in a follow-up.

So we're implementing the parser now (the "follow-up").
Parser doesn't implement the semantic meaning - just syntax.
Values are stored as strings, type coercion happens at resolve time.
