import SwiftUI
import VixenKit

/// Above the show window's sections while the show needs plugins that aren't ready (its `StudioPlugins.json` against
/// what's installed): which ones and why, with Manage Plugins… to fix it. The controllers stay in the show, silent,
/// and come back on their own once the plugin is installed or turned on. MCP `plugin_required` reads the same.
/// No text here may insist on its full height (`fixedSize`): the split view first lays the detail column out very narrow,
/// where that wrapped every word onto its own line and made the window's content taller than the screen, pushing the
/// sidebar and the list out of the window (B100). Long lines truncate instead, with the whole text in the tooltip.
struct StudioPluginBanner: View {
    let missing: [StudioPluginNeed]
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
            Image(systemName: "puzzlepiece.extension.fill")
                .foregroundStyle(.orange)
            VStack(alignment: .leading, spacing: 2) {
                Text(missing.count == 1 ? "This show needs a plugin that isn't ready" : "This show needs \(missing.count) plugins that aren't ready")
                    .font(.callout.weight(.semibold))
                ForEach(missing) { need in
                    Text(need.message ?? "\(need.id) \(need.versionRange)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .help(need.message ?? "\(need.id) \(need.versionRange)")
                }
                Text("Its controllers stay in the show and stay dark until the plugin is installed and on.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: StudioMetrics.spacingS)
            Button("Manage Plugins…") {
                UserDefaults.standard.set(StudioSettingsTab.plugins, forKey: StudioSettingsTab.storageKey)
                openSettings()
            }
        }
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.orange.opacity(0.12))
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityElement(children: .combine)
    }
}
