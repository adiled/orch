use std::collections::HashMap;

use orch::types::{ClearableMap, ClearableVec, RawOrchFile, RawService};

/// Merge multiple raw Orchfile representations left-to-right.
///
/// - ARGs: last wins
/// - Services matched by name across files
/// - Scalars: last wins (overlay replaces base)
/// - Keyed lists (ENV, PUBLISH, VOLUME): merge by key, overlay wins on conflict
/// - Positional lists (REQUIRES, AFTER, ENV_FILE): append with dedup
/// - CLEAR flags: discard base values before applying overlay
/// - FROM/RUN special: setting FROM clears RUN + host-only directives, and vice versa
/// - New services from later files: appended to result
pub fn merge(files: Vec<RawOrchFile>) -> RawOrchFile {
    let mut result = RawOrchFile {
        args: HashMap::new(),
        services: Vec::new(),
    };

    for file in files {
        // Merge ARGs: last wins
        for (k, v) in file.args {
            result.args.insert(k, v);
        }

        // Merge services by name
        for overlay_svc in file.services {
            if let Some(base_svc) = result
                .services
                .iter_mut()
                .find(|s| s.name == overlay_svc.name)
            {
                merge_service(base_svc, overlay_svc);
            } else {
                // New service — add as-is
                result.services.push(overlay_svc);
            }
        }
    }

    result
}

/// Merge an overlay service into a base service (mutates base in place).
fn merge_service(base: &mut RawService, overlay: RawService) {
    // Handle mode switching: FROM clears RUN + host-only, RUN clears FROM + container-only
    if overlay.from.is_some() && base.run.is_some() {
        // Switching to container mode: clear host-only directives
        base.run = None;
        base.run_source = None;
        base.user = None;
        base.stop_command = None;
        base.reload_command = None;
        base.host_directives_used.clear();
    }
    if overlay.run.is_some() && base.from.is_some() {
        // Switching to host mode: clear container-only directives
        base.from = None;
        base.from_source = None;
        base.entrypoint = None;
        base.cmd = None;
        base.publish.values.clear();
        base.publish.cleared = true;
        base.volumes.values.clear();
        base.volumes.cleared = true;
        base.container_directives_used.clear();
    }

    // Scalar last-wins: overlay replaces base for each Some field
    merge_scalar(&mut base.from, &overlay.from);
    if overlay.from_source.is_some() {
        base.from_source = overlay.from_source;
    }
    merge_scalar(&mut base.run, &overlay.run);
    if overlay.run_source.is_some() {
        base.run_source = overlay.run_source;
    }

    merge_scalar(&mut base.entrypoint, &overlay.entrypoint);
    merge_scalar(&mut base.cmd, &overlay.cmd);
    merge_scalar(&mut base.user, &overlay.user);
    merge_scalar(&mut base.stop_command, &overlay.stop_command);
    merge_scalar(&mut base.reload_command, &overlay.reload_command);
    merge_scalar(&mut base.workdir, &overlay.workdir);
    merge_scalar(&mut base.healthcheck, &overlay.healthcheck);
    merge_scalar(&mut base.readiness_timeout, &overlay.readiness_timeout);
    merge_scalar(&mut base.oneshot, &overlay.oneshot);
    merge_scalar(&mut base.disabled, &overlay.disabled);
    merge_scalar(&mut base.recreate, &overlay.recreate);
    merge_scalar(&mut base.restart, &overlay.restart);
    merge_scalar(&mut base.restart_delay, &overlay.restart_delay);
    merge_scalar(&mut base.start_limit_burst, &overlay.start_limit_burst);
    merge_scalar(
        &mut base.start_limit_interval,
        &overlay.start_limit_interval,
    );
    merge_scalar(&mut base.timeout_start, &overlay.timeout_start);
    merge_scalar(&mut base.timeout_stop, &overlay.timeout_stop);
    merge_scalar(&mut base.memory, &overlay.memory);
    merge_scalar(&mut base.cpus, &overlay.cpus);
    merge_scalar(&mut base.cpu_quota, &overlay.cpu_quota);
    merge_scalar(&mut base.limit_nofile, &overlay.limit_nofile);
    merge_scalar(&mut base.limit_nproc, &overlay.limit_nproc);
    merge_scalar(&mut base.tasks_max, &overlay.tasks_max);
    merge_scalar(&mut base.io_weight, &overlay.io_weight);
    merge_scalar(&mut base.stdout, &overlay.stdout);
    merge_scalar(&mut base.stderr, &overlay.stderr);

    // Keyed map: ENV (merge by var name)
    merge_keyed_map(&mut base.env, overlay.env);

    // Keyed list: PUBLISH (merge by container port — second element)
    merge_keyed_vec(&mut base.publish, overlay.publish, 1);

    // Keyed list: VOLUME (merge by destination — second element)
    merge_keyed_vec(&mut base.volumes, overlay.volumes, 1);

    // Positional lists: append + dedup
    merge_positional(&mut base.requires, overlay.requires);
    merge_positional(&mut base.after, overlay.after);
    merge_positional(&mut base.env_files, overlay.env_files);

    // C2/C3 provenance: append overlay's tracking
    if !overlay.container_directives_used.is_empty() {
        base.container_directives_used
            .extend(overlay.container_directives_used);
    }
    if !overlay.host_directives_used.is_empty() {
        base.host_directives_used
            .extend(overlay.host_directives_used);
    }

    // Update file_index to the last file that touched this service
    base.file_index = overlay.file_index;
}

/// Scalar merge: overlay replaces base if Some.
fn merge_scalar(base: &mut Option<String>, overlay: &Option<String>) {
    if overlay.is_some() {
        *base = overlay.clone();
    }
}

/// Keyed map merge (ENV): if cleared, discard base; then merge by key.
fn merge_keyed_map(base: &mut ClearableMap, overlay: ClearableMap) {
    if overlay.cleared {
        base.values.clear();
    }
    for (k, v) in overlay.values {
        base.values.insert(k, v);
    }
    // Once cleared, the flag stays (downstream knows base was wiped)
    if overlay.cleared {
        base.cleared = true;
    }
}

/// Keyed vec merge (PUBLISH, VOLUME): merge by element at `key_index`.
/// If cleared, discard base values first.
fn merge_keyed_vec(
    base: &mut ClearableVec<(String, String)>,
    overlay: ClearableVec<(String, String)>,
    key_index: usize,
) {
    if overlay.cleared {
        base.values.clear();
    }

    for entry in overlay.values {
        let key = if key_index == 0 { &entry.0 } else { &entry.1 };

        // Find and replace existing entry with same key, or append
        if let Some(existing) = base.values.iter_mut().find(|e| {
            let existing_key = if key_index == 0 { &e.0 } else { &e.1 };
            existing_key == key
        }) {
            *existing = entry;
        } else {
            base.values.push(entry);
        }
    }

    if overlay.cleared {
        base.cleared = true;
    }
}

/// Positional list merge: if cleared, discard base; then append + dedup.
fn merge_positional(base: &mut ClearableVec<String>, overlay: ClearableVec<String>) {
    if overlay.cleared {
        base.values.clear();
    }

    for val in overlay.values {
        if !base.values.contains(&val) {
            base.values.push(val);
        }
    }

    if overlay.cleared {
        base.cleared = true;
    }
}

#[cfg(test)]
mod tests;
