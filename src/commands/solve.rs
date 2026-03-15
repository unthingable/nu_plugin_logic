use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand};
use nu_protocol::{
    Category, LabeledError, ListStream, PipelineData, Signals, Signature, SyntaxShape, Type, Value,
};

use crate::engine::convert::value_to_pattern;
use crate::engine::term::Term;
use crate::LogicEngine;
use crate::LogicPlugin;

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
  ls | solve {type: "file", name: "$stem.rs"}

Multi-source mode (pipeline record-of-tables):
  {proc: $proc, ports: $ports} | solve {proc: {pid: "$pid"}, ports: {pid: "$pid", port: "$port"}}

Multi-source mode (named fact sets):
  solve {proc: {pid: "$pid", name: "$name"}, ports: {pid: "$pid", port: "$port"}}

$-prefixed strings are logic variables. Shared variable names across
sources become join conditions.

Results stream lazily — piping to `first N` short-circuits after N solutions."#
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let pattern_val: Value = call.req(0)?;

        // Pattern must be a record
        let Value::Record { val: record, .. } = &pattern_val else {
            return Err(
                LabeledError::new("solve expects a record pattern")
                    .with_label("expected a record", span),
            );
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
                let stream = ListStream::new(iter, span, Signals::empty());
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
                    sources.push((value_to_pattern(pattern_val), facts));
                }
                drop(store);

                let iter = plugin.engine.search(sources, span);
                let stream = ListStream::new(iter, span, Signals::empty());
                return Ok(PipelineData::ListStream(stream, None));
            }
            // Not in store either — fall through to single-source
            // (supports nested record patterns against pipeline input)
        }

        // Single-source mode: filter pipeline input
        let pattern = value_to_pattern(&pattern_val);
        let rows: Vec<Value> = input.into_iter().collect();
        let iter = plugin.engine.filter(pattern, rows, span);
        let stream = ListStream::new(iter, span, Signals::empty());
        Ok(PipelineData::ListStream(stream, None))
    }
}

/// Extract sources from a pipeline record-of-tables.
/// The input must be a `Value::Record` where each field matching
/// a pattern key contains a `Value::List` of rows.
fn sources_from_pipeline(
    input: PipelineData,
    pattern_record: &impl std::ops::Deref<Target = nu_protocol::Record>,
    span: nu_protocol::Span,
) -> Result<Vec<(Term, Vec<Value>)>, LabeledError> {
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
            vals.iter().cloned().collect()
        } else {
            return Err(
                LabeledError::new(format!("Source '{source_name}' is not a table"))
                    .with_label("expected a list of records", span),
            );
        };
        sources.push((value_to_pattern(pattern_val), facts));
    }
    Ok(sources)
}
