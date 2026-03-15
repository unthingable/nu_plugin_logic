use nu_protocol::{Record, Span, Value};

use super::substitution::Substitution;
use super::term::Term;
use super::unify::unify;
use crate::store::FactStore;

/// Depth-first backtracking search across multiple named fact sets.
///
/// Each query is `(source_name, pattern)`. The search iterates rows in the first
/// source, unifies each against its pattern, and on success carries the substitution
/// forward to the next source. When all sources match, the complete substitution
/// is emitted as a result row.
pub fn search(
    queries: &[(String, Term)],
    store: &FactStore,
    span: Span,
) -> Result<Vec<Value>, String> {
    // Resolve each source name to its fact rows
    let sources: Vec<(&Term, &[Value])> = queries
        .iter()
        .map(|(name, pattern)| {
            let facts = store
                .get(name)
                .ok_or_else(|| format!("Unknown fact set: '{name}'"))?;
            Ok((pattern, facts.as_slice()))
        })
        .collect::<Result<Vec<_>, String>>()?;

    // Search smallest fact sets first to prune the search space early
    let mut sources = sources;
    sources.sort_by_key(|(_, facts)| facts.len());

    let mut results = Vec::new();
    backtrack(&sources, 0, Substitution::new(), &mut results, span);
    Ok(results)
}

fn backtrack(
    sources: &[(&Term, &[Value])],
    depth: usize,
    sub: Substitution,
    results: &mut Vec<Value>,
    span: Span,
) {
    if depth >= sources.len() {
        // All sources matched — emit a result row from the bindings
        let bindings = sub.into_bindings();
        let mut record = Record::new();
        // Sort by key for stable column order
        let mut entries: Vec<_> = bindings.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in entries {
            record.push(name, value);
        }
        results.push(Value::record(record, span));
        return;
    }

    let (pattern, facts) = &sources[depth];
    for row in *facts {
        let mut new_sub = sub.clone();
        if unify(pattern, row, &mut new_sub) {
            backtrack(sources, depth + 1, new_sub, results, span);
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
            r.push("pid", Value::string("$pid", span()));
            r.push("name", Value::string("$name", span()));
            value_to_pattern(&Value::record(r, span()))
        };
        let ports_pattern = {
            let mut r = Record::new();
            r.push("pid", Value::string("$pid", span()));
            r.push("port", Value::string("$port", span()));
            value_to_pattern(&Value::record(r, span()))
        };

        let queries = vec![
            ("proc".into(), proc_pattern),
            ("ports".into(), ports_pattern),
        ];

        let results = search(&queries, &store, span()).unwrap();
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
    fn missing_source_errors() {
        let store = make_store();
        let pattern = {
            let mut r = Record::new();
            r.push("x", Value::string("$x", span()));
            value_to_pattern(&Value::record(r, span()))
        };
        let queries = vec![("nonexistent".into(), pattern)];
        assert!(search(&queries, &store, span()).is_err());
    }
}
