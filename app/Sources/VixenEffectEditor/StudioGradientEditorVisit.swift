import AppKit
import VixenBridge
import VixenKit

// Studio (B27): a gradient row's editor commits each change as it's made. Every commit of one visit shares a gesture, so
// the visit is one step on the sequence's Undo however many changes it made.

extension EffectEditorModel {
    /// Opens the Color Gradient Editor on a row, committing each change at once.
    func openGradientEditor(_ rowKey: EeRowKey, gradient: GradientValue, discrete: Bool, validColors: [UInt32], title: String) {
        guard let window = hostWindow else { return }
        PendingCommits.flushAll()
        let visit = UInt64.random(in: 1...UInt64.max)
        GradientEditorDialog.present(on: window, core: core, gradient: gradient, discrete: discrete, validColors: validColors, title: title) { [weak self] edited in
            self?.commit(rowKey, .gradient(value: edited), gesture: visit)
        }
    }

    /// `studio.effectGradient` with path ["open", param] (opens that gradient row's editor) or ["state", param] (the row's
    /// stops and the window's Undo), on the effect editor's selection. Param `*` is the first gradient row.
    func registerStudioGradientHook() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.effectGradient"] = { [weak self] args in
            guard let self, args.count == 2 else { return "FAIL needs open|state and a param" }
            let rows = sections.flatMap(\.rows).filter { if case .gradient = $0.control { true } else { false } }
            guard let row = rows.first(where: { args[1] == "*" || $0.key.param == args[1] }),
                  case let .gradient(value, discrete, valid) = row.control else {
                return "FAIL no gradient row \(args[1]) among \(sections.flatMap(\.rows).map(\.label))"
            }
            if args[0] == "open" {
                openGradientEditor(row.key, gradient: value, discrete: discrete, validColors: valid, title: row.label)
                return "opened \(row.label)"
            }
            let undo = hostWindow?.undoManager
            let stops = value.stops.sorted { $0.position < $1.position }
                .map { String(format: "%08X@%d", $0.argb, Int(($0.position * 100).rounded())) }
                .joined(separator: " ")
            return "stops=\(stops) undo=\(undo?.canUndo == true ? undo?.undoActionName ?? "-" : "-")"
        }
    }
}
