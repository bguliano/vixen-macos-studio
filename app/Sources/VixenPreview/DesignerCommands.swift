import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The menus. Items in File, View, Edit, Help, and a Settings menu of their own, because these are per preview and not app preferences. They act on the designer whose
/// window is key. Undo, Redo, Cut, Copy, Paste, Delete and Select All are the standard Edit items, which reach `DesignerResponder` down the responder chain.
struct PreviewDesignerCommands: Commands {
    let registry: DesignerRegistry
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(after: .newItem) {
            let model = registry.active
            Button("Save") { model?.save() }
                .keyboardShortcut(model == nil ? nil : KeyboardShortcut("s", modifiers: .command))
                .disabled(model == nil)
            Divider()
            Button("Import Prop") { model?.importProp() }
                .disabled(model == nil)
        }
        CommandGroup(after: .toolbar) {
            let model = registry.active
            Toggle("Prop Information", isOn: Binding(get: { model?.showInfo ?? false }, set: { model?.showInfo = $0 }))
                .disabled(model == nil)
            Button("Prop Editor") { openWindow(id: WindowID.customPropEditor, value: UUID()) }
                .disabled(model == nil)
            Toggle("Elements", isOn: Binding(get: { model?.showElements ?? true }, set: { model?.showElements = $0 }))
                .disabled(model == nil)
            Toggle("Properties", isOn: Binding(get: { model?.showProperties ?? true }, set: { model?.showProperties = $0 }))
                .disabled(model == nil)
            // F80: upstream's panels could be dragged out of the window. The floating panel's close button docks it again.
            Toggle("Float Elements Panel", isOn: Binding(get: { model?.elementsFloating ?? false }, set: { model?.setFloating(.elements, $0) }))
                .disabled(model == nil)
            Toggle("Float Properties Panel", isOn: Binding(get: { model?.propertiesFloating ?? false }, set: { model?.setFloating(.properties, $0) }))
                .disabled(model == nil)
        }
        // Not after .pasteboard: the sequencer replaces that group, and SwiftUI then drops other scenes' groups placed after it.
        CommandGroup(before: .textEditing) {
            DesignerEditItems(model: registry.active)
        }
        CommandMenu("Settings") {
            DesignerSettingsItems(model: registry.active)
        }
        CommandGroup(after: .windowArrangement) {
            // F70: the way back for a window that may not be right-clickable any more.
            Button("Turn Off Preview Transparency") { LivePreviewCoordinator.shared?.turnOffTransparency() }
                .disabled(LivePreviewCoordinator.shared?.anyTransparent != true)
        }
        CommandGroup(after: .help) {
            let model = registry.active
            Button("View Help") { model?.help("https://www.vixenlights.com/docs/usage/preview/preview-main-screen/") }
                .keyboardShortcut(model == nil ? nil : KeyboardShortcut("?", modifiers: .command))
                .disabled(model == nil)
            Button("Vixen YouTube Channel") { model?.help("https://www.youtube.com/user/VixenLightsSoftware") }
                .disabled(model == nil)
            Button("Template Help") { model?.help("https://www.vixenlights.com/docs/") }
                .disabled(model == nil)
            // Q3: no handler upstream, kept until the owner decides.
            Button("Custom Prop Help") {}
                .disabled(model == nil)
        }
    }
}

private struct DesignerEditItems: View {
    let model: PreviewDesignerModel?

    var body: some View {
        // Only while a designer is in front, so the other windows' Edit menus keep to their own items.
        if let model {
            items(model)
        }
    }

    @ViewBuilder
    private func items(_ model: PreviewDesignerModel) -> some View {
        let t = model.toolbar
        // The group before Find starts with its own separator.
        Button("Lock") { model.apply(model.designer?.lock()) }
            .keyboardShortcut("l", modifiers: .command)
            .disabled(!t.lock)
        Button("Unlock") { model.apply(model.designer?.unlock()) }
            .keyboardShortcut("u", modifiers: .command)
            .disabled(!t.unlock)
        // ⌘N is File > New on the Mac.
        Button("Unlock All") { model.apply(model.designer?.unlockAll()) }
            .keyboardShortcut("u", modifiers: [.command, .option])
            .disabled(!t.unlockAll)
        Button(t.hideLockedActive ? "Show Locked" : "Hide Locked") { model.toggleHideLocked() }
            .disabled(!t.hideLockedEnabled)
        Divider()
        Button("Background Properties...") { model.sheet = .resizeBackground }
        Divider()
        Button("Templates (Legacy)") { model.sheet = .templates }
    }
}

private struct DesignerSettingsItems: View {
    let model: PreviewDesignerModel?

    var body: some View {
        let settings = model?.settings
        Toggle("Update Location Properties", isOn: Binding(get: { settings?.saveLocations ?? true }, set: { on in
            model?.designer?.setSaveLocations(on: on)
            model?.refreshSettings()
        }))
        .help("Update location properties for each preview item for positioning.")
        .disabled(model == nil)
        Button("Location Offset Setup") { model?.sheet = .locationOffset }
            .disabled(model == nil)
        Toggle("Use OpenGL Preview", isOn: Binding(get: { settings?.useOpenGl ?? false }, set: { on in
            model?.designer?.setUseOpenGl(on: on)
            model?.refreshSettings()
        }))
        .disabled(model == nil)
        Toggle("Keep Insert Mode Active", isOn: Binding(get: { settings?.keepInsertMode ?? false }, set: { on in
            model?.designer?.setKeepInsertMode(on: on)
            model?.refreshSettings()
        }))
        .help("Stay in insert mode after inserting each Prop.")
        .disabled(model == nil)
    }
}
