const MAX_SERVICE_NAME_LEN: usize = 63;

pub(crate) fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let starts_well = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    starts_well && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn check_service_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("service name cannot be empty".into());
    }
    if name.len() > MAX_SERVICE_NAME_LEN {
        return Err(format!(
            "service name '{name}' exceeds {MAX_SERVICE_NAME_LEN} character limit ({})",
            name.len()
        ));
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(format!(
            "service name '{name}' must start with a lowercase letter"
        ));
    }
    match name
        .chars()
        .find(|&c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
    {
        Some(c) => Err(format!(
            "service name '{name}' contains invalid character '{c}' (only lowercase alphanumeric and hyphens allowed)"
        )),
        None => Ok(()),
    }
}

pub(crate) fn is_user_name(text: &str) -> bool {
    let mut chars = text.chars();
    let starts_well = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    starts_well && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_allow_uppercase_and_leading_underscore() {
        assert!(is_identifier("DATABASE_URL"));
        assert!(is_identifier("_private"));
        assert!(is_identifier("port2"));
        assert!(!is_identifier("2port"));
        assert!(!is_identifier("my-arg"));
        assert!(!is_identifier(""));
    }

    #[test]
    fn user_names_follow_posix_portable_names() {
        for name in ["postgres", "www-data", "_postgres", "first.last", "1000"] {
            assert!(is_user_name(name), "{name}");
        }
        for name in ["-leading-hyphen", ".hidden", "bad name", "DOMAIN\\user", ""] {
            assert!(!is_user_name(name), "{name}");
        }
    }

    #[test]
    fn service_names_are_lowercase_alphanumeric_and_hyphens() {
        assert!(check_service_name("a-b-c-123").is_ok());
        assert!(check_service_name("Web").is_err());
        assert!(check_service_name("1web").is_err());
        assert!(check_service_name(&"a".repeat(63)).is_ok());
        assert!(check_service_name(&"a".repeat(64)).is_err());
    }
}
