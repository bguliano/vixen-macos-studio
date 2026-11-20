import SwiftUI
import VixenBridge
import VixenKit

/// "Show in Preview" in the Megatree, Pixel Grid and Arch templates: the new prop also gets the matching preview shape, already
/// linked light for light in the prop's order, so it needn't be drawn and linked by hand. Rust builds the shape
/// (`studio_props::add_linked`), the code the MCP `element_template` previewId and `preview_prop_create` elementId run.
enum StudioTemplatePreview {
    struct Choice: Decodable, Hashable {
        let id: String
        let name: String
    }

    /// The profile's previews, read when a template opens.
    @MainActor static var choices: [Choice] = []

    /// Each template's last choice for the app's life ("" is none), as the templates remember their other fields.
    @MainActor static var chosen: [SetupTemplateKind: String] = [:]

    nonisolated static func supports(_ kind: SetupTemplateKind) -> Bool {
        shape(for: kind) != nil
    }

    /// The shape kind Rust builds for a template.
    nonisolated static func shape(for kind: SetupTemplateKind) -> String? {
        switch kind {
        case .megatree: "megatree"
        case .pixelGrid: "matrix"
        case .arch: "arch"
        default: nil
        }
    }

    /// The template's preview, or "" when none is chosen or the chosen one is gone.
    @MainActor static func preview(for kind: SetupTemplateKind) -> String {
        let id = chosen[kind] ?? ""
        return choices.contains { $0.id == id } ? id : ""
    }

    /// Reads the previews from the working copy. Call off the main thread.
    nonisolated static func load(from session: DisplaySetupSession) -> [Choice] {
        (try? JSONDecoder().decode([Choice].self, from: Data(session.studioPreviewChoices().utf8))) ?? []
    }

    /// Adds the linked shape for `element`, the head of what the template made. Returns why it couldn't, if it couldn't.
    /// Call off the main thread.
    nonisolated static func add(to element: String, kind: SetupTemplateKind, preview: String, in session: DisplaySetupSession) -> String? {
        guard let shape = shape(for: kind),
              let json = try? JSONSerialization.data(withJSONObject: ["previewId": preview, "elementId": element, "kind": shape]),
              let arguments = String(data: json, encoding: .utf8) else { return nil }
        do {
            _ = try session.studioAddPreviewShape(argumentsJson: arguments)
            return nil
        } catch FfiError.Failed(let message) {
            return message
        } catch {
            return "\(error)"
        }
    }
}

/// "Show in Preview:" with the profile's previews, and a line saying what choosing one does.
struct StudioTemplatePreviewPicker: View {
    let kind: SetupTemplateKind
    @Binding var preview: String

    private var shapeName: String {
        switch kind {
        case .megatree: "Mega Tree"
        case .pixelGrid: "Pixel Grid"
        default: "Arch"
        }
    }

    var body: some View {
        let choices = StudioTemplatePreview.choices
        VStack(alignment: .leading, spacing: 4) {
            Picker("Show in Preview:", selection: $preview) {
                Text("None").tag("")
                if !choices.isEmpty { Divider() }
                ForEach(choices, id: \.id) { Text($0.name).tag($0.id) }
            }
            .fixedSize()
            .disabled(choices.isEmpty)
            Text(choices.isEmpty
                ? "There are no previews yet. Add one under Previews to show new props in it."
                : preview.isEmpty
                    ? "Choose a preview to add a matching \(shapeName), linked to the new lights."
                    : "Adds a \(shapeName) with the same strings and lights, linked in wiring order. Move or resize it in the preview.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
