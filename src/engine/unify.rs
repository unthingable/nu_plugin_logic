use nu_protocol::{Span, Value};

use super::substitution::Substitution;
use super::term::{StringPatternPart, Term};

/// Attempt to unify a pattern against a concrete Nushell value,
/// accumulating variable bindings in `sub`.
///
/// Returns:
/// - `Ok(true)` — pattern matches the value
/// - `Ok(false)` — pattern could apply but this value doesn't match
///   (e.g., literal mismatch, variable already bound to a different value)
/// - `Err(msg)` — structural problem: the pattern *cannot* apply to this data
///   (e.g., record pattern references a field that doesn't exist, string pattern
///   applied to a non-string value)
///
/// On failure, `sub` may contain partial bindings and should be discarded.
pub fn unify(pattern: &Term, value: &Value, sub: &mut Substitution) -> Result<bool, String> {
    match pattern {
        Term::Literal(lit) => Ok(lit == value),

        Term::Variable(name) => bind_or_check_value(name, value, sub),

        Term::Record(fields) => {
            let Value::Record { val: record, .. } = value else {
                return Ok(false);
            };
            for (field_name, field_pattern) in fields {
                match record.get(field_name) {
                    Some(field_value) => {
                        if !unify(field_pattern, field_value, sub)? {
                            return Ok(false);
                        }
                    }
                    None => {
                        let available: Vec<&str> =
                            record.columns().map(|c| c.as_str()).collect();
                        return Err(format!(
                            "pattern field '{}' not found in record (available: {})",
                            field_name,
                            available.join(", ")
                        ));
                    }
                }
            }
            Ok(true)
        }

        Term::StringPattern(parts) => {
            let Value::String { val: s, .. } = value else {
                return Err(format!(
                    "string pattern cannot match non-string value (got {})",
                    value.get_type()
                ));
            };
            unify_string_pattern(parts, s, sub)
        }
    }
}

/// Bind-or-check for a full Value (used by Term::Variable).
fn bind_or_check_value(
    name: &str,
    value: &Value,
    sub: &mut Substitution,
) -> Result<bool, String> {
    if let Some(bound) = sub.get(name) {
        Ok(bound == value)
    } else {
        sub.bind(name.to_string(), value.clone());
        Ok(true)
    }
}

/// Recursive string pattern matching with support for multiple variables.
///
/// Literals must match exactly (consumed left-to-right). Variables bind to
/// the text between literals. When a variable is followed (eventually) by a
/// literal, the leftmost occurrence of that literal delimits the variable's
/// capture. A trailing variable captures the remainder of the string.
fn unify_string_pattern(
    parts: &[StringPatternPart],
    s: &str,
    sub: &mut Substitution,
) -> Result<bool, String> {
    match parts {
        [] => Ok(s.is_empty()),

        [StringPatternPart::Literal(lit), rest @ ..] => match s.strip_prefix(lit.as_str()) {
            Some(remaining) => unify_string_pattern(rest, remaining, sub),
            None => Ok(false),
        },

        [StringPatternPart::Variable(name)] => {
            // Last part — variable captures the rest of the string
            bind_or_check(name, s, sub)
        }

        [StringPatternPart::Variable(name), rest @ ..] => {
            // Find the next literal to use as a delimiter
            let next_lit = rest.iter().find_map(|p| match p {
                StringPatternPart::Literal(lit) => Some(lit.as_str()),
                _ => None,
            });
            match next_lit {
                Some(lit) => match s.find(lit) {
                    Some(pos) => {
                        if !bind_or_check(name, &s[..pos], sub)? {
                            return Ok(false);
                        }
                        unify_string_pattern(rest, &s[pos..], sub)
                    }
                    None => Ok(false),
                },
                None => {
                    // No more literals — adjacent trailing variables.
                    // Give this one empty string; last variable gets everything.
                    if !bind_or_check(name, "", sub)? {
                        return Ok(false);
                    }
                    unify_string_pattern(rest, s, sub)
                }
            }
        }
    }
}

fn bind_or_check(name: &str, value: &str, sub: &mut Substitution) -> Result<bool, String> {
    let val = Value::string(value, Span::unknown());
    if let Some(bound) = sub.get(name) {
        Ok(*bound == val)
    } else {
        sub.bind(name.to_string(), val);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::convert::value_to_pattern;
    use nu_protocol::{Record, Span};

    fn span() -> Span {
        Span::unknown()
    }

    #[test]
    fn literal_match() {
        let pattern = Term::Literal(Value::string("file", span()));
        let value = Value::string("file", span());
        let mut sub = Substitution::new();
        assert!(unify(&pattern, &value, &mut sub).unwrap());
    }

    #[test]
    fn literal_mismatch() {
        let pattern = Term::Literal(Value::string("file", span()));
        let value = Value::string("dir", span());
        let mut sub = Substitution::new();
        assert!(!unify(&pattern, &value, &mut sub).unwrap());
    }

    #[test]
    fn variable_binds() {
        let pattern = Term::Variable("x".into());
        let value = Value::string("hello", span());
        let mut sub = Substitution::new();
        assert!(unify(&pattern, &value, &mut sub).unwrap());
        assert_eq!(sub.get("x"), Some(&Value::string("hello", span())));
    }

    #[test]
    fn variable_consistency() {
        let pattern = Term::Variable("x".into());
        let mut sub = Substitution::new();
        sub.bind("x".into(), Value::int(42, span()));
        assert!(unify(&pattern, &Value::int(42, span()), &mut sub).unwrap());
        assert!(!unify(&pattern, &Value::int(99, span()), &mut sub).unwrap());
    }

    #[test]
    fn record_pattern() {
        let mut rec = Record::new();
        rec.push("type", Value::string("file", span()));
        rec.push("name", Value::string("main.rs", span()));
        rec.push("size", Value::int(1024, span()));
        let value = Value::record(rec, span());

        let pattern_val = {
            let mut rec = Record::new();
            rec.push("type", Value::string("file", span()));
            rec.push("name", Value::string("&f", span()));
            Value::record(rec, span())
        };
        let pattern = value_to_pattern(&pattern_val).unwrap();

        let mut sub = Substitution::new();
        assert!(unify(&pattern, &value, &mut sub).unwrap());
        assert_eq!(sub.get("f"), Some(&Value::string("main.rs", span())));
    }

    #[test]
    fn string_pattern_suffix() {
        let parts = vec![
            StringPatternPart::Variable("stem".into()),
            StringPatternPart::Literal(".rs".into()),
        ];
        let mut sub = Substitution::new();
        assert!(unify_string_pattern(&parts, "main.rs", &mut sub).unwrap());
        assert_eq!(sub.get("stem"), Some(&Value::string("main", span())));
    }

    #[test]
    fn string_pattern_prefix() {
        let parts = vec![
            StringPatternPart::Literal("test_".into()),
            StringPatternPart::Variable("name".into()),
        ];
        let mut sub = Substitution::new();
        assert!(unify_string_pattern(&parts, "test_foo", &mut sub).unwrap());
        assert_eq!(sub.get("name"), Some(&Value::string("foo", span())));
    }

    #[test]
    fn string_pattern_no_match() {
        let parts = vec![
            StringPatternPart::Variable("stem".into()),
            StringPatternPart::Literal(".rs".into()),
        ];
        let mut sub = Substitution::new();
        assert!(!unify_string_pattern(&parts, "main.py", &mut sub).unwrap());
    }

    #[test]
    fn string_pattern_two_vars() {
        let parts = vec![
            StringPatternPart::Variable("name".into()),
            StringPatternPart::Literal(".".into()),
            StringPatternPart::Variable("ext".into()),
        ];
        let mut sub = Substitution::new();
        assert!(unify_string_pattern(&parts, "main.rs", &mut sub).unwrap());
        assert_eq!(sub.get("name"), Some(&Value::string("main", span())));
        assert_eq!(sub.get("ext"), Some(&Value::string("rs", span())));
    }

    #[test]
    fn string_pattern_three_vars() {
        // $a.$b.$c against "x.y.z"
        let parts = vec![
            StringPatternPart::Variable("a".into()),
            StringPatternPart::Literal(".".into()),
            StringPatternPart::Variable("b".into()),
            StringPatternPart::Literal(".".into()),
            StringPatternPart::Variable("c".into()),
        ];
        let mut sub = Substitution::new();
        assert!(unify_string_pattern(&parts, "x.y.z", &mut sub).unwrap());
        assert_eq!(sub.get("a"), Some(&Value::string("x", span())));
        assert_eq!(sub.get("b"), Some(&Value::string("y", span())));
        assert_eq!(sub.get("c"), Some(&Value::string("z", span())));
    }

    #[test]
    fn string_pattern_last_var_greedy() {
        // $a.$b against "x.y.z" — b captures "y.z"
        let parts = vec![
            StringPatternPart::Variable("a".into()),
            StringPatternPart::Literal(".".into()),
            StringPatternPart::Variable("b".into()),
        ];
        let mut sub = Substitution::new();
        assert!(unify_string_pattern(&parts, "x.y.z", &mut sub).unwrap());
        assert_eq!(sub.get("a"), Some(&Value::string("x", span())));
        assert_eq!(sub.get("b"), Some(&Value::string("y.z", span())));
    }

    #[test]
    fn nested_record_pattern() {
        let mut inner = Record::new();
        inner.push("port", Value::int(8080, span()));
        inner.push("host", Value::string("localhost", span()));
        let mut outer = Record::new();
        outer.push("name", Value::string("web", span()));
        outer.push("config", Value::record(inner, span()));
        let value = Value::record(outer, span());

        let pattern_val = {
            let mut inner = Record::new();
            inner.push("port", Value::string("&port", span()));
            let mut outer = Record::new();
            outer.push("config", Value::record(inner, span()));
            Value::record(outer, span())
        };
        let pattern = value_to_pattern(&pattern_val).unwrap();

        let mut sub = Substitution::new();
        assert!(unify(&pattern, &value, &mut sub).unwrap());
        assert_eq!(sub.get("port"), Some(&Value::int(8080, span())));
    }
}
