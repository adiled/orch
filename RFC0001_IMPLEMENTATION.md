# RFC 0001 Parser Implementation Plan

## Overview
Implement parsing support for the RFC 0001 directive set (node unit-spec extensions across time, space, and trust axes).

## RFC 0001 Directives to Add

### Time Axis
- `STARTUP` - startup type (systemd-style)
- `LIVENESS` - health-based restart trigger
- `READINESS` - gate dependents/traffic
- `READY notify|poll` - ready-check method
- `WATCHDOG` - hang detection
- `LIFECYCLE managed|simple` - service lifecycle type

### Space Axis  
- `MACHINE_STATES` / `DEFAULT_STATE` - machine state management
- `STATE`, `GROUP`, `SLICE` - systemd unit organization
- `ARCH` - architecture requirements
- `DEVICE` - device access requirements
- `REQUIRES_CAP` - admission control check

### Trust Axis
- `CONDITION`, `ASSERT`, `WINDOW` - predicates (one ABI provider)
- `REQUIRES_HEALTHY` - dependency on healthy state
- `ON_FAILURE` - failure action handler
- `CAPABILITY` - Linux capability requirements
- `READONLY_ROOT` - read-only rootfs
- `NO_NEW_PRIVILEGES` - security flag
- `PRIVATE_TMP` - private temp directory
- `SECCOMP` - seccomp filter
- `EPHEMERAL` - stateless service
- `ON_TAMPER` - tamper detection response
- `SECRET <NAME> from <ref>` - secret reference (no plaintext)
- `IDENTITY [scope=]` - identity management
- `AUDIT` - audit logging

### Other
- `UPDATE`, `ROLLOUT` - change management
- `METRICS`, `TRACES`, `LOG_FORMAT` - observability
- `SESSION daemon|oneshot|transactional` - session type
- `SERVICE name@` - templated service names
- `INSTANCES <count>` - instance count
- `%i`, `%0Ni` - template substitutions
- `ASSURANCE`, `LABEL`, `PROFILE` - open facets

## Implementation Steps

### Phase 1: Grammar Updates (`grammar.ebnf`)
1. Add new directive types to grammar
2. Define terminal productions for values (durations, percentages, etc.)
3. Ensure backward compatibility with existing directives

### Phase 2: Type Definitions (`types.rs`)
1. Extend `RawService` with RFC 0001 fields
2. Add enums for policy values (managed/simple, notify/poll, etc.)
3. Create struct types for complex directives (secrets, identity)

### Phase 3: Parser Updates (`parser.rs`)
1. Parse new directive names in `parse_raw()`
2. Handle value parsing for each new directive type
3. Add validation helper functions as needed

### Phase 4: Resolution (`resolve.rs`)
1. Type coercion for string-to-typed values
2. Constraint validation (DAG checks, etc.)
3. Variable expansion for new directives

### Phase 5: Spec Update (`SPEC.md`)
1. Document each new directive
2. Add examples
3. Specify merge semantics for overlays

## Testing Strategy
1. Add test cases in `parser.rs` test module
2. Create integration tests in `tests/`
3. Test overlay merging behavior

## Files to Modify
- `grammar.ebnf` - EBNF grammar definition
- `src/types.rs` - data structures
- `src/parser.rs` - parsing logic
- `src/resolve.rs` - resolution/validation
- `SPEC.md` - specification document
- `docs/grammar.pdf` (regenerated from grammar.ebnf)

## Notes
- Parser deliberately untouched in PR#6; this implementation creates the parser
- RFC 0001 is additive to existing 1.0.0-rc baseline
- No version bump required for RFC 0001 additions
- SECRET references must NOT resolve/emit plaintext values
