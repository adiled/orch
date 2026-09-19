use std::collections::HashMap;

use crate::types::{ClearableMap, ClearableVec, RawOrchFile, RawService};

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

    // Keyed list: PUBLISH (merge by container port — third element)
    merge_keyed_vec(&mut base.publish, overlay.publish, |e| e.2.as_str());

    // Keyed list: VOLUME (merge by destination — second element)
    merge_keyed_vec(&mut base.volumes, overlay.volumes, |e| e.1.as_str());

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

/// Keyed vec merge (PUBLISH, VOLUME): merge by the key produced by `key_of`.
/// If cleared, discard base values first; otherwise overlay entries replace
/// base entries with the same key, and new keys are appended.
fn merge_keyed_vec<T, F>(base: &mut ClearableVec<T>, overlay: ClearableVec<T>, key_of: F)
where
    F: Fn(&T) -> &str,
{
    if overlay.cleared {
        base.values.clear();
    }

    for entry in overlay.values {
        let key = key_of(&entry).to_string();

        // Find and replace existing entry with same key, or append
        if let Some(existing) = base.values.iter_mut().find(|e| key_of(e) == key) {
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
#[allow(non_snake_case)]
mod tests {
    use crate::merge::merge;
    use crate::types::{RawOrchFile, RawService, Source};

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    fn raw_file(args: Vec<(&str, &str)>, services: Vec<RawService>) -> RawOrchFile {
        RawOrchFile {
            args: args
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            services,
        }
    }

    fn raw_svc(name: &str, file_index: usize) -> RawService {
        RawService::new(name.to_string(), file_index)
    }

    fn svc_with_from(name: &str, image: &str, file_index: usize) -> RawService {
        let mut s = raw_svc(name, file_index);
        s.from = Some(image.to_string());
        s.from_source = Some(Source::new(2, file_index));
        s
    }

    fn svc_with_run(name: &str, cmd: &str, file_index: usize) -> RawService {
        let mut s = raw_svc(name, file_index);
        s.run = Some(cmd.to_string());
        s.run_source = Some(Source::new(2, file_index));
        s
    }

    // ---------------------------------------------------------------------------
    // ARG merge
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__args_last_wins() {
        let f1 = raw_file(vec![("port", "8080"), ("host", "localhost")], vec![]);
        let f2 = raw_file(vec![("port", "9090")], vec![]);
        let result = merge(vec![f1, f2]);

        assert_eq!(result.args.get("port").unwrap(), "9090");
        assert_eq!(result.args.get("host").unwrap(), "localhost");
    }

    #[test]
    fn test_merge__args_three_files() {
        let f1 = raw_file(vec![("x", "1")], vec![]);
        let f2 = raw_file(vec![("x", "2"), ("y", "10")], vec![]);
        let f3 = raw_file(vec![("x", "3")], vec![]);
        let result = merge(vec![f1, f2, f3]);

        assert_eq!(result.args.get("x").unwrap(), "3");
        assert_eq!(result.args.get("y").unwrap(), "10");
    }

    // ---------------------------------------------------------------------------
    // Scalar last-wins
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__scalar_from_last_wins() {
        let s1 = svc_with_from("db", "postgres:15", 0);
        let mut s2 = raw_svc("db", 1);
        s2.from = Some("postgres:16".to_string());
        s2.from_source = Some(Source::new(2, 1));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);

        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].from.as_deref(), Some("postgres:16"));
    }

    #[test]
    fn test_merge__scalar_overlay_none_does_not_clear() {
        let mut s1 = svc_with_from("web", "nginx:latest", 0);
        s1.workdir = Some("/app".to_string());

        // Overlay sets FROM but not workdir
        let mut s2 = raw_svc("web", 1);
        s2.from = Some("nginx:alpine".to_string());
        s2.from_source = Some(Source::new(2, 1));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);

        assert_eq!(result.services[0].from.as_deref(), Some("nginx:alpine"));
        assert_eq!(result.services[0].workdir.as_deref(), Some("/app"));
    }

    #[test]
    fn test_merge__all_scalars_overlay() {
        let mut s1 = svc_with_from("web", "nginx:latest", 0);
        s1.entrypoint = Some("/old-entry".to_string());
        s1.cmd = Some("old-cmd".to_string());
        s1.workdir = Some("/old".to_string());
        s1.healthcheck = Some("curl old".to_string());
        s1.readiness_timeout = Some("10".to_string());
        s1.oneshot = Some("false".to_string());
        s1.disabled = Some("false".to_string());
        s1.recreate = Some("never".to_string());
        s1.restart = Some("no".to_string());
        s1.restart_delay = Some("1s".to_string());
        s1.start_limit_burst = Some("3".to_string());
        s1.start_limit_interval = Some("60s".to_string());
        s1.timeout_start = Some("10s".to_string());
        s1.timeout_stop = Some("10s".to_string());
        s1.memory = Some("1G".to_string());
        s1.cpus = Some("1.0".to_string());
        s1.cpu_quota = Some("50%".to_string());
        s1.limit_nofile = Some("1024".to_string());
        s1.limit_nproc = Some("512".to_string());
        s1.tasks_max = Some("100".to_string());
        s1.io_weight = Some("50".to_string());
        s1.stdout = Some("/old.log".to_string());
        s1.stderr = Some("/old.err".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.memory = Some("4G".to_string());
        s2.cpus = Some("2.0".to_string());
        s2.stdout = Some("/new.log".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let svc = &result.services[0];

        // Overridden
        assert_eq!(svc.memory.as_deref(), Some("4G"));
        assert_eq!(svc.cpus.as_deref(), Some("2.0"));
        assert_eq!(svc.stdout.as_deref(), Some("/new.log"));
        // Preserved from base
        assert_eq!(svc.entrypoint.as_deref(), Some("/old-entry"));
        assert_eq!(svc.healthcheck.as_deref(), Some("curl old"));
        assert_eq!(svc.stderr.as_deref(), Some("/old.err"));
        assert_eq!(svc.limit_nofile.as_deref(), Some("1024"));
    }

    // ---------------------------------------------------------------------------
    // Keyed map merge: ENV
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__env_merge_by_key() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env.values.insert("A".to_string(), "1".to_string());
        s1.env.values.insert("B".to_string(), "2".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.env
            .values
            .insert("B".to_string(), "override".to_string());
        s2.env.values.insert("C".to_string(), "3".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let env = &result.services[0].env.values;

        assert_eq!(env.get("A").unwrap(), "1");
        assert_eq!(env.get("B").unwrap(), "override");
        assert_eq!(env.get("C").unwrap(), "3");
    }

    #[test]
    fn test_merge__env_clear_then_set() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env.values.insert("OLD".to_string(), "val".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.env.cleared = true;
        s2.env.values.insert("NEW".to_string(), "fresh".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let env = &result.services[0].env.values;

        assert!(!env.contains_key("OLD"));
        assert_eq!(env.get("NEW").unwrap(), "fresh");
        assert!(result.services[0].env.cleared);
    }

    #[test]
    fn test_merge__env_clear_only() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env.values.insert("OLD".to_string(), "val".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.env.cleared = true;

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert!(result.services[0].env.values.is_empty());
    }

    // ---------------------------------------------------------------------------
    // Keyed vec merge: PUBLISH (keyed by container port)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__publish_merge_by_container_port() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.publish
            .values
            .push(("".to_string(), "8080".to_string(), "80".to_string()));
        s1.publish
            .values
            .push(("".to_string(), "8443".to_string(), "443".to_string()));

        let mut s2 = raw_svc("web", 1);
        s2.publish
            .values
            .push(("".to_string(), "9090".to_string(), "80".to_string())); // override port 80
        s2.publish
            .values
            .push(("".to_string(), "3000".to_string(), "3000".to_string())); // new

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let pubs = &result.services[0].publish.values;

        assert_eq!(pubs.len(), 3);
        // Port 80: overridden host from 8080 to 9090
        let p80 = pubs.iter().find(|(_, _, c)| c == "80").unwrap();
        assert_eq!(p80.1, "9090");
        // Port 443: unchanged
        let p443 = pubs.iter().find(|(_, _, c)| c == "443").unwrap();
        assert_eq!(p443.1, "8443");
        // Port 3000: new
        let p3000 = pubs.iter().find(|(_, _, c)| c == "3000").unwrap();
        assert_eq!(p3000.1, "3000");
    }

    #[test]
    fn test_merge__publish_clear_then_add() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.publish
            .values
            .push(("".to_string(), "8080".to_string(), "80".to_string()));

        let mut s2 = raw_svc("web", 1);
        s2.publish.cleared = true;
        s2.publish
            .values
            .push(("".to_string(), "9090".to_string(), "80".to_string()));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let pubs = &result.services[0].publish.values;

        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].1, "9090");
        assert_eq!(pubs[0].2, "80");
    }

    // ---------------------------------------------------------------------------
    // Keyed vec merge: VOLUME (keyed by destination)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__volume_merge_by_dest() {
        let mut s1 = svc_with_from("db", "postgres", 0);
        s1.volumes
            .values
            .push(("pgdata".to_string(), "/var/lib/postgresql/data".to_string()));
        s1.volumes
            .values
            .push(("./config".to_string(), "/etc/postgres".to_string()));

        let mut s2 = raw_svc("db", 1);
        // Override source for same dest
        s2.volumes
            .values
            .push(("./local-config".to_string(), "/etc/postgres".to_string()));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let vols = &result.services[0].volumes.values;

        assert_eq!(vols.len(), 2);
        let etc = vols.iter().find(|(_, d)| d == "/etc/postgres").unwrap();
        assert_eq!(etc.0, "./local-config");
    }

    // ---------------------------------------------------------------------------
    // Positional list merge: REQUIRES, AFTER, ENV_FILE
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__requires_append_dedup() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.requires.values.push("db".to_string());
        s1.requires.values.push("redis".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.requires.values.push("redis".to_string()); // duplicate — should be deduped
        s2.requires.values.push("cache".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let reqs = &result.services[0].requires.values;

        assert_eq!(reqs, &["db", "redis", "cache"]);
    }

    #[test]
    fn test_merge__after_append_dedup() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.after.values.push("db".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.after.values.push("db".to_string());
        s2.after.values.push("redis".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert_eq!(result.services[0].after.values, &["db", "redis"]);
    }

    #[test]
    fn test_merge__env_file_append_dedup() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env_files.values.push(".env".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.env_files.values.push(".env".to_string());
        s2.env_files.values.push(".env.local".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert_eq!(result.services[0].env_files.values, &[".env", ".env.local"]);
    }

    #[test]
    fn test_merge__requires_clear_then_add() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.requires.values.push("db".to_string());
        s1.requires.values.push("redis".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.requires.cleared = true;
        s2.requires.values.push("cache".to_string());

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert_eq!(result.services[0].requires.values, &["cache"]);
    }

    // ---------------------------------------------------------------------------
    // Mode switching: FROM ↔ RUN
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__switch_to_host_clears_container_directives() {
        let mut s1 = svc_with_from("svc", "nginx:latest", 0);
        s1.entrypoint = Some("/docker-entrypoint.sh".to_string());
        s1.cmd = Some("nginx -g 'daemon off;'".to_string());
        s1.publish
            .values
            .push(("".to_string(), "8080".to_string(), "80".to_string()));
        s1.volumes
            .values
            .push(("./html".to_string(), "/usr/share/nginx/html".to_string()));
        s1.container_directives_used
            .push(("ENTRYPOINT".to_string(), 3, 0));
        s1.container_directives_used.push(("CMD".to_string(), 4, 0));
        s1.container_directives_used
            .push(("PUBLISH".to_string(), 5, 0));
        s1.container_directives_used
            .push(("VOLUME".to_string(), 6, 0));

        let s2 = svc_with_run("svc", "/usr/local/bin/myapp", 1);

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let svc = &result.services[0];

        // FROM cleared, RUN set
        assert!(svc.from.is_none());
        assert_eq!(svc.run.as_deref(), Some("/usr/local/bin/myapp"));
        // Container-only directives cleared
        assert!(svc.entrypoint.is_none());
        assert!(svc.cmd.is_none());
        assert!(svc.publish.values.is_empty());
        assert!(svc.volumes.values.is_empty());
        assert!(svc.container_directives_used.is_empty());
    }

    #[test]
    fn test_merge__switch_to_container_clears_host_directives() {
        let mut s1 = svc_with_run("svc", "/usr/local/bin/myapp", 0);
        s1.user = Some("nobody".to_string());
        s1.stop_command = Some("kill $MAINPID".to_string());
        s1.reload_command = Some("kill -HUP $MAINPID".to_string());
        s1.host_directives_used.push(("USER".to_string(), 3, 0));
        s1.host_directives_used.push(("STOP".to_string(), 4, 0));
        s1.host_directives_used.push(("RELOAD".to_string(), 5, 0));

        let s2 = svc_with_from("svc", "myapp:latest", 1);

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let svc = &result.services[0];

        // RUN cleared, FROM set
        assert!(svc.run.is_none());
        assert_eq!(svc.from.as_deref(), Some("myapp:latest"));
        // Host-only directives cleared
        assert!(svc.user.is_none());
        assert!(svc.stop_command.is_none());
        assert!(svc.reload_command.is_none());
        assert!(svc.host_directives_used.is_empty());
    }

    // ---------------------------------------------------------------------------
    // New services from overlay
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__new_service_from_overlay() {
        let s1 = svc_with_from("web", "nginx", 0);
        let s2 = svc_with_from("cache", "redis:7", 1);

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);

        assert_eq!(result.services.len(), 2);
        assert_eq!(result.services[0].name, "web");
        assert_eq!(result.services[1].name, "cache");
    }

    #[test]
    fn test_merge__overlay_adds_and_overrides() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env.values.insert("PORT".to_string(), "80".to_string());
        let s1b = svc_with_from("db", "postgres:15", 0);

        let mut s2_web = raw_svc("web", 1);
        s2_web
            .env
            .values
            .insert("PORT".to_string(), "8080".to_string());
        let s2_cache = svc_with_from("cache", "redis:7", 1);

        let result = merge(vec![
            raw_file(vec![], vec![s1, s1b]),
            raw_file(vec![], vec![s2_web, s2_cache]),
        ]);

        assert_eq!(result.services.len(), 3);
        assert_eq!(result.services[0].env.values.get("PORT").unwrap(), "8080");
        assert_eq!(result.services[1].name, "db");
        assert_eq!(result.services[2].name, "cache");
    }

    // ---------------------------------------------------------------------------
    // Single file pass-through
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__single_file_identity() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.env.values.insert("A".to_string(), "1".to_string());
        s1.requires.values.push("db".to_string());

        let result = merge(vec![raw_file(vec![("port", "8080")], vec![s1])]);

        assert_eq!(result.args.get("port").unwrap(), "8080");
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].from.as_deref(), Some("nginx"));
        assert_eq!(result.services[0].env.values.get("A").unwrap(), "1");
        assert_eq!(result.services[0].requires.values, &["db"]);
    }

    // ---------------------------------------------------------------------------
    // Three-file merge
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__three_file_cascade() {
        let mut s1 = svc_with_from("web", "nginx:1.24", 0);
        s1.env.values.insert("MODE".to_string(), "prod".to_string());
        s1.publish
            .values
            .push(("".to_string(), "80".to_string(), "80".to_string()));
        s1.requires.values.push("db".to_string());

        let mut s2 = raw_svc("web", 1);
        s2.from = Some("nginx:1.25".to_string());
        s2.from_source = Some(Source::new(2, 1));
        s2.env
            .values
            .insert("MODE".to_string(), "staging".to_string());
        s2.publish
            .values
            .push(("".to_string(), "8080".to_string(), "80".to_string())); // override host for port 80

        let mut s3 = raw_svc("web", 2);
        s3.env
            .values
            .insert("DEBUG".to_string(), "true".to_string());
        s3.requires.values.push("cache".to_string());

        let result = merge(vec![
            raw_file(vec![], vec![s1]),
            raw_file(vec![], vec![s2]),
            raw_file(vec![], vec![s3]),
        ]);

        let svc = &result.services[0];
        assert_eq!(svc.from.as_deref(), Some("nginx:1.25"));
        assert_eq!(svc.env.values.get("MODE").unwrap(), "staging");
        assert_eq!(svc.env.values.get("DEBUG").unwrap(), "true");
        // Port 80 host overridden to 8080
        assert_eq!(svc.publish.values.len(), 1);
        assert_eq!(svc.publish.values[0].1, "8080");
        assert_eq!(svc.publish.values[0].2, "80");
        // Requires: db + cache (deduped)
        assert_eq!(svc.requires.values, &["db", "cache"]);
    }

    // ---------------------------------------------------------------------------
    // Empty overlay
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__empty_overlay() {
        let s1 = svc_with_from("web", "nginx", 0);
        let result = merge(vec![
            raw_file(vec![("p", "1")], vec![s1]),
            raw_file(vec![], vec![]),
        ]);

        assert_eq!(result.args.get("p").unwrap(), "1");
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].from.as_deref(), Some("nginx"));
    }

    #[test]
    fn test_merge__empty_base() {
        let s1 = svc_with_from("web", "nginx", 1);
        let result = merge(vec![raw_file(vec![], vec![]), raw_file(vec![], vec![s1])]);

        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].from.as_deref(), Some("nginx"));
    }

    // ---------------------------------------------------------------------------
    // file_index tracking
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__file_index_updated_to_last_file() {
        let s1 = svc_with_from("web", "nginx", 0);
        let s2 = raw_svc("web", 1);

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert_eq!(result.services[0].file_index, 1);
    }

    // ---------------------------------------------------------------------------
    // C2/C3 directive tracking
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__container_directives_extend() {
        let mut s1 = svc_with_from("web", "nginx", 0);
        s1.container_directives_used
            .push(("ENTRYPOINT".to_string(), 3, 0));

        let mut s2 = raw_svc("web", 1);
        s2.container_directives_used.push(("CMD".to_string(), 2, 1));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        assert_eq!(result.services[0].container_directives_used.len(), 2);
    }

    // ---------------------------------------------------------------------------
    // CLEAR on volumes
    // ---------------------------------------------------------------------------

    #[test]
    fn test_merge__volume_clear_then_add() {
        let mut s1 = svc_with_from("db", "postgres", 0);
        s1.volumes
            .values
            .push(("pgdata".to_string(), "/data".to_string()));
        s1.volumes
            .values
            .push(("./conf".to_string(), "/etc".to_string()));

        let mut s2 = raw_svc("db", 1);
        s2.volumes.cleared = true;
        s2.volumes
            .values
            .push(("tmpdata".to_string(), "/data".to_string()));

        let result = merge(vec![raw_file(vec![], vec![s1]), raw_file(vec![], vec![s2])]);
        let vols = &result.services[0].volumes.values;

        assert_eq!(vols.len(), 1);
        assert_eq!(vols[0].0, "tmpdata");
        assert_eq!(vols[0].1, "/data");
    }
}
