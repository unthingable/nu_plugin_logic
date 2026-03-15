use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand};
use nu_protocol::{
    Category, LabeledError, PipelineData, Record, Signature, SyntaxShape, Type, Value,
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
            .input_output_types(vec![(Type::Any, Type::Nothing)])
            .required("name", SyntaxShape::String, "name to store facts under")
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "Store pipeline data as named facts for use with solve"
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let name: String = call.req(0)?;
        let values: Vec<Value> = input.into_iter().collect();

        plugin
            .store
            .lock()
            .map_err(|e| LabeledError::new(format!("lock error: {e}")))?
            .assert_facts(name, values);

        Ok(PipelineData::Empty)
    }
}

pub struct FactsList;

impl PluginCommand for FactsList {
    type Plugin = LogicPlugin;

    fn name(&self) -> &str {
        "facts list"
    }

    fn signature(&self) -> Signature {
        Signature::build("facts list")
            .input_output_types(vec![(Type::Nothing, Type::Any)])
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "List registered fact sets"
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
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

        Ok(PipelineData::Value(Value::list(rows, span), None))
    }
}

pub struct FactsClear;

impl PluginCommand for FactsClear {
    type Plugin = LogicPlugin;

    fn name(&self) -> &str {
        "facts clear"
    }

    fn signature(&self) -> Signature {
        Signature::build("facts clear")
            .input_output_types(vec![(Type::Nothing, Type::Nothing)])
            .optional("name", SyntaxShape::String, "fact set to clear (omit to clear all)")
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "Clear fact sets"
    }

    fn run(
        &self,
        plugin: &Self::Plugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let name: Option<String> = call.opt(0)?;

        let mut store = plugin
            .store
            .lock()
            .map_err(|e| LabeledError::new(format!("lock error: {e}")))?;

        match name {
            Some(name) => {
                if !store.clear(&name) {
                    return Err(
                        LabeledError::new(format!("Unknown fact set: '{name}'"))
                            .with_label("not registered", span),
                    );
                }
            }
            None => store.clear_all(),
        }

        Ok(PipelineData::Empty)
    }
}
