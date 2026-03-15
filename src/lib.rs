mod commands;
pub mod engine;
mod store;

use std::sync::{Arc, Mutex};

use nu_plugin::Plugin;

use commands::{Facts, Solve};
use engine::native::NativeEngine;
use engine::LogicEngine;
use store::FactStore;

pub struct LogicPlugin {
    pub(crate) store: Arc<Mutex<FactStore>>,
    pub(crate) engine: NativeEngine,
}

impl LogicPlugin {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(FactStore::new())),
            engine: NativeEngine,
        }
    }
}

impl Plugin for LogicPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn nu_plugin::PluginCommand<Plugin = Self>>> {
        vec![Box::new(Solve), Box::new(Facts)]
    }
}
