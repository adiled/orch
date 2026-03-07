use std::fmt;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
    pub file: Option<String>,
}

impl ParseError {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        ParseError {
            line,
            message: message.into(),
            file: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}, line {}: {}", file, self.line, self.message),
            None => write!(f, "line {}: {}", self.line, self.message),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub service: String,
    pub message: String,
    pub file: Option<String>,
}

impl ValidationError {
    pub fn new(service: impl Into<String>, message: impl Into<String>) -> Self {
        ValidationError {
            service: service.into(),
            message: message.into(),
            file: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}: service '{}': {}", file, self.service, self.message),
            None => write!(f, "service '{}': {}", self.service, self.message),
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug)]
pub enum OrchError {
    Parse(ParseError),
    Validation(ValidationError),
    Io(std::io::Error),
}

impl fmt::Display for OrchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrchError::Parse(e) => write!(f, "parse error: {}", e),
            OrchError::Validation(e) => write!(f, "validation error: {}", e),
            OrchError::Io(e) => write!(f, "io error: {}", e),
        }
    }
}

impl std::error::Error for OrchError {}

impl From<ParseError> for OrchError {
    fn from(e: ParseError) -> Self {
        OrchError::Parse(e)
    }
}

impl From<ValidationError> for OrchError {
    fn from(e: ValidationError) -> Self {
        OrchError::Validation(e)
    }
}

impl From<std::io::Error> for OrchError {
    fn from(e: std::io::Error) -> Self {
        OrchError::Io(e)
    }
}
