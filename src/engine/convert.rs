use nu_protocol::Value;

use super::term::{StringPatternPart, Term};

/// Parse a string like "$stem.rs" into pattern parts.
/// `$`-prefixed word characters become variables, everything else is literal.
pub fn parse_string_pattern(s: &str) -> Vec<StringPatternPart> {
    let mut parts = Vec::new();
    let mut chars = s.chars().peekable();
    let mut literal = String::new();

    while let Some(c) = chars.next() {
        if c == '$' {
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
                // Lone '$' — treat as literal
                literal.push('$');
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

    parts
}

/// Convert a Nushell Value into a pattern Term.
///
/// - `"_"` → Wildcard
/// - `"$name"` (pure variable) → Variable
/// - `"$stem.rs"` (variable + literals) → StringPattern
/// - Record values → Record of sub-patterns
/// - Everything else → Literal
pub fn value_to_pattern(value: &Value) -> Term {
    if let Value::String { val, .. } = value {
        if val == "_" {
            return Term::Wildcard;
        }
        let parts = parse_string_pattern(val);
        return match parts.as_slice() {
            [StringPatternPart::Variable(name)] => Term::Variable(name.clone()),
            _ if parts
                .iter()
                .any(|p| matches!(p, StringPatternPart::Variable(_))) =>
            {
                Term::StringPattern(parts)
            }
            _ => Term::Literal(value.clone()),
        };
    }

    if let Value::Record { val, .. } = value {
        let fields = val
            .iter()
            .map(|(name, v)| (name.clone(), value_to_pattern(v)))
            .collect();
        return Term::Record(fields);
    }

    Term::Literal(value.clone())
}
