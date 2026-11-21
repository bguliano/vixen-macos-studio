import SwiftUI
import VixenKit

/// The designer in the show window's Previews pane (UI roadmap Phase 2 item 1). The same view and model as the Preview
/// Configuration window; the model attaches to the show window instead, so Edit ▸ Undo, the Edit actions and the menus follow
/// it while it shows, and its alerts and sheets attach there. Leaving it runs the window's close prompts and write-back.
@MainActor
enum StudioDesignerPane {
    private static weak var current: PreviewDesignerModel?

    static func install(model app: AppModel) {
        StudioPreviewDesignerPane.make = { id in AnyView(StudioDesignerPaneRoot(previewID: id).environment(app)) }
        StudioPreviewDesignerPane.leave = {
            guard let model = current else { return true }
            guard await model.studioLeave() else { return false }
            current = nil
            return true
        }
        StudioPreviewDesignerPane.flush = {
            await current?.studioFlush()
            current = nil
        }
    }

    fileprivate static func opened(_ model: PreviewDesignerModel) {
        current = model
    }
}

private struct StudioDesignerPaneRoot: View {
    let previewID: String
    @Environment(AppModel.self) private var app
    @State private var model: PreviewDesignerModel?

    var body: some View {
        Group {
            if let model {
                DesignerContent(model: model)
            } else {
                Color.black
            }
        }
        .onAppear {
            guard model == nil else { return }
            let made = PreviewDesignerModel(previewID: previewID, app: app)
            made.studioInPane = true
            StudioDesignerPane.opened(made)
            model = made
        }
    }
}
