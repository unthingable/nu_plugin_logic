use nu_protocol::{Record, Span, Value};

use super::search;
use super::substitution::Substitution;
use super::term::Term;
use super::unify::unify;
use super::LogicEngine;
use crate::store::FactStore;

/// Hand-rolled unification + backtracking engine.
/// No external dependencies beyond nu-protocol.
pub struct NativeEngine;

impl LogicEngine for NativeEngine {
    fn filter(&self, pattern: &Term, input: &[Value], span: Span) -> Vec<Value> {
        input
            .iter()
            .filter_map(|row| {
                let mut sub = Substitution::new();
                if !unify(pattern, row, &mut sub) {
                    return None;
                }
                let bindings = sub.into_bindings();
                if bindings.is_empty() {
                    return Some(row.clone());
                }
                // Rebuild the record with variable bindings merged in
                let Value::Record { val: record, .. } = row else {
                    return Some(row.clone());
                };
                let mut result = Record::new();
                for (col, val) in record.iter() {
                    result.push(col.to_string(), val.clone());
                }
                for (name, value) in bindings {
                    if result.get(&name).is_none() {
                        result.push(name, value);
                    }
                }
                Some(Value::record(result, span))
            })
            .collect()
    }

    fn search(
        &self,
        queries: &[(String, Term)],
        store: &FactStore,
        span: Span,
    ) -> Result<Vec<Value>, String> {
        search::search(queries, store, span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::convert::value_to_pattern;

    fn span() -> Span {
        Span::unknown()
    }

    #[test]
    fn filter_literal() {
        let engine = NativeEngine;
        let rows = vec![
            {
                let mut r = Record::new();
                r.push("type", Value::string("file", span()));
                r.push("name", Value::string("main.rs", span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("type", Value::string("dir", span()));
                r.push("name", Value::string("src", span()));
                Value::record(r, span())
            },
        ];

        let pattern_val = {
            let mut r = Record::new();
            r.push("type", Value::string("file", span()));
            Value::record(r, span())
        };
        let pattern = value_to_pattern(&pattern_val);

        let results = engine.filter(&pattern, &rows, span());
        assert_eq!(results.len(), 1);
        if let Value::Record { val, .. } = &results[0] {
            assert_eq!(val.get("name"), Some(&Value::string("main.rs", span())));
        }
    }

    #[test]
    fn filter_with_variable() {
        let engine = NativeEngine;
        let rows = vec![
            {
                let mut r = Record::new();
                r.push("type", Value::string("file", span()));
                r.push("name", Value::string("main.rs", span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("type", Value::string("dir", span()));
                r.push("name", Value::string("src", span()));
                Value::record(r, span())
            },
        ];

        let pattern_val = {
            let mut r = Record::new();
            r.push("type", Value::string("file", span()));
            r.push("name", Value::string("$f", span()));
            Value::record(r, span())
        };
        let pattern = value_to_pattern(&pattern_val);

        let results = engine.filter(&pattern, &rows, span());
        assert_eq!(results.len(), 1);
        if let Value::Record { val, .. } = &results[0] {
            assert_eq!(val.get("f"), Some(&Value::string("main.rs", span())));
        }
    }

    #[test]
    fn filter_string_pattern() {
        let engine = NativeEngine;
        let rows = vec![
            {
                let mut r = Record::new();
                r.push("type", Value::string("file", span()));
                r.push("name", Value::string("main.rs", span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("type", Value::string("file", span()));
                r.push("name", Value::string("lib.py", span()));
                Value::record(r, span())
            },
        ];

        let pattern_val = {
            let mut r = Record::new();
            r.push("name", Value::string("$stem.rs", span()));
            Value::record(r, span())
        };
        let pattern = value_to_pattern(&pattern_val);

        let results = engine.filter(&pattern, &rows, span());
        assert_eq!(results.len(), 1);
        if let Value::Record { val, .. } = &results[0] {
            assert_eq!(val.get("stem"), Some(&Value::string("main", span())));
        }
    }
}
