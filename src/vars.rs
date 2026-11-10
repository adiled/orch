use std::collections::BTreeMap;

pub(crate) enum Piece<'a> {
    Text(&'a str),
    Var(&'a str),
}

pub(crate) fn pieces(input: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut pieces = Vec::new();
    let mut rest = input;
    while let Some(dollar) = rest.find('$') {
        let (before, from_dollar) = rest.split_at(dollar);
        pieces.push(Piece::Text(before));
        if let Some(after_escape) = from_dollar.strip_prefix("$${") {
            pieces.push(Piece::Text("${"));
            rest = after_escape;
        } else if let Some(reference) = from_dollar.strip_prefix("${") {
            let close = reference.find('}').ok_or_else(|| {
                format!("unclosed '${{' in \"{input}\" (write $${{ for a literal ${{)")
            })?;
            if close == 0 {
                return Err(format!("empty variable reference '${{}}' in \"{input}\""));
            }
            pieces.push(Piece::Var(&reference[..close]));
            rest = &reference[close + 1..];
        } else {
            pieces.push(Piece::Text("$"));
            rest = &from_dollar[1..];
        }
    }
    pieces.push(Piece::Text(rest));
    Ok(pieces)
}

pub(crate) fn expand(input: &str, args: &BTreeMap<String, String>) -> Result<String, Vec<String>> {
    let mut expanded = String::with_capacity(input.len());
    let mut undefined = Vec::new();
    for piece in pieces(input).map_err(|message| vec![message])? {
        match piece {
            Piece::Text(text) => expanded.push_str(text),
            Piece::Var(name) => match args.get(name) {
                Some(value) => expanded.push_str(value),
                None => undefined.push(format!(
                    "undefined variable '${{{name}}}' (C5): declare it with ARG, or write $${{ for a literal ${{"
                )),
            },
        }
    }
    if undefined.is_empty() {
        Ok(expanded)
    } else {
        Err(undefined)
    }
}
