use nu_protocol::Value;

/// Variable bindings accumulated during unification.
/// Preserves insertion order — variables appear in the order they were
/// first bound, which matches the declaration order in the pattern.
#[derive(Debug, Clone)]
pub struct Substitution {
    bindings: Vec<(String, Value)>,
}

impl Default for Substitution {
    fn default() -> Self {
        Self::new()
    }
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    pub fn bind(&mut self, name: String, value: Value) {
        debug_assert!(self.get(&name).is_none(), "bind called for already-bound variable: {name}");
        self.bindings.push((name, value));
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
