use nu_protocol::Value;

use super::term::{StringPatternPart, Term};

/// Parse a list of Values as a pattern using left-to-right rules:
/// 1. `&var` → standalone extraction (field=var, pattern=Variable(var)), advance 1
/// 2. `key:val` (colon not at end) → split on first `:`, advance 1
/// 3. `key:` (trailing colon) → key, next element is value, advance 2
/// 4. bare `key` → pair with next element as value, advance 2
/// 5. bare `key` at end → error suggesting `&key`
///
/// Returns Ok(None) if the list doesn't look like a pattern (non-string in key position, @-prefixed element).
/// Returns Ok(Some(fields)) on success.
/// Returns Err on syntax errors (trailing key, invalid standalone var).
pub fn parse_pattern_list(vals: &[Value]) -> Result<Option<Vec<(String, Term)>>, String> {
    if vals.is_empty() {
        return Ok(None);
    }

    let mut fields = Vec::new();
    let mut i = 0;

    while i < vals.len() {
        // Key position: must be a string
        let Value::String { val: s, .. } = &vals[i] else {
            return Ok(None);
        };

        // @-prefixed → data source reference, not a pattern
        if s.starts_with('@') {
            return Ok(None);
        }

        // Rule 1: Standalone extraction `&var`
        if let Some(var_name) = s.strip_prefix('&') {
            if var_name.is_empty() {
                return Err("standalone '&' is not a valid variable — use '&name'".to_string());
            }
            if !var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(format!(
                    "standalone '{}' is not a simple variable name — \
                     standalone extraction requires a plain variable like '&name'",
                    s
                ));
            }
            fields.push((var_name.to_string(), Term::Variable(var_name.to_string())));
            i += 1;
            continue;
        }

        // Check for colon in the string
        if let Some(colon_pos) = s.find(':') {
            let key = &s[..colon_pos];
            let after_colon = &s[colon_pos + 1..];

            if key.is_empty() {
                return Err(format!(
                    "empty key before ':' in '{s}' — keys must be non-empty"
                ));
            }

            if after_colon.is_empty() {
                // Rule 3: Trailing colon `key:` — next element is value
                if i + 1 >= vals.len() {
                    return Err(format!(
                        "'{s}' at end of pattern list has no value — \
                         add a value after it or use '&{key}' for extraction"
                    ));
                }
                let pat = value_to_pattern(&vals[i + 1])?;
                fields.push((key.to_string(), pat));
                i += 2;
            } else {
                // Rule 2: Infix colon `key:value`
                let pat = value_to_pattern(&Value::string(after_colon, vals[i].span()))?;
                fields.push((key.to_string(), pat));
                i += 1;
            }
            continue;
        }

        // Rule 4/5: Bare key
        if i + 1 >= vals.len() {
            // Rule 5: Trailing bare key — error
            return Err(format!(
                "'{s}' at end of pattern list has no value — \
                 did you mean '&{s}' to extract it?"
            ));
        }

        // Rule 4: Bare pair
        let pat = value_to_pattern(&vals[i + 1])?;
        fields.push((s.clone(), pat));
        i += 2;
    }

    Ok(Some(fields))
}

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

    // List as pattern: flexible left-to-right parsing
    if let Value::List { vals, .. } = value
        && let Some(fields) = parse_pattern_list(vals)?
    {
        return Ok(Term::Record(fields));
    }

    Ok(Term::Literal(value.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nu_protocol::Span;

    fn span() -> Span {
        Span::unknown()
    }

    fn sv(s: &str) -> Value {
        Value::string(s, span())
    }

    // Test 1: Two standalone extractions
    #[test]
    fn standalone_extractions() {
        let vals = vec![sv("&name"), sv("&size")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "name");
        assert!(matches!(&result[0].1, Term::Variable(v) if v == "name"));
        assert_eq!(result[1].0, "size");
        assert!(matches!(&result[1].1, Term::Variable(v) if v == "size"));
    }

    // Test 2: Infix pair + standalone
    #[test]
    fn infix_colon_and_standalone() {
        let vals = vec![sv("type:file"), sv("&name")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "type");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &sv("file")));
        assert_eq!(result[1].0, "name");
        assert!(matches!(&result[1].1, Term::Variable(v) if v == "name"));
    }

    // Test 3: Trailing colon pair + standalone
    #[test]
    fn trailing_colon_and_standalone() {
        let vals = vec![sv("type:"), sv("file"), sv("&name")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "type");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &sv("file")));
        assert_eq!(result[1].0, "name");
        assert!(matches!(&result[1].1, Term::Variable(v) if v == "name"));
    }

    // Test 4: Bare pair + standalone
    #[test]
    fn bare_pair_and_standalone() {
        let vals = vec![sv("type"), sv("file"), sv("&name")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "type");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &sv("file")));
        assert_eq!(result[1].0, "name");
        assert!(matches!(&result[1].1, Term::Variable(v) if v == "name"));
    }

    // Test 5: Two bare pairs (backward compat with old [k v k v] syntax)
    #[test]
    fn two_bare_pairs_backward_compat() {
        let vals = vec![sv("type"), sv("file"), sv("name"), sv("&name")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "type");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &sv("file")));
        assert_eq!(result[1].0, "name");
        assert!(matches!(&result[1].1, Term::Variable(v) if v == "name"));
    }

    // Test 6: Infix pair with string pattern value
    #[test]
    fn infix_colon_with_string_pattern() {
        let vals = vec![sv("name:&stem.&ext")];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "name");
        assert!(matches!(&result[0].1, Term::StringPattern(_)));
    }

    // Test 7: Trailing bare key error
    #[test]
    fn trailing_bare_key_error() {
        let vals = vec![sv("type")];
        let result = parse_pattern_list(&vals);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("&type"), "error should suggest &type: {err}");
    }

    // Test 8: Invalid standalone `&`
    #[test]
    fn lone_ampersand_error() {
        let vals = vec![sv("&")];
        let result = parse_pattern_list(&vals);
        assert!(result.is_err());
    }

    // Test 9: Invalid standalone `&stem.&ext` (not simple variable)
    #[test]
    fn non_simple_standalone_error() {
        let vals = vec![sv("&stem.&ext")];
        let result = parse_pattern_list(&vals);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("not a simple variable"),
            "error should mention non-simple variable: {err}"
        );
    }

    // Test 10: Non-string in key position returns None
    #[test]
    fn non_string_key_returns_none() {
        let vals = vec![Value::int(42, span()), sv("file")];
        let result = parse_pattern_list(&vals).unwrap();
        assert!(result.is_none());
    }

    // Test 11: @-prefixed returns None
    #[test]
    fn at_prefixed_returns_none() {
        let vals = vec![sv("@foo")];
        let result = parse_pattern_list(&vals).unwrap();
        assert!(result.is_none());
    }

    // Test: Empty list returns None
    #[test]
    fn empty_list_returns_none() {
        let vals: Vec<Value> = vec![];
        let result = parse_pattern_list(&vals).unwrap();
        assert!(result.is_none());
    }

    // Test: value_to_pattern with list uses parse_pattern_list
    #[test]
    fn value_to_pattern_list_standalone() {
        let list = Value::list(vec![sv("&name"), sv("&size")], span());
        let term = value_to_pattern(&list).unwrap();
        match term {
            Term::Record(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "name");
                assert_eq!(fields[1].0, "size");
            }
            _ => panic!("expected Record, got {term:?}"),
        }
    }

    // Test: value_to_pattern with infix colon
    #[test]
    fn value_to_pattern_list_infix() {
        let list = Value::list(vec![sv("type:file"), sv("&name")], span());
        let term = value_to_pattern(&list).unwrap();
        match term {
            Term::Record(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "type");
                assert_eq!(fields[1].0, "name");
            }
            _ => panic!("expected Record, got {term:?}"),
        }
    }

    // Test: trailing colon at end of list is an error
    #[test]
    fn trailing_colon_at_end_error() {
        let vals = vec![sv("type:")];
        let result = parse_pattern_list(&vals);
        assert!(result.is_err());
    }

    // Test: infix colon with empty key is an error
    #[test]
    fn empty_key_before_colon_error() {
        let vals = vec![sv(":value")];
        let result = parse_pattern_list(&vals);
        assert!(result.is_err());
    }

    // Test: Non-string value element works (e.g., int as value in bare pair)
    #[test]
    fn bare_pair_with_int_value() {
        let vals = vec![sv("port"), Value::int(8080, span())];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "port");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &Value::int(8080, span())));
    }

    // Test: trailing colon with non-string value
    #[test]
    fn trailing_colon_with_int_value() {
        let vals = vec![sv("port:"), Value::int(8080, span())];
        let result = parse_pattern_list(&vals).unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "port");
        assert!(matches!(&result[0].1, Term::Literal(v) if v == &Value::int(8080, span())));
    }
}
