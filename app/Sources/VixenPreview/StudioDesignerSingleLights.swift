import SwiftUI
import VixenBridge
import VixenKit

/// Edit ▸ Add in the preview designer (B60): the Single Light tool by name, and Bulbs for Selected Elements, which places one Single
/// Light per light of the Elements list's selection, each linked to its light and named after it, as one Undo step. Drawing with the
/// tool while several elements or a group are selected does the same from where the mouse goes down. Rust places them
/// (`studio_single_lights` in vixen-preview), the code the MCP `preview_prop_create` kind single runs.
struct StudioDesignerAddMenu: View {
    let model: PreviewDesignerModel

    static let singleLightHelp = "Single Light: click to place one bulb. With several elements or a group selected in Elements, places one bulb per light, already linked"

    var body: some View {
        Menu("Add") {
            Button("Single Light") { model.choose(.single) }
                .help(Self.singleLightHelp)
            Button("Bulbs for Selected Elements") { model.apply(model.designer?.studioPlaceSingleLights()) }
                .help("One linked Single Light per light selected in Elements, for smart bulbs, lamps and other single lights")
        }
        Divider()
    }
}
