use nu_protocol::Value;

use super::term::{StringPatternPart, Term};

/// Parse a string like "&stem.rs" into pattern parts.
/// `&`-prefixed word characters become logic variables, everything else is literal.
///
/// Returns `Err` if the pattern contains `&&` (a common mistake — use `&` for variables).
pub fn parse_string_pattern(s: &str) -> Result<Vec<StringPatternPart>, String> {
    let mut parts = Vec::new();
    let mut chars = s.chars().peekable();
    let mut literal = String::new();

    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                return Err(
                    "unexpected '&&' in pattern — use a single '&' for variables".to_string(),
                );
            }
            let mut var_name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' {
                    var_name.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if var_name.is_empty() {
                // Lone '&' — treat as literal
                literal.push('&');
            } else {
                if !literal.is_empty() {
                    parts.push(StringPatternPart::Literal(std::mem::take(&mut literal)));
                }
                parts.push(StringPatternPart::Variable(var_name));
            }
        } else {
            literal.push(c);
        }
    }

    if !literal.is_empty() {
        parts.push(StringPatternPart::Literal(literal));
    }

    Ok(parts)
}

/// Convert a Nushell Value into a pattern Term.
///
/// - `"&name"` (pure variable) → Variable
/// - `"&stem.rs"` (variable + literals) → StringPattern
/// - Record values → Record of sub-patterns
/// - List [k v k v] with string keys → Record of sub-patterns
/// - Everything else → Literal
pub fn value_to_pattern(value: &Value) -> Result<Term, String> {
    if let Value::String { val, .. } = value {
        let parts = parse_string_pattern(val)?;
        return Ok(match parts.as_slice() {
            [StringPatternPart::Variable(name)] => Term::Variable(name.clone()),
            _ if parts
                .iter()
                .any(|p| matches!(p, StringPatternPart::Variable(_))) =>
            {
                Term::StringPattern(parts)
            }
            _ => Term::Literal(value.clone()),
        });
    }

    if let Value::Record { val, .. } = value {
        let fields = val
            .iter()
            .map(|(name, v)| Ok((name.clone(), value_to_pattern(v)?)))
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(Term::Record(fields));
    }

    // List as record: [k v k v] → {k: v, k: v}
    // Even-length list with plain string keys (not @-prefixed) at even positions.
    if let Value::List { vals, .. } = value
        && is_kv_list(vals) {
            let fields = vals
                .chunks(2)
                .map(|pair| {
                    let key = match &pair[0] {
                        Value::String { val, .. } => val.clone(),
                        _ => unreachable!(),
                    };
                    Ok((key, value_to_pattern(&pair[1])?))
                })
                .collect::<Result<Vec<_>, String>>()?;
            return Ok(Term::Record(fields));
        }

    Ok(Term::Literal(value.clone()))
}

/// Check if a list looks like key-value pairs: even length, plain string keys
/// (not @-prefixed, which are fact references).
pub fn is_kv_list(vals: &[Value]) -> bool {
    vals.len() >= 2
        && vals.len().is_multiple_of(2)
        && vals.chunks(2).all(|pair| {
            matches!(&pair[0], Value::String { val, .. } if !val.starts_with('@'))
        })
}
