use std::collections::{BTreeMap, HashMap};
use std::net::Ipv6Addr;

use crate::error::ParseError;
use crate::lex::{is_ws, reject_control_chars};
use crate::names::{check_service_name, is_identifier, is_user_name};
use crate::syntax::{Block, Directive, ListField};
use crate::types::{PortMapping, RecreatePolicy, RestartPolicy, ServiceMode, VolumeMount};
use crate::vars;

const MAX_HOSTNAME_LABEL_LEN: usize = 63;
const IO_WEIGHT_RANGE: std::ops::RangeInclusive<u32> = 1..=10000;

#[derive(Debug, Clone)]
pub(crate) enum Setting {
    Image(String),
    RunCommand(String),
    Entrypoint(String),
    Cmd(String),
    Publish(PortMapping),
    Volume(VolumeMount),
    User(String),
    StopCommand(String),
    ReloadCommand(String),
    Workdir(String),
    Env { key: String, value: String },
    EnvFile(String),
    Requires(Vec<String>),
    After(Vec<String>),
    Healthcheck(String),
    ReadinessTimeout(String),
    Oneshot(bool),
    Disabled(bool),
    Recreate(RecreatePolicy),
    Restart(RestartPolicy),
    RestartDelay(String),
    StartLimitBurst(u32),
    StartLimitInterval(String),
    TimeoutStart(String),
    TimeoutStop(String),
    Memory(String),
    Cpus(f64),
    CpuQuota(String),
    LimitNofile(u64),
    LimitNproc(u64),
    TasksMax(u64),
    IoWeight(u32),
    Stdout(String),
    Stderr(String),
    Clear(ListField),
}

pub(crate) struct TypedEntry {
    pub line: usize,
    pub directive: Directive,
    pub setting: Setting,
}

pub(crate) struct TypedBlock {
    pub service: String,
    pub file: usize,
    pub mode: Option<ServiceMode>,
    pub entries: Vec<TypedEntry>,
}

pub(crate) fn typed_block(
    block: Block,
    file: usize,
    args: &BTreeMap<String, String>,
) -> Result<TypedBlock, Vec<ParseError>> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let mut repeats = RepeatCheck::default();

    for entry in block.entries {
        let typed = expanded(entry.directive, &entry.value, args).and_then(|text| {
            let setting = setting(entry.directive, &text).map_err(|m| vec![m])?;
            repeats
                .record(entry.line, entry.directive, &setting)
                .map_err(|m| vec![m])?;
            Ok(setting)
        });
        match typed {
            Ok(setting) => entries.push(TypedEntry {
                line: entry.line,
                directive: entry.directive,
                setting,
            }),
            Err(messages) => errors.extend(
                messages
                    .into_iter()
                    .map(|message| ParseError::new(entry.line, message)),
            ),
        }
    }

    if errors.is_empty() {
        Ok(TypedBlock {
            service: block.service,
            file,
            mode: block.mode.map(|(mode, _)| mode),
            entries,
        })
    } else {
        Err(errors)
    }
}

fn expanded(
    directive: Directive,
    value: &str,
    args: &BTreeMap<String, String>,
) -> Result<String, Vec<String>> {
    if directive == Directive::Clear {
        Ok(value.to_string())
    } else {
        vars::expand(value, args)
    }
}

#[derive(Default)]
struct RepeatCheck {
    scalars: HashMap<Directive, usize>,
    env_keys: HashMap<String, usize>,
    volume_destinations: HashMap<String, usize>,
}

impl RepeatCheck {
    fn record(
        &mut self,
        line: usize,
        directive: Directive,
        setting: &Setting,
    ) -> Result<(), String> {
        let first = match setting {
            Setting::Env { key, .. } => first_line(&mut self.env_keys, key.clone(), line)
                .map(|first| (format!("ENV {key}"), first)),
            Setting::Volume(volume) => first_line(
                &mut self.volume_destinations,
                volume.destination.clone(),
                line,
            )
            .map(|first| (format!("VOLUME destination {}", volume.destination), first)),
            _ if directive.is_scalar() => first_line(&mut self.scalars, directive, line)
                .map(|first| (directive.keyword().to_string(), first)),
            _ => None,
        };
        match first {
            Some((what, first)) => Err(format!("{what} is already set at line {first} (C6)")),
            None => Ok(()),
        }
    }
}

fn first_line<K: std::hash::Hash + Eq>(
    seen: &mut HashMap<K, usize>,
    key: K,
    line: usize,
) -> Option<usize> {
    match seen.get(&key) {
        Some(&first) => Some(first),
        None => {
            seen.insert(key, line);
            None
        }
    }
}

fn setting(directive: Directive, text: &str) -> Result<Setting, String> {
    let keyword = directive.keyword();
    reject_control_chars(text)?;
    Ok(match directive {
        Directive::From => Setting::Image(free_text(keyword, text)?),
        Directive::Run => Setting::RunCommand(free_text(keyword, text)?),
        Directive::Entrypoint => Setting::Entrypoint(free_text(keyword, text)?),
        Directive::Cmd => Setting::Cmd(free_text(keyword, text)?),
        Directive::Publish => Setting::Publish(port_mapping(text)?),
        Directive::Volume => Setting::Volume(volume_mount(text)?),
        Directive::User => Setting::User(user_name(text)?),
        Directive::Stop => Setting::StopCommand(free_text(keyword, text)?),
        Directive::Reload => Setting::ReloadCommand(free_text(keyword, text)?),
        Directive::Workdir => Setting::Workdir(path(keyword, text)?),
        Directive::Env => env(text)?,
        Directive::EnvFile => Setting::EnvFile(path(keyword, text)?),
        Directive::Requires => Setting::Requires(service_names(keyword, text)?),
        Directive::After => Setting::After(service_names(keyword, text)?),
        Directive::Healthcheck => Setting::Healthcheck(free_text(keyword, text)?),
        Directive::ReadinessTimeout => Setting::ReadinessTimeout(duration(keyword, text)?),
        Directive::Oneshot => Setting::Oneshot(flag(keyword, text)?),
        Directive::Disabled => Setting::Disabled(flag(keyword, text)?),
        Directive::Recreate => Setting::Recreate(recreate_policy(text)?),
        Directive::Restart => Setting::Restart(restart_policy(text)?),
        Directive::RestartDelay => Setting::RestartDelay(duration(keyword, text)?),
        Directive::StartLimitBurst => Setting::StartLimitBurst(integer(keyword, text)?),
        Directive::StartLimitInterval => Setting::StartLimitInterval(duration(keyword, text)?),
        Directive::TimeoutStart => Setting::TimeoutStart(duration(keyword, text)?),
        Directive::TimeoutStop => Setting::TimeoutStop(duration(keyword, text)?),
        Directive::Memory => Setting::Memory(memory_size(text)?),
        Directive::Cpus => Setting::Cpus(cpus(text)?),
        Directive::CpuQuota => Setting::CpuQuota(percentage(text)?),
        Directive::LimitNofile => Setting::LimitNofile(integer(keyword, text)?),
        Directive::LimitNproc => Setting::LimitNproc(integer(keyword, text)?),
        Directive::TasksMax => Setting::TasksMax(integer(keyword, text)?),
        Directive::IoWeight => Setting::IoWeight(io_weight(text)?),
        Directive::Stdout => Setting::Stdout(path(keyword, text)?),
        Directive::Stderr => Setting::Stderr(path(keyword, text)?),
        Directive::Clear => Setting::Clear(clear_target(text)?),
    })
}

fn has_edge_whitespace(text: &str) -> bool {
    text.starts_with(is_ws) || text.ends_with(is_ws)
}

fn free_text(keyword: &str, text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err(format!("{keyword} requires a value"));
    }
    if has_edge_whitespace(text) {
        return Err(format!(
            "{keyword} value \"{text}\" must not start or end with whitespace"
        ));
    }
    Ok(text.to_string())
}

fn is_rooted(path: &str) -> bool {
    path.starts_with('/') || path == "~" || path.starts_with("~/")
}

fn path(keyword: &str, text: &str) -> Result<String, String> {
    let path = free_text(keyword, text)?;
    if is_rooted(&path) {
        Ok(path)
    } else {
        Err(format!(
            "{keyword} path \"{path}\" must be absolute or start with ~"
        ))
    }
}

fn is_volume_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn integer<N: std::str::FromStr>(keyword: &str, text: &str) -> Result<N, String> {
    let not_a_number = || format!("{keyword} must be a number, got '{text}'");
    if !is_digits(text) {
        return Err(not_a_number());
    }
    text.parse().map_err(|_| not_a_number())
}

fn io_weight(text: &str) -> Result<u32, String> {
    let weight: u32 = integer("IO_WEIGHT", text)?;
    if IO_WEIGHT_RANGE.contains(&weight) {
        Ok(weight)
    } else {
        Err(format!(
            "IO_WEIGHT must be {}-{}, got {weight}",
            IO_WEIGHT_RANGE.start(),
            IO_WEIGHT_RANGE.end()
        ))
    }
}

fn integer_with_unit(text: &str, units: &[char]) -> bool {
    text.strip_suffix(units).is_some_and(is_digits)
}

fn duration(keyword: &str, text: &str) -> Result<String, String> {
    if integer_with_unit(text, &['s', 'm']) {
        Ok(text.to_string())
    } else {
        Err(format!(
            "{keyword} expects a whole number followed by s or m (like 30s or 5m), got '{text}'"
        ))
    }
}

fn memory_size(text: &str) -> Result<String, String> {
    if integer_with_unit(text, &['K', 'M', 'G']) {
        Ok(text.to_string())
    } else {
        Err(format!(
            "MEMORY expects a whole number followed by K, M or G (like 512M), got '{text}'"
        ))
    }
}

fn percentage(text: &str) -> Result<String, String> {
    if integer_with_unit(text, &['%']) {
        Ok(text.to_string())
    } else {
        Err(format!(
            "CPU_QUOTA expects a whole number followed by % (like 50%), got '{text}'"
        ))
    }
}

fn cpus(text: &str) -> Result<f64, String> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (text, None),
    };
    let is_number = is_digits(whole) && fraction.is_none_or(is_digits);
    is_number
        .then(|| text.parse().ok())
        .flatten()
        .ok_or_else(|| format!("CPUS must be a number, got '{text}'"))
}

fn flag(keyword: &str, text: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{keyword} expects 'true' or 'false', got '{text}'")),
    }
}

fn recreate_policy(text: &str) -> Result<RecreatePolicy, String> {
    match text {
        "always" => Ok(RecreatePolicy::Always),
        "never" => Ok(RecreatePolicy::Never),
        _ => Err(format!(
            "invalid RECREATE value '{text}', expected 'always' or 'never'"
        )),
    }
}

fn restart_policy(text: &str) -> Result<RestartPolicy, String> {
    match text {
        "no" => Ok(RestartPolicy::No),
        "always" => Ok(RestartPolicy::Always),
        "on-failure" => Ok(RestartPolicy::OnFailure),
        _ => Err(format!(
            "invalid RESTART value '{text}', expected 'no', 'always', or 'on-failure'"
        )),
    }
}

fn user_name(text: &str) -> Result<String, String> {
    if is_user_name(text) {
        Ok(text.to_string())
    } else {
        Err(format!(
            "invalid USER '{text}': use letters, digits, '_', '-' and '.', not starting with '-' or '.'"
        ))
    }
}

fn env(text: &str) -> Result<Setting, String> {
    let (key, value) = text
        .split_once('=')
        .ok_or_else(|| format!("invalid ENV format '{text}', expected KEY=value"))?;
    if key.is_empty() {
        return Err("ENV key cannot be empty".into());
    }
    if !is_identifier(key) {
        return Err(format!(
            "invalid ENV key '{key}': use letters, digits and '_', not starting with a digit"
        ));
    }
    if value.ends_with(is_ws) {
        return Err(format!("ENV {key} value must not end with whitespace"));
    }
    Ok(Setting::Env {
        key: key.to_string(),
        value: value.to_string(),
    })
}

fn service_names(keyword: &str, text: &str) -> Result<Vec<String>, String> {
    let names: Vec<String> = text
        .split(is_ws)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    if names.is_empty() {
        return Err(format!("{keyword} requires a value"));
    }
    names.iter().try_for_each(|name| check_service_name(name))?;
    Ok(names)
}

fn clear_target(text: &str) -> Result<ListField, String> {
    Directive::from_keyword(text)
        .and_then(Directive::list_field)
        .ok_or_else(|| format!("CLEAR targets unknown directive '{text}'"))
}

fn port_mapping(text: &str) -> Result<PortMapping, String> {
    let malformed =
        || format!("invalid PUBLISH format '{text}', expected [address:]host_port:container_port");
    let (rest, container) = text.rsplit_once(':').ok_or_else(malformed)?;
    let (address, host) = match rest.rsplit_once(':') {
        Some((address, host)) => (Some(address), host),
        None => (None, rest),
    };
    Ok(PortMapping {
        address: address.map(host_address).transpose()?,
        host: port("host", host)?,
        container: port("container", container)?,
    })
}

fn port(side: &str, text: &str) -> Result<u16, String> {
    is_digits(text)
        .then(|| text.parse::<u16>().ok())
        .flatten()
        .filter(|&port| port != 0)
        .ok_or_else(|| format!("invalid {side} port '{text}' in PUBLISH (expected 1-65535)"))
}

fn host_address(text: &str) -> Result<String, String> {
    if text.contains(['[', ']']) {
        return Err(format!(
            "invalid PUBLISH address '{text}': write IPv6 addresses without brackets"
        ));
    }
    let is_valid = if text.contains(':') {
        text.parse::<Ipv6Addr>().is_ok()
    } else {
        is_ipv4(text) || is_hostname(text)
    };
    if is_valid {
        Ok(text.to_string())
    } else {
        Err(format!(
            "invalid PUBLISH address '{text}': expected an IPv4 address, an IPv6 address, or a hostname"
        ))
    }
}

fn is_ipv4(text: &str) -> bool {
    let octets: Vec<&str> = text.split('.').collect();
    octets.len() == 4
        && octets
            .iter()
            .all(|octet| is_digits(octet) && octet.parse::<u32>().is_ok_and(|n| n <= 255))
}

fn is_hostname(text: &str) -> bool {
    text.split('.').all(is_hostname_label)
}

fn is_hostname_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_HOSTNAME_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn volume_mount(text: &str) -> Result<VolumeMount, String> {
    let malformed =
        || format!("invalid VOLUME format '{text}', expected source:destination[:ro|:rw]");
    let parts: Vec<&str> = text.split(':').collect();
    let (source, destination, mode) = match parts[..] {
        [source, destination] => (source, destination, "rw"),
        [source, destination, mode] => (source, destination, mode),
        _ => return Err(malformed()),
    };
    let is_part = |part: &str| !part.is_empty() && !has_edge_whitespace(part);
    if !is_part(source) || !is_part(destination) {
        return Err(malformed());
    }
    let read_only = match mode {
        "ro" => true,
        "rw" => false,
        _ => {
            return Err(format!(
                "invalid VOLUME mode '{mode}', expected 'ro' or 'rw'"
            ));
        }
    };
    let is_named = !is_rooted(source);
    if is_named && !is_volume_name(source) {
        return Err(format!(
            "invalid VOLUME source '{source}': use an absolute path, a path starting with ~, or a volume name"
        ));
    }
    if !is_rooted(destination) {
        return Err(format!(
            "VOLUME destination '{destination}' must be absolute or start with ~"
        ));
    }
    Ok(VolumeMount {
        source: source.to_string(),
        destination: without_trailing_slashes(destination),
        is_named,
        read_only,
    })
}

fn without_trailing_slashes(path: &str) -> String {
    match path.trim_end_matches('/') {
        "" => "/".to_string(),
        trimmed => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(text: &str) -> Result<PortMapping, String> {
        port_mapping(text)
    }

    #[test]
    fn publish_accepts_ipv4_ipv6_and_hostnames() {
        assert_eq!(
            publish("127.0.0.1:8080:80").unwrap().address.as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            publish("::1:8080:80").unwrap().address.as_deref(),
            Some("::1")
        );
        assert_eq!(
            publish(":::8080:80").unwrap().address.as_deref(),
            Some("::")
        );
        assert_eq!(
            publish("my-host.local:8080:80").unwrap().address.as_deref(),
            Some("my-host.local")
        );
        assert_eq!(publish("8080:80").unwrap().address, None);
    }

    #[test]
    fn publish_rejects_malformed_addresses_and_ports() {
        for bad in [
            "[::1]:8080:80",
            "1:2:3:4",
            ":8080:80",
            "-bad-:8080:80",
            "0:80",
            "+80:80",
            "8080:65536",
            "8080",
            "8080:80/udp",
        ] {
            assert!(publish(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn publish_compares_ports_as_numbers() {
        assert_eq!(publish("8080:080").unwrap().container, 80);
    }

    #[test]
    fn volume_takes_an_optional_mode() {
        let read_only = volume_mount("~/config:/etc/app:ro").unwrap();
        assert!(read_only.read_only);
        assert!(!read_only.is_named);
        assert_eq!(read_only.destination, "/etc/app");
        let default = volume_mount("data:/var/data").unwrap();
        assert!(!default.read_only);
        assert!(default.is_named);
    }

    #[test]
    fn volume_destinations_ignore_trailing_slashes() {
        assert_eq!(volume_mount("a:/data/").unwrap().destination, "/data");
        assert_eq!(volume_mount("a:/").unwrap().destination, "/");
    }

    #[test]
    fn volume_rejects_empty_parts_and_unknown_modes() {
        for bad in [
            ":/data",
            "/h:",
            "/h",
            "/h:/c:z",
            "/h:/c:ro:x",
            "a : /b",
            "./rel:/c",
            "../rel:/c",
            "a:rel",
            "bad name:/c",
        ] {
            assert!(volume_mount(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn paths_are_absolute_or_home_relative() {
        for good in ["/app", "/", "~", "~/logs/app.log"] {
            assert!(path("WORKDIR", good).is_ok(), "{good}");
        }
        for bad in ["app", "./app", "../app", "~other/app", ".env.local"] {
            assert!(path("WORKDIR", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn env_keys_are_identifiers() {
        assert!(env("DATABASE_URL=postgres://h/db?x=1").is_ok());
        assert!(env("EMPTY=").is_ok());
        assert!(env("KEY = value").is_err());
        assert!(env("MY KEY=1").is_err());
        assert!(env("NOEQUALS").is_err());
    }

    #[test]
    fn cpus_is_a_plain_decimal() {
        assert_eq!(cpus("0.5").unwrap(), 0.5);
        assert_eq!(cpus("2").unwrap(), 2.0);
        for bad in ["NaN", "inf", "-2", "1e3", ".5", "1.", "abc"] {
            assert!(cpus(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sizes_durations_and_percentages_are_integers_with_a_unit() {
        assert!(memory_size("512M").is_ok());
        assert!(memory_size("1.5G").is_err());
        assert!(memory_size("banana").is_err());
        assert!(memory_size("2G # two gigs").is_err());
        assert!(duration("TIMEOUT_START", "90s").is_ok());
        assert!(duration("TIMEOUT_START", "forever").is_err());
        assert!(duration("TIMEOUT_START", "1h").is_err());
        assert!(percentage("200%").is_ok());
        assert!(percentage("lots").is_err());
    }

    #[test]
    fn dependency_lists_hold_service_names() {
        assert_eq!(
            service_names("AFTER", "db  cache").unwrap(),
            vec!["db", "cache"]
        );
        assert!(service_names("AFTER", "db # optional").is_err());
    }

    #[test]
    fn io_weight_is_range_checked() {
        assert!(io_weight("1").is_ok());
        assert!(io_weight("10000").is_ok());
        assert!(io_weight("0").is_err());
        assert!(io_weight("10001").unwrap_err().contains("1-10000"));
    }
}
