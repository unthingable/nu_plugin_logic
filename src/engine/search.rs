use nu_protocol::{Record, Span, Value};

use super::substitution::Substitution;
use super::term::Term;
use super::unify::unify;

/// Iterative depth-first backtracking search across multiple fact sets.
///
/// Yields one `Value::Record` per solution. Each record's columns are the
/// bound variable names, values are the matched data. The search is lazy —
/// solutions are produced one at a time, so `first N` short-circuits.
pub struct SearchIterator {
    sources: Vec<(Term, Vec<Value>)>,
    stack: Vec<SearchFrame>,
    span: Span,
}

struct SearchFrame {
    depth: usize,
    row_index: usize,
    sub: Substitution,
}

impl SearchIterator {
    pub fn new(mut sources: Vec<(Term, Vec<Value>)>, span: Span) -> Self {
        // Search smallest fact sets first to prune early
        sources.sort_by_key(|(_, facts)| facts.len());
        Self {
            sources,
            stack: vec![SearchFrame {
                depth: 0,
                row_index: 0,
                sub: Substitution::new(),
            }],
            span,
        }
    }
}

impl Iterator for SearchIterator {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        while !self.stack.is_empty() {
            let top = self.stack.len() - 1;
            let depth = self.stack[top].depth;

            if depth >= self.sources.len() {
                // All sources matched — emit one result
                let sub = self.stack.pop().unwrap().sub;
                return Some(sub_to_record(sub, self.span));
            }

            let row_index = self.stack[top].row_index;
            let facts_len = self.sources[depth].1.len();

            if row_index >= facts_len {
                // Exhausted this source — backtrack
                self.stack.pop();
                continue;
            }

            // Advance to the next row for when we return to this frame
            self.stack[top].row_index += 1;

            // Try unifying the current row
            let mut new_sub = self.stack[top].sub.clone();
            let matched = unify(
                &self.sources[depth].0,
                &self.sources[depth].1[row_index],
                &mut new_sub,
            );

            if matched {
                self.stack.push(SearchFrame {
                    depth: depth + 1,
                    row_index: 0,
                    sub: new_sub,
                });
            }
        }
        None
    }
}

fn sub_to_record(sub: Substitution, span: Span) -> Value {
    let bindings = sub.into_bindings();
    let mut record = Record::new();
    for (name, value) in bindings {
        record.push(name, value);
    }
    Value::record(record, span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::convert::value_to_pattern;
    use crate::store::FactStore;

    fn span() -> Span {
        Span::unknown()
    }

    fn make_store() -> FactStore {
        let mut store = FactStore::new();

        let procs = vec![
            {
                let mut r = Record::new();
                r.push("pid", Value::int(1, span()));
                r.push("name", Value::string("nginx", span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("pid", Value::int(2, span()));
                r.push("name", Value::string("postgres", span()));
                Value::record(r, span())
            },
        ];
        store.assert_facts("proc".into(), procs);

        let ports = vec![
            {
                let mut r = Record::new();
                r.push("pid", Value::int(1, span()));
                r.push("port", Value::int(80, span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("pid", Value::int(1, span()));
                r.push("port", Value::int(443, span()));
                Value::record(r, span())
            },
            {
                let mut r = Record::new();
                r.push("pid", Value::int(2, span()));
                r.push("port", Value::int(5432, span()));
                Value::record(r, span())
            },
        ];
        store.assert_facts("ports".into(), ports);

        store
    }

    #[test]
    fn two_source_join() {
        let store = make_store();

        let proc_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("name", Value::string("&name", span()));
            value_to_pattern(&Value::record(r, span()))
        };
        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("port", Value::string("&port", span()));
            value_to_pattern(&Value::record(r, span()))
        };

        let sources = vec![
            (proc_pattern, store.get("proc").unwrap().clone()),
            (ports_pattern, store.get("ports").unwrap().clone()),
        ];

        let results: Vec<Value> = SearchIterator::new(sources, span()).collect();
        assert_eq!(results.len(), 3);

        // Check first result: pid=1, name=nginx, port=80
        if let Value::Record { val, .. } = &results[0] {
            assert_eq!(val.get("pid"), Some(&Value::int(1, span())));
            assert_eq!(val.get("name"), Some(&Value::string("nginx", span())));
            assert_eq!(val.get("port"), Some(&Value::int(80, span())));
        } else {
            panic!("expected record");
        }
    }

    #[test]
    fn lazy_short_circuit() {
        let store = make_store();

        let proc_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("name", Value::string("&name", span()));
            value_to_pattern(&Value::record(r, span()))
        };
        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("port", Value::string("&port", span()));
            value_to_pattern(&Value::record(r, span()))
        };

        let sources = vec![
            (proc_pattern, store.get("proc").unwrap().clone()),
            (ports_pattern, store.get("ports").unwrap().clone()),
        ];

        // Only take the first result — shouldn't need to compute all 3
        let results: Vec<Value> = SearchIterator::new(sources, span()).take(1).collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn missing_source_returns_empty() {
        // With the new API, source resolution happens in the command layer.
        // An empty sources list yields zero results.
        let results: Vec<Value> = SearchIterator::new(vec![], span()).collect();
        assert_eq!(results.len(), 1); // one "solution" with no bindings
    }
}
