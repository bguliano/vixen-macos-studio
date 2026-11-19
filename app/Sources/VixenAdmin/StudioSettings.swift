import SwiftUI
import VixenKit

/// Studio's Settings window (⌘,): Studio's own preferences first, then Vixen's engine options.
struct StudioSettingsView: View {
    var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") {
                StudioGeneralSettings()
            }
            Tab("Agents", systemImage: "sparkles") {
                StudioAgentSettings()
            }
            Tab("Engine", systemImage: "cpu") {
                OptionsForm()
            }
        }
        .scenePadding()
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
        }
        .formStyle(.grouped)
        .frame(width: 440)
        .fixedSize(horizontal: false, vertical: true)
    }
}
