import AppKit
import SwiftUI
import VixenBridge

/// F79: the **Curve Library** selector, as a sheet. New, Edit and Delete act on the library right away.
@MainActor
public enum CurveLibrarySelector {
    /// `onChosen` isn't called on Cancel.
    public static func present(on window: NSWindow, core: VixenCore, onChosen: @escaping (LibraryCurveItem) -> Void) {
        KitSheet.present(on: window) { dismiss in
            CurveSelectorContent(core: core) { chosen in
                dismiss()
                if let chosen {
                    onChosen(chosen)
                }
            }
        }
    }
}

private struct CurveSelectorContent: View {
    let core: VixenCore
    let onFinish: (LibraryCurveItem?) -> Void
    @State private var items: [LibraryCurveItem] = []
    @State private var selected: String?
    @State private var hostReference = WeakWindow()
    private var hostWindow: NSWindow? { hostReference.window }

    var body: some View {
        SelectorLayout(
            title: "Curve Library",
            minSize: CGSize(width: 572, height: 472),
            newTitle: "New Curve",
            editTitle: "Edit Curve",
            deleteTitle: "Delete Curve",
            hasSelection: selected != nil,
            onNew: newCurve,
            onEdit: editCurve,
            onDelete: deleteCurve,
            onOK: { onFinish(items.first { $0.name == selected }) },
            onCancel: { onFinish(nil) }
        ) {
            ForEach(items, id: \.name) { item in
                SelectorCell(name: item.name, selected: selected == item.name, image: CurveThumbnail.image(points: item.curve.points, size: CGSize(width: 68, height: 68)))
                    .onTapGesture(count: 2) { onFinish(item) }
                    .onTapGesture { selected = item.name }
            }
        }
        .background(WindowFinder(into: hostReference))
        .onAppear(perform: refill)
    }

    private func refill() {
        items = core.libraryCurves()
        selected = nil
    }

    private func newCurve() {
        guard let hostWindow else { return }
        CurveEditorDialog.askForName(on: hostWindow, core: core) { name in
            guard let name else { return }
            // Cancel in the editor leaves the new ramp in the library (KEEP, F79).
            _ = core.libraryAddCurve(name: name, curve: defaultCurveValue())
            refill()
            CurveEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: name) { refill() }
        }
    }

    private func editCurve() {
        guard let hostWindow, let selected else { return }
        CurveEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: selected) { refill() }
    }

    private func deleteCurve() {
        guard let hostWindow, let selected else { return }
        Task { @MainActor in
            let answer = await askInSheet(
                on: hostWindow,
                message: "If you delete this library curve, ALL places it is used will be unlinked and will become independent curves. Are you sure you want to continue?",
                title: "Delete library curve?",
                buttons: ["Delete Curve", "Cancel"],
                destructiveIndex: 0
            )
            if answer == 0 {
                core.libraryRemoveCurves(names: [selected])
                refill()
            }
        }
    }
}

/// F95: the **Color Gradient Library** selector.
@MainActor
public enum GradientLibrarySelector {
    public static func present(on window: NSWindow, core: VixenCore, onChosen: @escaping (LibraryGradientItem) -> Void) {
        KitSheet.present(on: window) { dismiss in
            GradientSelectorContent(core: core) { chosen in
                dismiss()
                if let chosen {
                    onChosen(chosen)
                }
            }
        }
    }
}

private struct GradientSelectorContent: View {
    let core: VixenCore
    let onFinish: (LibraryGradientItem?) -> Void
    @State private var items: [LibraryGradientItem] = []
    @State private var selected: String?
    @State private var hostReference = WeakWindow()
    private var hostWindow: NSWindow? { hostReference.window }

    var body: some View {
        SelectorLayout(
            title: "Color Gradient Library",
            minSize: CGSize(width: 574, height: 397),
            newTitle: "New Gradient",
            editTitle: "Edit Gradient",
            deleteTitle: "Delete Gradient",
            hasSelection: selected != nil,
            onNew: newGradient,
            onEdit: editGradient,
            onDelete: deleteGradient,
            onOK: { onFinish(items.first { $0.name == selected }) },
            onCancel: { onFinish(nil) }
        ) {
            ForEach(items, id: \.name) { item in
                SelectorCell(name: item.name, selected: selected == item.name, image: GradientThumbnail.image(value: item.gradient, size: CGSize(width: 68, height: 68)))
                    .onTapGesture(count: 2) { onFinish(item) }
                    .onTapGesture { selected = item.name }
            }
        }
        .background(WindowFinder(into: hostReference))
        .onAppear(perform: refill)
    }

    private func refill() {
        items = core.libraryGradients()
        selected = nil
    }

    private func newGradient() {
        guard let hostWindow else { return }
        GradientEditorDialog.askForName(on: hostWindow, core: core) { name in
            guard let name else { return }
            _ = core.libraryAddGradient(name: name, gradient: defaultGradientValue())
            refill()
            GradientEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: name) { refill() }
        }
    }

    private func editGradient() {
        guard let hostWindow, let selected else { return }
        GradientEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: selected) { refill() }
    }

    private func deleteGradient() {
        guard let hostWindow, let selected else { return }
        Task { @MainActor in
            let answer = await askInSheet(
                on: hostWindow,
                message: "If you delete this library gradient, ALL places it is used will be unlinked and will become independent gradients. Are you sure you want to continue?",
                title: "Delete library gradient?",
                buttons: ["Delete Gradient", "Cancel"],
                destructiveIndex: 0
            )
            if answer == 0 {
                core.libraryRemoveGradients(names: [selected])
                refill()
            }
        }
    }
}

private struct SelectorCell: View {
    let name: String
    let selected: Bool
    let image: NSImage

    var body: some View {
        VStack(spacing: 3) {
            Image(nsImage: image)
                .interpolation(.none)
                .frame(width: 68, height: 68)
                .overlay(Rectangle().stroke(Color(red: 136 / 255, green: 136 / 255, blue: 136 / 255), lineWidth: 2))
            Text(name)
                .font(.caption)
                .lineLimit(2)
                .multilineTextAlignment(.center)
                .frame(width: 92)
        }
        .padding(4)
        .background(selected ? Color.accentColor.opacity(0.35) : Color.clear, in: RoundedRectangle(cornerRadius: 5))
        .contentShape(Rectangle())
    }
}

private struct SelectorLayout<Cells: View>: View {
    let title: String
    let minSize: CGSize
    let newTitle: String
    let editTitle: String
    let deleteTitle: String
    let hasSelection: Bool
    let onNew: () -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void
    let onOK: () -> Void
    let onCancel: () -> Void
    @ViewBuilder let cells: () -> Cells

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 100))], spacing: 10) {
                    cells()
                }
                .padding(6)
            }
            .background(Color(nsColor: .textBackgroundColor).opacity(0.3))
            .overlay(Rectangle().stroke(Color.secondary.opacity(0.4)))
            HStack {
                Button(newTitle, action: onNew)
                Button(editTitle, action: onEdit).disabled(!hasSelection)
                Button(deleteTitle, action: onDelete).disabled(!hasSelection)
                Spacer()
                Button("Cancel", action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("OK", action: onOK)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(minWidth: minSize.width, minHeight: minSize.height)
    }
}
