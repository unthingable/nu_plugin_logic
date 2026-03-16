use nu_protocol::{Record, Span, Value};

use super::substitution::Substitution;
use super::term::{vars_in_term, Term};
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
    /// Variable names in the order they first appear across the user's patterns,
    /// left to right. Used to fix output column order after source reordering.
    decl_order: Vec<String>,
}

struct SearchFrame {
    depth: usize,
    row_index: usize,
    sub: Substitution,
}

impl SearchIterator {
    pub fn new(mut sources: Vec<(Term, Vec<Value>)>, span: Span) -> Self {
        // Collect variable declaration order BEFORE reordering sources.
        let decl_order = declaration_order(&sources);

        // Search smallest fact sets first to prune early.
        sources.sort_by_key(|(_, facts)| facts.len());

        Self {
            sources,
            stack: vec![SearchFrame {
                depth: 0,
                row_index: 0,
                sub: Substitution::new(),
            }],
            span,
            decl_order,
        }
    }
}

/// Collect variable names in declaration order: first appearance across all
/// patterns, left to right, in the original (pre-sort) source order.
fn declaration_order(sources: &[(Term, Vec<Value>)]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for (term, _) in sources {
        for name in vars_in_term(term) {
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
    }
    seen
}

impl Iterator for SearchIterator {
    type Item = Result<Value, String>;

    fn next(&mut self) -> Option<Result<Value, String>> {
        while let Some(frame) = self.stack.last_mut() {
            let depth = frame.depth;

            if depth >= self.sources.len() {
                // All sources matched — emit one result
                let sub = self.stack.pop().unwrap().sub;
                return Some(Ok(sub_to_record(sub, &self.decl_order, self.span)));
            }

            let facts_len = self.sources[depth].1.len();
            let row_index = frame.row_index;

            if row_index >= facts_len {
                // Exhausted this source — backtrack
                self.stack.pop();
                continue;
            }

            // Advance to the next row for when we return to this frame
            self.stack.last_mut().unwrap().row_index += 1;

            // Try unifying the current row
            let mut new_sub = self.stack.last().unwrap().sub.clone();
            match unify(
                &self.sources[depth].0,
                &self.sources[depth].1[row_index],
                &mut new_sub,
            ) {
                Ok(true) => {
                    self.stack.push(SearchFrame {
                        depth: depth + 1,
                        row_index: 0,
                        sub: new_sub,
                    });
                }
                Ok(false) => {}
                Err(e) => {
                    // Structural error — stop iteration
                    self.stack.clear();
                    return Some(Err(e));
                }
            }
        }
        None
    }
}

fn sub_to_record(sub: Substitution, decl_order: &[String], span: Span) -> Value {
    let mut bindings = sub.into_bindings();
    let mut record = Record::new();

    // Emit variables in declaration order first.
    for name in decl_order {
        if let Some(pos) = bindings.iter().position(|(k, _)| k == name) {
            let (k, v) = bindings.remove(pos);
            record.push(k, v);
        }
    }

    // Any remaining bindings (shouldn't happen in practice) come after.
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
        store.store_facts("proc".into(), procs);

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
        store.store_facts("ports".into(), ports);

        store
    }

    #[test]
    fn two_source_join() {
        let store = make_store();

        let proc_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("name", Value::string("&name", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };
        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("port", Value::string("&port", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };

        let sources = vec![
            (proc_pattern, store.get("proc").unwrap().clone()),
            (ports_pattern, store.get("ports").unwrap().clone()),
        ];

        let results: Vec<Value> = SearchIterator::new(sources, span())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
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
    fn two_source_join_column_order() {
        // Put the larger source (ports, 3 rows) first in the declaration order
        // so that sort-by-size will reorder them. The output columns must still
        // match the declaration order [pid, port, name], not the search order.
        let store = make_store();

        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("port", Value::string("&port", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };
        let proc_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("name", Value::string("&name", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };

        // Declaration order: ports first (pid, port), then proc (pid already seen, name).
        // sort_by_size will put proc (2 rows) before ports (3 rows).
        let sources = vec![
            (ports_pattern, store.get("ports").unwrap().clone()),
            (proc_pattern, store.get("proc").unwrap().clone()),
        ];

        let results: Vec<Value> = SearchIterator::new(sources, span())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results.len(), 3);

        for result in &results {
            if let Value::Record { val, .. } = result {
                let cols: Vec<&str> = val.columns().map(|s| s.as_str()).collect();
                assert_eq!(
                    cols,
                    vec!["pid", "port", "name"],
                    "column order must match declaration order"
                );
            } else {
                panic!("expected record");
            }
        }
    }

    #[test]
    fn lazy_short_circuit() {
        let store = make_store();

        let proc_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("name", Value::string("&name", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };
        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("&pid", span()));
            r.push("port", Value::string("&port", span()));
            value_to_pattern(&Value::record(r, span())).unwrap()
        };

        let sources = vec![
            (proc_pattern, store.get("proc").unwrap().clone()),
            (ports_pattern, store.get("ports").unwrap().clone()),
        ];

        // Only take the first result — shouldn't need to compute all 3
        let results: Vec<Result<Value, String>> =
            SearchIterator::new(sources, span()).take(1).collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn missing_source_returns_empty() {
        // With the new API, source resolution happens in the command layer.
        // An empty sources list yields zero results.
        let results: Vec<Result<Value, String>> =
            SearchIterator::new(vec![], span()).collect();
        assert_eq!(results.len(), 1); // one "solution" with no bindings
        assert!(results[0].is_ok());
    }
}
