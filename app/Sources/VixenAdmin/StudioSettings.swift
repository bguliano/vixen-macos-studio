import SwiftUI
import VixenKit

/// Studio's Settings window (⌘,): Studio's own preferences first, then Vixen's engine options.
struct StudioSettingsView: View {
    /// Tools ▸ Plugins ▸ Manage Plugins… opens Settings on Plugins.
    @AppStorage(StudioSettingsTab.storageKey) private var tab = "general"

    var body: some View {
        TabView(selection: $tab) {
            Tab("General", systemImage: "gearshape", value: "general") {
                StudioGeneralSettings()
            }
            Tab("Agents", systemImage: "sparkles", value: "agents") {
                StudioAgentSettings()
            }
            Tab("Plugins", systemImage: "puzzlepiece.extension", value: StudioSettingsTab.plugins) {
                StudioPluginsSettings()
            }
            Tab("Engine", systemImage: "cpu", value: "engine") {
                OptionsForm()
            }
        }
        .scenePadding()
        .onAppear {
            // `{ "action": "hook", "name": "studio.settings", "path": ["tab", "plugins"] }`: the tab bar isn't scriptable.
            guard UITestHarness.isActive else { return }
            UITestHarness.hooks["studio.settings"] = { args in
                guard args.first == "tab", args.count > 1 else { return "FAIL tab <general|agents|plugins|engine>" }
                tab = args[1]
                try? await Task.sleep(for: .milliseconds(500))
                return tab
            }
        }
    }
}

private struct StudioGeneralSettings: View {
    @AppStorage(StudioAppearance.storageKey) private var appearance = StudioAppearance.system.rawValue

    var body: some View {
        Form {
            Section {
                Picker("Appearance", selection: $appearance) {
                    ForEach(StudioAppearance.allCases) { option in
                        Text(option.title).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
            } footer: {
                Text("The timeline and previews always stay dark, so lights read clearly.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            StudioUpdateSettingsSection()
        }
        .formStyle(.grouped)
        .frame(width: 440)
        .fixedSize(horizontal: false, vertical: true)
    }
}
