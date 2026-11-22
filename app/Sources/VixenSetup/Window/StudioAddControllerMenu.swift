import SwiftUI
import VixenKit

/// The Add Controller menu's items: built-in types, then installed plugins' types under a "Plugins" heading (or Install
/// Plugins… when there are none). The list is read from Rust each time, so a plugin installed while the pane is open
/// shows up without reopening it.
struct StudioAddControllerMenuItems: View {
    let model: SetupModel
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        let plugins = StudioPluginsModel.shared
        let _ = plugins.revision
        let pluginIDs = plugins.pluginTypeIDs
        let types = model.controllerTypes
        ForEach(types.filter { !pluginIDs.contains($0.id) }, id: \.id) { type in
            Button(type.name) { add(type.id) }
        }
        Section("Plugins") {
            let offered = types.filter { pluginIDs.contains($0.id) }
            ForEach(offered, id: \.id) { type in
                Button(type.name) { add(type.id) }
            }
            if offered.isEmpty {
                Button("Install Plugins…") {
                    UserDefaults.standard.set(StudioSettingsTab.plugins, forKey: StudioSettingsTab.storageKey)
                    openSettings()
                }
            }
        }
    }

    private func add(_ id: String) {
        model.controllerTypeID = id
        Task { await model.addController() }
    }
}
