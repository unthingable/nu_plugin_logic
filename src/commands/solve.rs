use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand};
use nu_protocol::{Category, LabeledError, PipelineData, Signature, SyntaxShape, Type, Value};

use crate::engine::convert::value_to_pattern;
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

Multi-source mode (named fact sets):
  solve {proc: {pid: "$pid", name: "$name"}, ports: {pid: "$pid", port: "$port"}}

$-prefixed strings are logic variables. Shared variable names across
sources become join conditions. "_" is a wildcard (matches anything)."#
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

        // Multi-source mode: all values are records AND all keys are registered fact sets
        let all_values_are_records = !record.is_empty()
            && record
                .iter()
                .all(|(_, v)| matches!(v, Value::Record { .. }));

        if all_values_are_records {
            let store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;

            if record.iter().all(|(k, _)| store.get(k).is_some()) {
                // Multi-source mode
                let mut queries = Vec::new();
                for (source_name, pattern_val) in record.iter() {
                    queries.push((source_name.to_string(), value_to_pattern(pattern_val)));
                }

                let results = plugin
                    .engine
                    .search(&queries, &store, span)
                    .map_err(|e| LabeledError::new(e))?;

                return Ok(PipelineData::Value(Value::list(results, span), None));
            }
            // Not all keys in store — fall through to single-source
            // (supports nested record patterns against pipeline input)
        }

        // Single-source mode: filter pipeline input
        let pattern = value_to_pattern(&pattern_val);
        let rows: Vec<Value> = input.into_iter().collect();
        let results = plugin.engine.filter(&pattern, &rows, span);
        Ok(PipelineData::Value(Value::list(results, span), None))
    }
}
