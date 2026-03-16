use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand};
use nu_protocol::{
    Category, LabeledError, ListStream, PipelineData, ShellError, Signature, Span, SyntaxShape,
    Type, Value,
};

use crate::engine::convert::value_to_pattern;
use crate::engine::term::Term;
use crate::store::FactStore;
use crate::LogicEngine;
use crate::LogicPlugin;

type Sources = Vec<(Term, Vec<Value>)>;

/// Convert an engine iterator of `Result<Value, String>` into a `Value` iterator
/// suitable for `ListStream`. On the first `Err`, yields a `Value::Error` and stops.
fn engine_results_to_values(
    iter: Box<dyn Iterator<Item = Result<Value, String>> + Send>,
    span: Span,
) -> impl Iterator<Item = Value> + Send {
    iter.map(move |r| match r {
        Ok(v) => v,
        Err(msg) => Value::error(
            ShellError::GenericError {
                error: msg,
                msg: "structural mismatch in pattern".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            },
            span,
        ),
    })
}

pub struct Solve;

impl PluginCommand for Solve {
    type Plugin = LogicPlugin;

    fn name(&self) -> &str {
        "solve"
    }

    fn signature(&self) -> Signature {
        Signature::build("solve")
            .input_output_types(vec![(Type::Any, Type::Any)])
            .required("pattern", SyntaxShape::Any, "record pattern to match against")
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "Find all valid combinations matching a pattern"
    }

    fn extra_description(&self) -> &str {
        r#"Single-source mode (pipeline input):
  ls | solve [type:file &name &size]
  ls | solve [type file, name &stem.rs]
  ls | solve {type: "file", name: "&stem.rs"}

Multi-source inline:
  solve [$proc [&pid &name] $ports [&pid &port]]
  solve [$proc [pid &pid, name &name], $ports [pid &pid, port &port]]

Multi-source via fact store:
  solve [@proc [pid &pid], @ports [pid &pid, port &port]]
  solve {proc: {pid: "&pid"}, ports: {pid: "&pid", port: "&port"}}

Multi-source via pipeline:
  {proc: $proc, ports: $ports} | solve {proc: {pid: "&pid"}, ports: {pid: "&pid", port: "&port"}}

Mixed (inline data + stored facts):
  solve [$fresh [pid &pid], @stored [pid &pid, port &port]]

Pattern list forms (all equivalent for extracting field `name`):
  &name          standalone extraction (field and variable both 'name')
  name:&name     infix colon pair
  name: &name    trailing colon pair
  name &name     bare pair

&-prefixed strings are logic variables. $-prefixed are nushell variables.
@-prefixed source names reference fact sets.

Results stream lazily — piping to `first N` short-circuits after N solutions."#
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let pattern_val: Value = call.req(0)?;

        // Inline multi-source: list of alternating [source, pattern, ...]
        // Sources can be tables ($var) or fact references (@name)
        if matches!(&pattern_val, Value::List { .. }) {
            let store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;
            if let Some(sources) = sources_from_inline_list(&pattern_val, &store, span)? {
                drop(store);
                let iter = plugin.engine.search(sources, span);
                let values = engine_results_to_values(iter, span);
                let stream = ListStream::new(values, span, engine.signals().clone());
                return Ok(PipelineData::ListStream(stream, None));
            }
            drop(store);
        }

        // Pattern must be a record (or list parsed as record pattern) for remaining modes.
        // Lists are handled via value_to_pattern which now parses flexible pattern syntax.
        if !matches!(&pattern_val, Value::Record { .. }) {
            // Try single-source filter with flexible pattern parsing
            let pattern = value_to_pattern(&pattern_val)
                .map_err(|e| LabeledError::new(e).with_label("invalid pattern", span))?;
            let iter = plugin
                .engine
                .filter(pattern, Box::new(input.into_iter()), span);
            let values = engine_results_to_values(iter, span);
            let stream = ListStream::new(values, span, engine.signals().clone());
            return Ok(PipelineData::ListStream(stream, None));
        }

        let Value::Record { val: record, .. } = &pattern_val else {
            unreachable!()
        };

        // Multi-source mode: pattern is record-of-records
        let all_values_are_records = !record.is_empty()
            && record
                .iter()
                .all(|(_, v)| matches!(v, Value::Record { .. }));

        if all_values_are_records {
            // Check if pipeline input is a record-of-tables with matching keys
            let input_has_sources =
                if let PipelineData::Value(Value::Record { val: ref input_rec, .. }, _) = input {
                    record.iter().all(|(k, _)| {
                        input_rec
                            .get(k)
                            .is_some_and(|v| matches!(v, Value::List { .. }))
                    })
                } else {
                    false
                };

            if input_has_sources {
                let sources = sources_from_pipeline(input, record, span)?;
                let iter = plugin.engine.search(sources, span);
                let values = engine_results_to_values(iter, span);
                let stream = ListStream::new(values, span, engine.signals().clone());
                return Ok(PipelineData::ListStream(stream, None));
            }

            // Try fact store
            let store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;

            if record.iter().all(|(k, _)| store.get(k).is_some()) {
                let mut sources = Vec::new();
                for (source_name, pattern_val) in record.iter() {
                    let facts = store.get(source_name).unwrap().clone();
                    let pat = value_to_pattern(pattern_val).map_err(|e| {
                        LabeledError::new(e).with_label("invalid pattern", span)
                    })?;
                    sources.push((pat, facts));
                }
                drop(store);

                let iter = plugin.engine.search(sources, span);
                let values = engine_results_to_values(iter, span);
                let stream = ListStream::new(values, span, engine.signals().clone());
                return Ok(PipelineData::ListStream(stream, None));
            }
            // Not in store either — fall through to single-source
        }

        // Single-source mode: filter pipeline input
        let pattern = value_to_pattern(&pattern_val)
            .map_err(|e| LabeledError::new(e).with_label("invalid pattern", span))?;
        let iter = plugin
            .engine
            .filter(pattern, Box::new(input.into_iter()), span);
        let values = engine_results_to_values(iter, span);
        let stream = ListStream::new(values, span, engine.signals().clone());
        Ok(PipelineData::ListStream(stream, None))
    }
}

/// Check if the argument is a list of alternating [source, pattern, ...] pairs.
/// Sources can be `Value::List` (inline data) or `@name` strings (fact references).
/// Patterns can be records or key-value lists.
fn sources_from_inline_list(
    val: &Value,
    store: &FactStore,
    span: nu_protocol::Span,
) -> Result<Option<Sources>, LabeledError> {
    let Value::List { vals, .. } = val else {
        return Ok(None);
    };

    if vals.len() < 2 || vals.len() % 2 != 0 {
        return Ok(None);
    }

    let is_pairs = vals
        .chunks(2)
        .all(|pair| is_data_source(&pair[0]) && is_pattern_like(&pair[1]));

    if !is_pairs {
        return Ok(None);
    }

    let mut sources = Vec::new();
    for pair in vals.chunks(2) {
        let data = resolve_data_source(&pair[0], store, span)?;
        let pattern = value_to_pattern(&pair[1]).map_err(|e| {
            LabeledError::new(e).with_label("invalid pattern", span)
        })?;
        sources.push((pattern, data));
    }

    Ok(Some(sources))
}

/// A value is a data source if it's a table (list) or a @-prefixed fact reference.
fn is_data_source(v: &Value) -> bool {
    match v {
        Value::List { .. } => true,
        Value::String { val, .. } => val.starts_with('@'),
        _ => false,
    }
}

/// Resolve a data source to its rows.
fn resolve_data_source(
    v: &Value,
    store: &FactStore,
    span: nu_protocol::Span,
) -> Result<Vec<Value>, LabeledError> {
    match v {
        Value::List { vals, .. } => Ok(vals.to_vec()),
        Value::String { val, .. } if val.starts_with('@') => {
            let name = &val[1..];
            let facts = store
                .get(name)
                .ok_or_else(|| {
                    LabeledError::new(format!("Unknown fact set: '{name}'"))
                        .with_label("not registered", span)
                })?
                .clone();
            Ok(facts)
        }
        _ => unreachable!("caller verified source is list or @-string"),
    }
}

/// A value looks like a pattern if it's a record, or a non-empty list whose
/// first element is a string that doesn't start with `@` (data source lists
/// contain records as elements, not strings).
fn is_pattern_like(v: &Value) -> bool {
    match v {
        Value::Record { .. } => true,
        Value::List { vals, .. } => {
            !vals.is_empty()
                && matches!(&vals[0], Value::String { val, .. } if !val.starts_with('@'))
        }
        _ => false,
    }
}

/// Extract sources from a pipeline record-of-tables.
fn sources_from_pipeline(
    input: PipelineData,
    pattern_record: &impl std::ops::Deref<Target = nu_protocol::Record>,
    span: nu_protocol::Span,
) -> Result<Sources, LabeledError> {
    let PipelineData::Value(Value::Record { val: input_record, .. }, _) = input else {
        unreachable!("caller verified input is a record");
    };

    let mut sources = Vec::new();
    for (source_name, pattern_val) in pattern_record.iter() {
        let data = input_record.get(source_name).ok_or_else(|| {
            LabeledError::new(format!("Missing source: '{source_name}'"))
                .with_label("not found in pipeline input", span)
        })?;
        let facts: Vec<Value> = if let Value::List { vals, .. } = data {
            vals.to_vec()
        } else {
            return Err(
                LabeledError::new(format!("Source '{source_name}' is not a table"))
                    .with_label("expected a list of records", span),
            );
        };
        let pat = value_to_pattern(pattern_val).map_err(|e| {
            LabeledError::new(e).with_label("invalid pattern", span)
        })?;
        sources.push((pat, facts));
    }
    Ok(sources)
}
