use nu_plugin::{serve_plugin, MsgPackSerializer};
use nu_plugin_logic::LogicPlugin;

fn main() {
    let plugin = LogicPlugin::new();
    serve_plugin(&plugin, MsgPackSerializer);
}
