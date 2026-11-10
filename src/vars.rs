//! `${name}` references in directive values.
//!
//! A `${name}` names a declared ARG. `$${` is an escape for a literal `${`.
//! Any other `$` is ordinary text, so shell constructs like `$HOME` or `$$`
//! pass through untouched.

pub(crate) enum Piece<'a> {
    Text(&'a str),
    Var(&'a str),
}

/// Split a value into literal text and `${name}` references, applying the
/// `$${` escape. Fails on an unclosed `${` or an empty `${}`.
pub(crate) fn split(input: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut pieces = Vec::new();
    let mut rest = input;
    while let Some(pos) = rest.find('$') {
        let (before, after) = rest.split_at(pos);
        pieces.push(Piece::Text(before));
        if let Some(tail) = after.strip_prefix("$${") {
            pieces.push(Piece::Text("${"));
            rest = tail;
        } else if let Some(body) = after.strip_prefix("${") {
            let end = body.find('}').ok_or_else(|| {
                format!("unclosed '${{' in \"{input}\" (write $${{ for a literal ${{)")
            })?;
            if end == 0 {
                return Err(format!("empty variable reference '${{}}' in \"{input}\""));
            }
            pieces.push(Piece::Var(&body[..end]));
            rest = &body[end + 1..];
        } else {
            pieces.push(Piece::Text("$"));
            rest = &after[1..];
        }
    }
    pieces.push(Piece::Text(rest));
    Ok(pieces)
}
