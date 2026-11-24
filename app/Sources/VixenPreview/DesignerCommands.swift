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
        // Only with a designer in front: File is the Administration window's menu otherwise, and a sequencer's Save is in its Sequence menu.
        CommandGroup(after: .newItem) {
            if let model = registry.active {
                Button("Save") { model.save() }
                    .keyboardShortcut("s", modifiers: .command)
                Divider()
                Button("Import Prop") { model.importProp() }
            }
        }
        // View, Settings and Help: only with a designer in front, so the other windows' menus keep to their own items.
        CommandGroup(after: .toolbar) {
            if let model = registry.active {
                Toggle("Prop Information", isOn: Binding(get: { model.showInfo }, set: { model.showInfo = $0 }))
                Button("Prop Editor") { openWindow(id: WindowID.customPropEditor, value: UUID()) }
                Toggle("Elements", isOn: Binding(get: { model.showElements }, set: { model.showElements = $0 }))
                Toggle("Properties", isOn: Binding(get: { model.showProperties }, set: { model.showProperties = $0 }))
                // F80: upstream's panels could be dragged out of the window. The floating panel's close button docks it again.
                Toggle("Float Elements Panel", isOn: Binding(get: { model.elementsFloating }, set: { model.setFloating(.elements, $0) }))
                Toggle("Float Properties Panel", isOn: Binding(get: { model.propertiesFloating }, set: { model.setFloating(.properties, $0) }))
            }
        }
        // Not after .pasteboard: the sequencer replaces that group, and SwiftUI then drops other scenes' groups placed after it.
        CommandGroup(before: .textEditing) {
            DesignerEditItems(model: registry.active)
        }
        CommandMenu("Settings") {
            if let model = registry.active {
                DesignerSettingsItems(model: model)
            }
        }
        CommandGroup(after: .windowArrangement) {
            // F70: the way back for a window that may not be right-clickable any more.
            Button("Turn Off Preview Transparency") { LivePreviewCoordinator.shared?.turnOffTransparency() }
                .disabled(LivePreviewCoordinator.shared?.anyTransparent != true)
        }
        // The Administration window's Help, always there, has upstream's Vixen YouTube Channel item.
        CommandGroup(after: .help) {
            if let model = registry.active {
                Button("View Help") { model.help("https://www.vixenlights.com/docs/usage/preview/preview-main-screen/") }
                    .keyboardShortcut("?", modifiers: .command)
                Button("Template Help") { model.help("https://www.vixenlights.com/docs/") }
                // Q3: no handler upstream, kept until the owner decides.
                Button("Custom Prop Help") {}
            }
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
        // STUDIO-BEGIN designer-add-menu: Edit ▸ Add names the Single Light tool and places one linked bulb per selected light (B60)
        StudioDesignerAddMenu(model: model)
        // STUDIO-END designer-add-menu
        Button("Templates (Legacy)") { model.sheet = .templates }
    }
}

private struct DesignerSettingsItems: View {
    let model: PreviewDesignerModel

    var body: some View {
        let settings = model.settings
        Toggle("Update Location Properties", isOn: Binding(get: { settings?.saveLocations ?? true }, set: { on in
            model.designer?.setSaveLocations(on: on)
            model.refreshSettings()
        }))
        .help("Update location properties for each preview item for positioning.")
        Button("Location Offset Setup") { model.sheet = .locationOffset }
        Toggle("Use OpenGL Preview", isOn: Binding(get: { settings?.useOpenGl ?? false }, set: { on in
            model.designer?.setUseOpenGl(on: on)
            model.refreshSettings()
        }))
        Toggle("Keep Insert Mode Active", isOn: Binding(get: { settings?.keepInsertMode ?? false }, set: { on in
            model.designer?.setKeepInsertMode(on: on)
            model.refreshSettings()
        }))
        .help("Stay in insert mode after inserting each Prop.")
    }
}
