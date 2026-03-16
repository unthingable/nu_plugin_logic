use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand};
use nu_protocol::{
    Category, LabeledError, ListStream, PipelineData, Record, Signature, SyntaxShape, Type, Value,
};

use crate::LogicPlugin;

pub struct Facts;

impl PluginCommand for Facts {
    type Plugin = LogicPlugin;

    fn name(&self) -> &str {
        "facts"
    }

    fn signature(&self) -> Signature {
        Signature::build("facts")
            .input_output_types(vec![(Type::Any, Type::Any)])
            .optional("name", SyntaxShape::String, "fact set name")
            .switch("drop", "remove the named fact set", None)
            .switch("clear", "remove all fact sets", None)
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "Store, retrieve, and manage named fact sets for use with solve"
    }

    fn extra_description(&self) -> &str {
        r#"With pipeline input: store data and pass it through
  ls | facts files | where size > 1kb

Without input: retrieve stored data
  facts files

No arguments: list all registered fact sets
  facts

Management:
  facts files --drop    # remove one fact set
  facts --clear         # remove all fact sets"#
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let name: Option<String> = call.opt(0)?;
        let do_drop = call.has_flag("drop")?;
        let clear = call.has_flag("clear")?;

        if clear {
            let mut store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;
            let cleared: Vec<Value> = store
                .list()
                .into_iter()
                .map(|(name, count)| {
                    let mut record = Record::new();
                    record.push("name", Value::string(name, span));
                    record.push("rows", Value::int(count as i64, span));
                    Value::record(record, span)
                })
                .collect();
            store.clear_all();
            return Ok(PipelineData::Value(Value::list(cleared, span), None));
        }

        if do_drop {
            let name = name.ok_or_else(|| {
                LabeledError::new("--drop requires a fact set name")
                    .with_label("specify which fact set to drop", span)
            })?;
            let mut store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;
            let row_count = store
                .get(&name)
                .ok_or_else(|| {
                    LabeledError::new(format!("Unknown fact set: '{name}'"))
                        .with_label("not registered", span)
                })?
                .len();
            store.clear(&name);
            let mut record = Record::new();
            record.push("name", Value::string(&name, span));
            record.push("rows", Value::int(row_count as i64, span));
            return Ok(PipelineData::Value(Value::record(record, span), None));
        }

        let Some(name) = name else {
            // No name → list all fact sets
            let store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;
            let rows: Vec<Value> = store
                .list()
                .into_iter()
                .map(|(name, count)| {
                    let mut record = Record::new();
                    record.push("name", Value::string(name, span));
                    record.push("rows", Value::int(count as i64, span));
                    Value::record(record, span)
                })
                .collect();
            return Ok(PipelineData::Value(Value::list(rows, span), None));
        };

        if matches!(input, PipelineData::Empty) {
            // No input → retrieve
            let store = plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;
            let facts = store
                .get(&name)
                .ok_or_else(|| {
                    LabeledError::new(format!("Unknown fact set: '{name}'"))
                        .with_label("not registered", span)
                })?
                .clone();
            drop(store);
            let stream = ListStream::new(facts.into_iter(), span, engine.signals().clone());
            Ok(PipelineData::ListStream(stream, None))
        } else {
            // Has input → store + passthrough
            let values: Vec<Value> = input.into_iter().collect();
            plugin
                .store
                .lock()
                .map_err(|e| LabeledError::new(format!("lock error: {e}")))?
                .store_facts(name, values.clone());
            let stream = ListStream::new(values.into_iter(), span, engine.signals().clone());
            Ok(PipelineData::ListStream(stream, None))
        }
    }
}
