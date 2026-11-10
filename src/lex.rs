use crate::error::ParseError;

#[derive(Debug, Clone)]
pub(crate) struct Statement {
    pub line: usize,
    pub keyword: String,
    pub value: Option<String>,
}

pub(crate) fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn is_control(c: char) -> bool {
    (c < ' ' && c != '\t') || ('\u{7f}'..='\u{9f}').contains(&c)
}

pub(crate) fn reject_control_chars(text: &str) -> Result<(), String> {
    match text.chars().find(|&c| is_control(c)) {
        Some(c) => Err(format!(
            "control character U+{:04X} is not allowed",
            c as u32
        )),
        None => Ok(()),
    }
}

pub(crate) fn statements(input: &str) -> Result<Vec<Statement>, Vec<ParseError>> {
    let without_bom = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut statements = Vec::new();
    let mut errors = Vec::new();

    for (index, raw_line) in without_bom.split('\n').enumerate() {
        let line = index + 1;
        let text = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        match statement(line, text) {
            Ok(Some(found)) => statements.push(found),
            Ok(None) => {}
            Err(message) => errors.push(ParseError::new(line, message)),
        }
    }

    if errors.is_empty() {
        Ok(statements)
    } else {
        Err(errors)
    }
}

fn statement(line: usize, text: &str) -> Result<Option<Statement>, String> {
    reject_control_chars(text)?;

    let content = text.trim_start_matches(is_ws);
    let is_blank_or_comment = content.is_empty() || content.starts_with('#');
    if is_blank_or_comment {
        return Ok(None);
    }
    if content.len() != text.len() {
        return Err("directives start at column 0 (remove the indentation)".into());
    }

    let (keyword, value) = split_keyword(text.trim_end_matches(is_ws));
    Ok(Some(Statement {
        line,
        keyword: keyword.to_string(),
        value: value.map(str::to_string),
    }))
}

fn split_keyword(text: &str) -> (&str, Option<&str>) {
    match text.find(is_ws) {
        Some(end) => {
            let value = text[end..].trim_start_matches(is_ws);
            (&text[..end], Some(value).filter(|v| !v.is_empty()))
        }
        None => (text, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexed(input: &str) -> Vec<(usize, String, Option<String>)> {
        statements(input)
            .expect("input should lex")
            .into_iter()
            .map(|s| (s.line, s.keyword, s.value))
            .collect()
    }

    #[test]
    fn blank_and_comment_lines_are_skipped_even_when_indented() {
        let got = lexed("\n   \n# top\n  # indented\nSERVICE a\n");
        assert_eq!(got, vec![(5, "SERVICE".into(), Some("a".into()))]);
    }

    #[test]
    fn bom_crlf_and_trailing_whitespace_are_dropped() {
        let got = lexed("\u{feff}SERVICE a \t\r\nRUN echo hi  \r\n");
        assert_eq!(
            got,
            vec![
                (1, "SERVICE".into(), Some("a".into())),
                (2, "RUN".into(), Some("echo hi".into())),
            ]
        );
    }

    #[test]
    fn final_line_ending_is_optional() {
        assert_eq!(lexed("SERVICE a").len(), 1);
    }

    #[test]
    fn hash_after_a_directive_is_part_of_the_value() {
        let got = lexed("ENV COLOR=#ff0000 # red");
        assert_eq!(got[0].2.as_deref(), Some("COLOR=#ff0000 # red"));
    }

    #[test]
    fn indented_directive_is_rejected() {
        let errors = statements("SERVICE a\n  RUN x\n").unwrap_err();
        assert_eq!(errors[0].line, 2);
        assert!(errors[0].message.contains("column 0"));
    }

    #[test]
    fn control_characters_other_than_tab_are_rejected() {
        let errors = statements("SERVICE a\nENV A=red\u{1b}[31m\n").unwrap_err();
        assert_eq!(errors[0].line, 2);
        assert!(errors[0].message.contains("U+001B"));
        assert!(statements("SERVICE a\nRUN printf 'a\tb'\n").is_ok());
    }

    #[test]
    fn lone_carriage_return_is_a_control_character() {
        assert!(statements("SERVICE a\rRUN x\n").is_err());
    }

    #[test]
    fn keyword_alone_has_no_value() {
        assert_eq!(lexed("SERVICE")[0].2, None);
    }
}
