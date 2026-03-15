use std::collections::HashMap;
use nu_protocol::Value;

/// Variable bindings accumulated during unification.
#[derive(Debug, Clone)]
pub struct Substitution {
    bindings: HashMap<String, Value>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, name: String, value: Value) {
        self.bindings.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.bindings.get(name)
    }

    pub fn into_bindings(self) -> HashMap<String, Value> {
        self.bindings
    }
}
