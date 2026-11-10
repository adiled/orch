use std::collections::HashMap;

use crate::error::ParseError;
use crate::lex::Statement;
use crate::names::{check_service_name, is_identifier};
use crate::types::{ORCH_VERSION, ServiceMode};
use crate::vars::{self, Piece};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Directive {
    From,
    Run,
    Entrypoint,
    Cmd,
    Publish,
    Volume,
    User,
    Stop,
    Reload,
    Workdir,
    Env,
    EnvFile,
    Requires,
    After,
    Healthcheck,
    ReadinessTimeout,
    Oneshot,
    Disabled,
    Recreate,
    Restart,
    RestartDelay,
    StartLimitBurst,
    StartLimitInterval,
    TimeoutStart,
    TimeoutStop,
    Memory,
    Cpus,
    CpuQuota,
    LimitNofile,
    LimitNproc,
    TasksMax,
    IoWeight,
    Stdout,
    Stderr,
    Clear,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    Mode,
    ContainerOnly,
    HostOnly,
    Anywhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListField {
    Env,
    EnvFile,
    Publish,
    Volume,
    Requires,
    After,
}

impl Directive {
    const ALL: [Directive; 35] = [
        Directive::From,
        Directive::Run,
        Directive::Entrypoint,
        Directive::Cmd,
        Directive::Publish,
        Directive::Volume,
        Directive::User,
        Directive::Stop,
        Directive::Reload,
        Directive::Workdir,
        Directive::Env,
        Directive::EnvFile,
        Directive::Requires,
        Directive::After,
        Directive::Healthcheck,
        Directive::ReadinessTimeout,
        Directive::Oneshot,
        Directive::Disabled,
        Directive::Recreate,
        Directive::Restart,
        Directive::RestartDelay,
        Directive::StartLimitBurst,
        Directive::StartLimitInterval,
        Directive::TimeoutStart,
        Directive::TimeoutStop,
        Directive::Memory,
        Directive::Cpus,
        Directive::CpuQuota,
        Directive::LimitNofile,
        Directive::LimitNproc,
        Directive::TasksMax,
        Directive::IoWeight,
        Directive::Stdout,
        Directive::Stderr,
        Directive::Clear,
    ];

    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Directive::From => "FROM",
            Directive::Run => "RUN",
            Directive::Entrypoint => "ENTRYPOINT",
            Directive::Cmd => "CMD",
            Directive::Publish => "PUBLISH",
            Directive::Volume => "VOLUME",
            Directive::User => "USER",
            Directive::Stop => "STOP",
            Directive::Reload => "RELOAD",
            Directive::Workdir => "WORKDIR",
            Directive::Env => "ENV",
            Directive::EnvFile => "ENV_FILE",
            Directive::Requires => "REQUIRES",
            Directive::After => "AFTER",
            Directive::Healthcheck => "HEALTHCHECK",
            Directive::ReadinessTimeout => "READINESS_TIMEOUT",
            Directive::Oneshot => "ONESHOT",
            Directive::Disabled => "DISABLED",
            Directive::Recreate => "RECREATE",
            Directive::Restart => "RESTART",
            Directive::RestartDelay => "RESTART_DELAY",
            Directive::StartLimitBurst => "START_LIMIT_BURST",
            Directive::StartLimitInterval => "START_LIMIT_INTERVAL",
            Directive::TimeoutStart => "TIMEOUT_START",
            Directive::TimeoutStop => "TIMEOUT_STOP",
            Directive::Memory => "MEMORY",
            Directive::Cpus => "CPUS",
            Directive::CpuQuota => "CPU_QUOTA",
            Directive::LimitNofile => "LIMIT_NOFILE",
            Directive::LimitNproc => "LIMIT_NPROC",
            Directive::TasksMax => "TASKS_MAX",
            Directive::IoWeight => "IO_WEIGHT",
            Directive::Stdout => "STDOUT",
            Directive::Stderr => "STDERR",
            Directive::Clear => "CLEAR",
        }
    }

    pub(crate) fn from_keyword(keyword: &str) -> Option<Directive> {
        Self::ALL.into_iter().find(|d| d.keyword() == keyword)
    }

    pub(crate) fn placement(self) -> Placement {
        match self {
            Directive::From | Directive::Run => Placement::Mode,
            Directive::Entrypoint | Directive::Cmd | Directive::Publish | Directive::Volume => {
                Placement::ContainerOnly
            }
            Directive::User | Directive::Stop | Directive::Reload => Placement::HostOnly,
            _ => Placement::Anywhere,
        }
    }

    pub(crate) fn mode(self) -> Option<ServiceMode> {
        match self {
            Directive::From => Some(ServiceMode::Container),
            Directive::Run => Some(ServiceMode::Host),
            _ => None,
        }
    }

    pub(crate) fn list_field(self) -> Option<ListField> {
        match self {
            Directive::Env => Some(ListField::Env),
            Directive::EnvFile => Some(ListField::EnvFile),
            Directive::Publish => Some(ListField::Publish),
            Directive::Volume => Some(ListField::Volume),
            Directive::Requires => Some(ListField::Requires),
            Directive::After => Some(ListField::After),
            _ => None,
        }
    }

    pub(crate) fn is_scalar(self) -> bool {
        self.mode().is_none() && self.list_field().is_none() && self != Directive::Clear
    }
}

impl Placement {
    pub(crate) fn excludes(self, mode: ServiceMode) -> bool {
        matches!(
            (self, mode),
            (Placement::ContainerOnly, ServiceMode::Host)
                | (Placement::HostOnly, ServiceMode::Container)
        )
    }

    pub(crate) fn describe(self) -> &'static str {
        match self {
            Placement::ContainerOnly => "container-only",
            Placement::HostOnly => "host-only",
            Placement::Mode | Placement::Anywhere => "allowed in any block",
        }
    }
}

pub(crate) fn mode_keyword(mode: ServiceMode) -> &'static str {
    match mode {
        ServiceMode::Container => "FROM",
        ServiceMode::Host => "RUN",
    }
}

pub(crate) struct Entry {
    pub line: usize,
    pub directive: Directive,
    pub value: String,
}

pub(crate) struct Block {
    pub service: String,
    pub mode: Option<(ServiceMode, usize)>,
    pub entries: Vec<Entry>,
}

pub(crate) struct ArgDecl {
    pub name: String,
    pub default: String,
}

#[derive(Default)]
pub(crate) struct Document {
    pub args: Vec<ArgDecl>,
    pub blocks: Vec<Block>,
}

pub(crate) fn document(statements: Vec<Statement>) -> Result<Document, Vec<ParseError>> {
    let mut builder = DocumentBuilder::default();
    for (position, statement) in statements.into_iter().enumerate() {
        let line = statement.line;
        if let Err(message) = builder.add(position, statement) {
            builder.errors.push(ParseError::new(line, message));
        }
    }
    builder.finish()
}

#[derive(Default)]
struct DocumentBuilder {
    document: Document,
    first_declared_at: HashMap<String, usize>,
    errors: Vec<ParseError>,
}

impl DocumentBuilder {
    fn add(&mut self, position: usize, statement: Statement) -> Result<(), String> {
        let Statement {
            line,
            keyword,
            value,
        } = statement;
        match keyword.as_str() {
            "ORCH_VERSION" => check_version(position, value),
            "ARG" => {
                self.document.args.push(arg_decl(value)?);
                Ok(())
            }
            "SERVICE" => self.open_block(line, value),
            other => match Directive::from_keyword(other) {
                Some(directive) => self.add_entry(line, directive, value),
                None => Err(format!("unknown directive '{other}'")),
            },
        }
    }

    fn open_block(&mut self, line: usize, name: Option<String>) -> Result<(), String> {
        let service = name.unwrap_or_default();
        self.document.blocks.push(Block {
            service: service.clone(),
            mode: None,
            entries: Vec::new(),
        });
        if service.is_empty() {
            return Err("SERVICE requires a name".into());
        }
        check_service_name(&service)?;
        match self.first_declared_at.get(&service) {
            Some(first) => Err(format!(
                "duplicate service name '{service}' (first defined at line {first})"
            )),
            None => {
                self.first_declared_at.insert(service, line);
                Ok(())
            }
        }
    }

    fn add_entry(
        &mut self,
        line: usize,
        directive: Directive,
        value: Option<String>,
    ) -> Result<(), String> {
        let keyword = directive.keyword();
        let block = self
            .document
            .blocks
            .last_mut()
            .ok_or_else(|| format!("directive '{keyword}' outside of SERVICE block"))?;
        let value = value.ok_or_else(|| format!("{keyword} requires a value"))?;

        if directive == Directive::Clear {
            check_clear_target(&value)?;
        } else {
            vars::pieces(&value)?;
        }

        if let Some(mode) = directive.mode() {
            if let Some((_, mode_line)) = block.mode {
                return Err(format!(
                    "a service block sets at most one of FROM and RUN (already set at line {mode_line})"
                ));
            }
            block.mode = Some((mode, line));
        }

        block.entries.push(Entry {
            line,
            directive,
            value,
        });
        Ok(())
    }

    fn finish(mut self) -> Result<Document, Vec<ParseError>> {
        for block in &self.document.blocks {
            self.errors.extend(misplaced_entries(block));
        }
        self.errors.sort_by_key(|e| e.line);
        if self.errors.is_empty() {
            Ok(self.document)
        } else {
            Err(self.errors)
        }
    }
}

fn check_version(position: usize, value: Option<String>) -> Result<(), String> {
    if position != 0 {
        return Err("ORCH_VERSION must be the first statement in the file".into());
    }
    match value {
        None => Err("ORCH_VERSION requires a version value".into()),
        Some(version) if version != ORCH_VERSION => Err(format!(
            "unsupported Orchfile version '{version}' (this parser supports {ORCH_VERSION})"
        )),
        Some(_) => Ok(()),
    }
}

fn arg_decl(value: Option<String>) -> Result<ArgDecl, String> {
    let value = value.ok_or("ARG requires name=value")?;
    let (name, default) = value
        .split_once('=')
        .ok_or_else(|| format!("invalid ARG format '{value}', expected name=value"))?;
    if name.is_empty() {
        return Err("ARG name cannot be empty".into());
    }
    if !is_identifier(name) {
        return Err(format!(
            "invalid ARG name '{name}': use letters, digits and '_', not starting with a digit"
        ));
    }
    Ok(ArgDecl {
        name: name.to_string(),
        default: literal_default(name, default)?,
    })
}

fn literal_default(name: &str, default: &str) -> Result<String, String> {
    let mut literal = String::with_capacity(default.len());
    for piece in vars::pieces(default)? {
        match piece {
            Piece::Text(text) => literal.push_str(text),
            Piece::Var(var) => {
                return Err(format!(
                    "ARG {name}: default cannot reference '${{{var}}}' (ARG defaults are literal; write $${{ for a literal ${{)"
                ));
            }
        }
    }
    Ok(literal)
}

fn check_clear_target(target: &str) -> Result<(), String> {
    match Directive::from_keyword(target) {
        Some(directive) if directive.list_field().is_some() => Ok(()),
        Some(_) => Err(format!(
            "CLEAR cannot target scalar directive '{target}' (scalars override by last-wins)"
        )),
        None => Err(format!("CLEAR targets unknown directive '{target}'")),
    }
}

fn misplaced_entries(block: &Block) -> Vec<ParseError> {
    let Some((mode, mode_line)) = block.mode else {
        return Vec::new();
    };
    block
        .entries
        .iter()
        .filter(|entry| entry.directive.placement().excludes(mode))
        .map(|entry| {
            ParseError::new(
                entry.line,
                format!(
                    "{} is {}, but this block sets {} at line {mode_line}",
                    entry.directive.keyword(),
                    entry.directive.placement().describe(),
                    mode_keyword(mode),
                ),
            )
        })
        .collect()
}
