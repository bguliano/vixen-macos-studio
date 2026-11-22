//! Studio: a plugin controller's module data survives a profile opened without any plugin host (vanilla's situation:
//! no provider is registered in this test binary). The controller and its data load as an unknown module type, Display
//! Setup edits other parts of the controller, and the save writes the plugin's `<Module>` entry back unchanged.
use serde_json::json;
use uuid::Uuid;
use vixen_io::{Controller, ControllerOutput, ModuleEntry};
use vixen_output::modules::studio_external::{ASSEMBLY_NAME, DATA_MODEL_TYPE};
use vixen_session::app::AppCore;
use vixen_xml::XElement;

const TYPE_ID: &str = "6f1c2a8e-0000-4000-8000-00000000c0de";

fn plugin_entry(store: &str) -> String {
    let at = store.find(&format!("moduleType=\"{TYPE_ID}\"")).expect("the plugin's entry is in ModuleStore.xml");
    let open = store[..at].rfind("<Module ").unwrap();
    let close = store[open..].find("</Module>").unwrap();
    store[open..open + close].to_string()
}

#[test]
fn plugin_controller_data_survives_a_profile_without_plugins() {
    let root = std::env::temp_dir().join(format!("studio-plugin-data-{}", Uuid::new_v4()));
    let type_id = Uuid::parse_str(TYPE_ID).unwrap();
    let (controller, instance) = (Uuid::new_v4(), Uuid::new_v4());
    {
        let core = AppCore::open(root.clone(), true).unwrap();
        let mut system = core.system.write();
        system.controllers.push(Controller {
            name: "Lamps".into(),
            hardware_type_id: type_id,
            hardware_instance_id: instance,
            id: controller,
            outputs: (0..3).map(|i| ControllerOutput { name: format!("Lamp {i}"), id: Uuid::new_v4() }).collect(),
        });
        let data = XElement::parse(&format!(
            "<PluginControllerData><ModuleInstanceId>{instance}</ModuleInstanceId><ModuleTypeId>{TYPE_ID}</ModuleTypeId><PluginId>com.example.lamp</PluginId><PluginName>Lamp Output</PluginName><PluginVersion>1.2.0</PluginVersion><Config>{{\"host\":\"10.1.1.1\",\"note\":\"a &lt;b&gt; &amp; c\"}}</Config></PluginControllerData>"
        ))
        .unwrap();
        system.module_store.set_instance(ModuleEntry::new(DATA_MODEL_TYPE, &type_id, &instance, ASSEMBLY_NAME, data));
        drop(system);
        core.save_system_config().unwrap();
        core.save_module_config().unwrap();
    }
    let store_path = root.join("SystemData/ModuleStore.xml");
    let before = plugin_entry(&std::fs::read_to_string(&store_path).unwrap());
    assert!(before.contains("<PluginId>com.example.lamp</PluginId>"), "{before}");

    let core = AppCore::open(root.clone(), true).unwrap();
    let run = |name: &str, args: serde_json::Value| core.studio_display.execute(&core, name, &args, false);
    run("controller_rename", json!({"id": controller.to_string(), "name": "Lamps Renamed"})).unwrap();
    run("controller_create", json!({"name": "Other", "typeId": vixen_output::ids::DDP.to_string(), "outputs": 3})).unwrap();
    run("profile_save", json!({"confirm": true})).unwrap();
    let after = std::fs::read_to_string(&store_path).unwrap();
    assert_eq!(plugin_entry(&after), before);
    let config = std::fs::read_to_string(root.join("SystemData/SystemConfig.xml")).unwrap();
    assert!(config.contains("Lamps Renamed") && config.contains(&instance.to_string()), "the controller stays");
    assert!(!root.join("SystemData/StudioPlugins.json").exists(), "nothing records requirements without a plugin host");
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}
