use nu_protocol::Value;

/// Variable bindings accumulated during unification.
/// Preserves insertion order — variables appear in the order they were
/// first bound, which matches the declaration order in the pattern.
#[derive(Debug, Clone)]
pub struct Substitution {
    bindings: Vec<(String, Value)>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    pub fn bind(&mut self, name: String, value: Value) {
        // Update if already bound, otherwise append
        if let Some(entry) = self.bindings.iter_mut().find(|(k, _)| k == &name) {
            entry.1 = value;
        } else {
            self.bindings.push((name, value));
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.bindings
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    pub fn into_bindings(self) -> Vec<(String, Value)> {
        self.bindings
    }
}
