import SwiftUI
import VixenBridge
import VixenKit

// R40: the Mark Collections panel without OK/Cancel windows. Every edit is live and one step on the sequence's Undo
// (`studio_marks_panel.rs`); these are the panel's Studio-only views.

extension MarksPanelModel {
    /// The live Offset Time field: `total` milliseconds from where the field found the marks.
    func offsetLive(_ row: MarkRowInfo, total: Int32, continuing: Bool) {
        panel.studioOffsetLive(id: row.id, totalMs: total, continuing: continuing)
        reload()
    }
}

/// Offset Time as a popover on the collection's row, in place of vanilla's number dialog: the marks move as the value changes, and one
/// Undo takes the whole adjustment back. Click away or press Escape when done.
struct StudioOffsetPopover: View {
    let model: MarksPanelModel
    let row: MarkRowInfo
    let prompt: MarkOffsetPrompt
    @State private var total: Int32 = 0
    @State private var started = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Offset “\(row.name)”").font(.headline)
            HStack {
                TextField("Offset", value: $total, format: .number.grouping(.never))
                    .monospacedDigit()
                    .frame(width: 90)
                    .labelsHidden()
                    .accessibilityLabel("Offset in milliseconds")
                Stepper("Offset", value: $total, in: prompt.minimumMs...prompt.maximumMs, step: 10)
                    .labelsHidden()
                Text("ms").foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                ForEach([-100, -10, 10, 100] as [Int32], id: \.self) { step in
                    Button(step > 0 ? "+\(step)" : "\(step)") { total = clamp(total + step) }
                        .controlSize(.small)
                        .help("Move the marks \(abs(step)) ms \(step > 0 ? "later" : "earlier")")
                }
            }
            Text("Every mark moves as you change the value. Edit ▸ Undo puts them back.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(width: 260)
        .onChange(of: total) {
            let value = clamp(total)
            if value != total {
                total = value
                return
            }
            model.offsetLive(row, total: value, continuing: started)
            started = true
        }
    }

    private func clamp(_ value: Int32) -> Int32 {
        min(max(value, prompt.minimumMs), prompt.maximumMs)
    }
}

/// The toolbar's Import as a menu of the file types, in place of vanilla's Import Type Selection dialog.
struct StudioImportMenu: View {
    let model: MarksPanelModel

    var body: some View {
        Menu {
            ForEach(ImportTypeSheet.kinds, id: \.self) { kind in
                Button(markImportLabel(kind: kind)) {
                    Task { await model.importTypeChosen(kind) }
                }
            }
        } label: {
            Image(systemName: "square.and.arrow.down")
                .frame(width: 22, height: 20)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Import")
        .accessibilityLabel("Import")
    }
}
