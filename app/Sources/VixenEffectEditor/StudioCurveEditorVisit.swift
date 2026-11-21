import AppKit
import VixenBridge
import VixenKit

// Studio (B24): a curve row's editor commits each change as it's made. Every commit of one visit shares a gesture, so the
// visit is one step on the sequence's Undo however many changes it made.

extension EffectEditorModel {
    /// Opens the curve editor on a row, committing each change at once.
    func openCurveEditor(_ rowKey: EeRowKey, curve: CurveValue, title: String) {
        guard let window = hostWindow else { return }
        PendingCommits.flushAll()
        let visit = UInt64.random(in: 1...UInt64.max)
        CurveEditorDialog.present(on: window, core: core, curve: curve, title: title) { [weak self] edited in
            self?.commit(rowKey, .curve(value: edited), gesture: visit)
        }
    }

    /// `studio.effectCurve` with path ["open", param] (opens that curve row's editor) or ["state", param] (the row's points
    /// and the window's Undo), on the effect editor's selection. Param `*` is the first curve row.
    func registerStudioCurveHook() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.effectCurve"] = { [weak self] args in
            guard let self, args.count == 2 else { return "FAIL needs open|state and a param" }
            let curveRows = sections.flatMap(\.rows).filter { if case .curve = $0.control { true } else { false } }
            guard let row = curveRows.first(where: { args[1] == "*" || $0.key.param == args[1] }), case let .curve(value) = row.control else {
                return "FAIL no curve row \(args[1]) among \(sections.flatMap(\.rows).map(\.label))"
            }
            if args[0] == "open" {
                openCurveEditor(row.key, curve: value, title: row.label)
                return "opened \(row.label)"
            }
            let undo = hostWindow?.undoManager
            let points = value.points.map { "\(Int($0.x.rounded())),\(Int($0.y.rounded()))" }.joined(separator: " ")
            return "points=\(points) undo=\(undo?.canUndo == true ? undo?.undoActionName ?? "-" : "-")"
        }
    }
}
