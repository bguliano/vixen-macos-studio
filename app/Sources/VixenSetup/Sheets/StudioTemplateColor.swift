import SwiftUI
import VixenBridge
import VixenKit

/// The Colour choice (RGB, RGBW or Single colour) in the Single Item, Megatree and Pixel Grid templates. Rust names the
/// choices and applies them (`studio_color.rs`), the same code as the MCP `element_color` tool, so the new lights get the
/// colour property and breakdown filter at once instead of a separate Color Handling sheet.
enum StudioTemplateColor {
    struct Choice: Decodable, Hashable {
        let mode: String
        let label: String
        let channelsPerPixel: Int
        let note: String
    }

    static let choices: [Choice] = (try? JSONDecoder().decode([Choice].self, from: Data(studioColorChoices().utf8))) ?? []

    /// Each template's last choice, for the app's life, as the templates remember their other fields.
    @MainActor static var chosen: [SetupTemplateKind: String] = [:]

    static func supports(_ kind: SetupTemplateKind) -> Bool {
        kind == .singleItem || kind == .megatree || kind == .pixelGrid
    }

    @MainActor static func mode(for kind: SetupTemplateKind) -> String {
        chosen[kind] ?? "rgb"
    }

    /// Applies the template's choice to what it made. Call off the main thread.
    nonisolated static func apply(_ mode: String, to nodes: [String], in session: DisplaySetupSession) {
        guard !nodes.isEmpty,
              let json = try? JSONSerialization.data(withJSONObject: ["ids": nodes, "mode": mode]),
              let arguments = String(data: json, encoding: .utf8) else { return }
        _ = try? session.studioApplyColorHandling(argumentsJson: arguments)
    }
}

/// "Color:" with a segmented picker and the choice's channel count underneath.
struct StudioTemplateColorPicker: View {
    @Binding var mode: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Picker("Color:", selection: $mode) {
                ForEach(StudioTemplateColor.choices, id: \.mode) { Text($0.label).tag($0.mode) }
            }
            .pickerStyle(.segmented)
            .fixedSize()
            Text(StudioTemplateColor.choices.first { $0.mode == mode }?.note ?? "")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
