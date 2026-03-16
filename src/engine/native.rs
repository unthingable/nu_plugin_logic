use nu_protocol::{Record, Span, Value};

use super::search::SearchIterator;
use super::substitution::Substitution;
use super::term::Term;
use super::unify::unify;
use super::LogicEngine;

/// Hand-rolled unification + backtracking engine.
/// No external dependencies beyond nu-protocol.
pub struct NativeEngine;

impl LogicEngine for NativeEngine {
    fn filter(
        &self,
        pattern: Term,
        input: Box<dyn Iterator<Item = Value> + Send>,
        span: Span,
    ) -> Box<dyn Iterator<Item = Result<Value, String>> + Send> {
        Box::new(FilterIterator {
            pattern,
            input,
            span,
            done: false,
        })
    }

    fn search(
        &self,
        sources: Vec<(Term, Vec<Value>)>,
        span: Span,
    ) -> Box<dyn Iterator<Item = Result<Value, String>> + Send> {
        Box::new(SearchIterator::new(sources, span))
    }
}

/// Lazy filter iterator that unifies each input row against a pattern.
/// On `Err` from unify, yields the error and stops.
struct FilterIterator {
    pattern: Term,
    input: Box<dyn Iterator<Item = Value> + Send>,
    span: Span,
    done: bool,
}

impl Iterator for FilterIterator {
    type Item = Result<Value, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            let row = self.input.next()?;
            let mut sub = Substitution::new();
            match unify(&self.pattern, &row, &mut sub) {
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Ok(false) => continue,
                Ok(true) => {
                    let bindings = sub.into_bindings();
                    if bindings.is_empty() {
                        return Some(Ok(row));
                    }
                    let Value::Record { val: record, .. } = &row else {
                        return Some(Ok(row));
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
                    return Some(Ok(Value::record(result, self.span)));
                }
            }
        }
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
        let pattern = value_to_pattern(&pattern_val).unwrap();

        let results: Vec<Value> = engine
            .filter(pattern, Box::new(rows.into_iter()), span())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
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
            r.push("name", Value::string("&f", span()));
            Value::record(r, span())
        };
        let pattern = value_to_pattern(&pattern_val).unwrap();

        let results: Vec<Value> = engine
            .filter(pattern, Box::new(rows.into_iter()), span())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
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
            r.push("name", Value::string("&stem.rs", span()));
            Value::record(r, span())
        };
        let pattern = value_to_pattern(&pattern_val).unwrap();

        let results: Vec<Value> = engine
            .filter(pattern, Box::new(rows.into_iter()), span())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 1);
        if let Value::Record { val, .. } = &results[0] {
            assert_eq!(val.get("stem"), Some(&Value::string("main", span())));
        }
    }
}
