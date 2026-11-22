//! Studio: plugin controllers in the profile (`studio/PLUGINS-DESIGN.md`). After each save, the plugin host learns which
//! plugins the show's controllers need, and writes `SystemData/StudioPlugins.json` so the show can say what's missing
//! on another Mac.
use vixen_io::{DataPaths, SystemData};
use vixen_output::modules::studio_external::{self, ExternalConfig};
use vixen_output::ControllerConfig;

/// Every plugin controller's module data, in controller order.
pub fn external_configs(system: &SystemData) -> Vec<ExternalConfig> {
    system
        .controllers
        .iter()
        .filter_map(|c| {
            let entry = system.module_store.instance(c.hardware_type_id, c.hardware_instance_id)?;
            match ControllerConfig::from_xml(&c.hardware_type_id, &entry.element)? {
                ControllerConfig::External(config) => Some(config),
                _ => None,
            }
        })
        .collect()
}

/// Tells the plugin host the profile was saved. Does nothing without a plugin host (vanilla behaviour, tests).
pub fn system_saved(system: &SystemData, paths: &DataPaths) {
    if studio_external::provider().is_none() {
        return;
    }
    studio_external::system_saved(paths.root(), &external_configs(system));
}
