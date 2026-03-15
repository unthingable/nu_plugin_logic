use std::collections::HashMap;
use nu_protocol::Value;

/// In-memory storage for named fact sets.
/// Facts persist for the plugin process lifetime (i.e., the Nushell session).
pub struct FactStore {
    facts: HashMap<String, Vec<Value>>,
}

impl FactStore {
    pub fn new() -> Self {
        Self {
            facts: HashMap::new(),
        }
    }

    pub fn assert_facts(&mut self, name: String, values: Vec<Value>) {
        self.facts.insert(name, values);
    }

    pub fn get(&self, name: &str) -> Option<&Vec<Value>> {
        self.facts.get(name)
    }

    pub fn list(&self) -> Vec<(&str, usize)> {
        let mut entries: Vec<_> = self
            .facts
            .iter()
            .map(|(k, v)| (k.as_str(), v.len()))
            .collect();
        entries.sort_by_key(|(name, _)| *name);
        entries
    }

    pub fn clear(&mut self, name: &str) -> bool {
        self.facts.remove(name).is_some()
    }

    pub fn clear_all(&mut self) {
        self.facts.clear();
    }
}
