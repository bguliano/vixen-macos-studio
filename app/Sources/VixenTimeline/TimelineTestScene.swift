import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// A window with just the timeline, for building and testing it alone. The app includes this when `VixenSequencer` isn't in the build. It opens
/// sequences the way the real window does, so Administration's New and Open both work. Under the test harness, `VIXEN_TIMELINE_SEQUENCE` names a file
/// in the profile's Sequence folder for the `timeline-test` window.
public struct TimelineTestScene: Scene {
    private let model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        WindowGroup("Timeline", id: WindowID.sequencer, for: SequenceDocumentRef.self) { $ref in
            TimelineDocumentView(ref: ref ?? .new())
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 1400, height: 900)

        Window("Timeline Test", id: "timeline-test") {
            TimelineDocumentView(ref: .fromEnvironment)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 1400, height: 900)
        .commands {
            CommandMenu("Timeline Test") {
                Button("Dump State") { TimelineTestHooks.dump() }
                Button("Run Benchmark") { TimelineTestHooks.bench() }
                Button("Select All") { TimelineTestHooks.run { $0.selectAll() } }
                Button("Clear Selection") { TimelineTestHooks.run { $0.clearSelection() } }
                Button("Undo") { TimelineTestHooks.undo() }
                Button("Redo") { TimelineTestHooks.redo() }
                Button("Save Copy") { TimelineTestHooks.save() }
                Button("Zoom In") { TimelineTestHooks.run { $0.zoomTime(scale: 0.8) } }
                Button("Zoom Out") { TimelineTestHooks.run { $0.zoomTime(scale: 1.25) } }
                Button("Zoom Rows In") { TimelineTestHooks.run { $0.zoomRows(scale: 1.25) } }
                Button("Zoom Rows Out") { TimelineTestHooks.run { $0.zoomRows(scale: 0.8) } }
                Button("Expand All") { TimelineTestHooks.expandAll() }
                Button("Scroll Right") { TimelineTestHooks.run { $0.pageTime(direction: 1) } }
                Button("Scroll Down") { TimelineTestHooks.run { $0.pageVertical(direction: 1) } }
                Button("Show Mark Bars") { TimelineTestHooks.run { $0.setAllMarkBarsVisible(visible: true) } }
                Button("Toggle Effect Info") {
                    TimelineTestHooks.settings { $0.showEffectInfo.toggle() }
                }
                Button("Toggle Highlight Rows") {
                    TimelineTestHooks.settings { $0.highlightRows.toggle() }
                }
                Button("Toggle Full Waveform") {
                    TimelineTestHooks.settings { $0.fullWaveform.toggle() }
                }
                Button("Toggle CAD Selection") {
                    TimelineTestHooks.settings { $0.cadStyle.toggle() }
                }
                Button("Toggle Resize Indicator") {
                    TimelineTestHooks.settings { $0.resizeIndicator.toggle() }
                }
                Button("Toggle Snap") {
                    TimelineTestHooks.settings { $0.snapEnabled.toggle() }
                }
                Button("Toggle Draw Mode") { TimelineTestHooks.toggleDrawMode() }
                Button("Toggle Legacy Cursor") {
                    TimelineTestHooks.settings { $0.legacyCursor.toggle() }
                }
                Button("Toggle Zoom Under Mouse") {
                    TimelineTestHooks.settings { $0.zoomUnderMouse.toggle() }
                }
                Button("Toggle Lock Ruler") {
                    TimelineTestHooks.settings { $0.lockRulerHeight.toggle() }
                }
                Button("Zoom To Total") { TimelineTestHooks.run { $0.zoomTime(scale: 1_000_000) } }
                Button("Zoom Far In") { TimelineTestHooks.zoomFarIn() }
                Button("Play") { TimelineTestHooks.play() }
                Button("Pause") { TimelineTestHooks.canvas?.controller.session().pause() }
                Button("Resume") { TimelineTestHooks.canvas?.controller.session().resume() }
                Button("Stop") { TimelineTestHooks.stop() }
                Button("Delete Marks Key") { TimelineTestHooks.run { $0.deleteMarksKey() } }
                Button("Copy Marks") { TimelineTestHooks.canvas?.controller.copyMarks() }
            }
        }
    }
}

extension SequenceDocumentRef {
    fileprivate static var fromEnvironment: SequenceDocumentRef {
        guard let name = ProcessInfo.processInfo.environment["VIXEN_TIMELINE_SEQUENCE"] else { return .new() }
        return .existing(path: name)
    }
}

private struct TimelineDocumentView: View {
    let ref: SequenceDocumentRef
    @Environment(AppModel.self) private var model
    @State private var canvas: TimelineCanvas?
    @State private var problem: String?

    var body: some View {
        Group {
            if let canvas {
                TimelinePane(canvas: canvas)
            } else if let problem {
                ContentUnavailableView("Couldn't open the sequence", systemImage: "exclamationmark.triangle", description: Text(problem))
            } else {
                ProgressView("Opening…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 640, minHeight: 480)
        .task(id: model.core != nil) {
            await open()
        }
    }

    private func open() async {
        guard canvas == nil, let core = model.core else { return }
        let ref = ref
        let root = core.dataRoot()
        do {
            let session: SequencerSession = try await Task.detached {
                switch ref {
                case .untitled:
                    return core.newSequence()
                case .existing(let path):
                    let full = path.hasPrefix("/") ? path : root + "/Sequence/" + path
                    return try core.openSequence(path: full)
                }
            }.value
            canvas = TimelineCanvas(session: session, loadProfileSettings: true)
            TimelineTestHooks.install()
            if ProcessInfo.processInfo.environment["VIXEN_TIMELINE_BENCH"] != nil {
                TimelineTestHooks.bench()
            }
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// What the test window's menu does, so a script can drive it and read the results.
@MainActor
enum TimelineTestHooks {
    static func run(_ command: (TimelineController) -> TimelineUpdate) {
        guard let canvas = TimelineCanvas.current else { return }
        canvas.apply(command(canvas.controller))
    }

    static func settings(_ change: (inout TimelineSettings) -> Void) {
        guard let canvas = TimelineCanvas.current else { return }
        var s = canvas.controller.settings()
        change(&s)
        canvas.apply(canvas.controller.applySettings(s: s))
    }

    static func undo() {
        guard let canvas = TimelineCanvas.current else { return }
        _ = canvas.controller.session().undo(steps: 1)
        canvas.apply(canvas.controller.refresh())
    }

    static func redo() {
        guard let canvas = TimelineCanvas.current else { return }
        _ = canvas.controller.session().redo(steps: 1)
        canvas.apply(canvas.controller.refresh())
    }

    static var canvas: TimelineCanvas? { TimelineCanvas.current }

    /// `{ "action": "hook", "name": "timelinePlayback" }`: the transport and what the timeline shows of it, as one line of JSON in `result.json`,
    /// so a script can read several moments in one run.
    static func install() {
        guard UITestHarness.isActive, UITestHarness.hooks["timelinePlayback"] == nil else { return }
        UITestHarness.hooks["timelineState"] = { _ in
            dump()
            return (try? String(contentsOf: outDir.appending(path: "timeline-state.json"), encoding: .utf8)) ?? "FAIL no timeline state"
        }
        UITestHarness.hooks["timelineBenchmark"] = { _ in
            guard canvas != nil else { return "FAIL no timeline" }
            bench()
            return "started"
        }
        UITestHarness.hooks["timelineEdgeLimits"] = { _ in
            // Exercise the production cache/prefetch path at the i32 paint-coordinate boundary, without changing a document.
            for width: CGFloat in [1, 512] {
                let plane = TilePlane(tileWidth: width, tileHeight: 256, timePerPixel: 100_000, opaque: true, vertical: true, horizontal: true) { _, _ in [] }
                plane.paintsViewportEdges = width == 1
                for x in [CGFloat(Int32.max) - 200, CGFloat(Int32.max) - 2, CGFloat(Int32.max) + 1] {
                    plane.setView(scrollX: x, scrollY: 0, size: CGSize(width: 1, height: 256), timePerPixel: 100_000)
                    plane.setView(scrollX: x, scrollY: 0, size: CGSize(width: 4, height: 512), timePerPixel: 50_000)
                }
                for _ in 0..<100 where !plane.isSettled { try? await Task.sleep(for: .milliseconds(10)) }
                guard plane.isSettled else { return "FAIL edge limit tiles did not settle" }
            }
            return "edge coordinates, resize and zoom passed at i32 boundary"
        }
        UITestHarness.hooks["timelinePlayback"] = { _ in
            guard let canvas = TimelineCanvas.current else { return "FAIL no timeline" }
            let c = canvas.controller
            let state: [String: Any] = [
                "transport": "\(c.session().transport())",
                "positionMs": c.session().positionMs(),
                "playbackLineX": canvas.grid.playbackLineX.map { Double($0) } ?? -1,
                "playbackRange": c.playbackRange().map { $0.map { Double($0) } ?? -1 },
                "cursor": c.view().cursor,
            ]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL" }
            return String(decoding: data, as: UTF8.self)
        }
    }

    /// Set Level, as the Effects panel would choose it.
    static func toggleDrawMode() {
        guard let canvas = TimelineCanvas.current else { return }
        var s = canvas.controller.settings()
        s.drawMode.toggle()
        canvas.apply(canvas.controller.applySettings(s: s))
        canvas.apply(canvas.controller.setSelectedEffectType(typeId: s.drawMode ? "32cff8e0-5b10-4466-a093-0d232c55aac0" : nil))
    }

    static func zoomFarIn() {
        guard let canvas = TimelineCanvas.current else { return }
        for _ in 0..<20 {
            canvas.apply(canvas.controller.zoomTime(scale: 0.8))
        }
    }

    static func play() {
        guard let canvas = TimelineCanvas.current else { return }
        canvas.controller.session().play(startMs: 0, endMs: Double(canvas.controller.view().total) / 10_000, looping: false)
    }

    static func stop() {
        TimelineCanvas.current?.controller.session().stop()
    }

    static func expandAll() {
        guard let canvas = TimelineCanvas.current else { return }
        canvas.apply(canvas.controller.expandAllRows())
    }

    static func save() {
        guard let canvas = TimelineCanvas.current else { return }
        canvas.controller.storeViewState()
        try? canvas.controller.session().save(path: outDir.appending(path: "saved.tim").path)
    }

    static var outDir: URL {
        URL(fileURLWithPath: ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory())
    }

    /// Everything a script wants to confirm, as JSON: `timeline-state.json` in the output folder.
    static func dump() {
        guard let canvas = TimelineCanvas.current else { return }
        let c = canvas.controller
        let v = c.view()
        let frame = c.rowFrame(top: 0, bottom: v.canvasHeight)
        let state: [String: Any] = [
            "view": [
                "timePerPixel": v.timePerPixel, "visibleStart": v.visibleStart, "total": v.total, "scrollX": v.scrollX, "verticalOffset": v.verticalOffset,
                "canvasWidth": v.canvasWidth, "canvasHeight": v.canvasHeight, "waveformHeight": v.waveformHeight, "rulerHeight": v.rulerHeight,
                "marksBarHeight": v.marksBarHeight, "cursor": v.cursor,
            ] as [String: Any],
            "gridSize": [canvas.grid.bounds.width, canvas.grid.bounds.height],
            "drag": c.dragStateName(),
            "guides": canvas.grid.lastGuides,
            "markSelection": c.markSelection(),
            "settings": ["drawMode": c.settings().drawMode, "snap": c.settings().snapEnabled],
            "modified": c.session().isModified(),
            "editorState": c.isEditorStateModified(),
            "marksBar": c.marksFrame(x: Int32(v.scrollX), width: Int32(canvas.grid.bounds.width), timePerPixel: v.timePerPixel).marks.map {
                ["id": $0.id, "x": $0.x, "y": $0.y, "w": $0.w, "h": $0.h, "selected": $0.selected, "text": $0.text] as [String: Any]
            },
            "transport": "\(c.session().transport())",
            "positionMs": c.session().positionMs(),
            "playbackLineX": canvas.grid.playbackLineX.map { Double($0) } ?? -1,
            "playbackRange": c.playbackRange().map { $0.map { Double($0) } ?? -1 },
            "selected": c.selectedElements(),
            "activeRow": c.activeRow().map { Int($0) } ?? -1,
            "elements": c.elementsInfo().map {
                ["id": $0.id, "start": $0.start, "duration": $0.duration, "rows": $0.rows, "selected": $0.selected, "rendered": $0.rendered, "type": $0.typeName] as [String: Any]
            },
            "marks": c.marksInfo().map {
                ["id": $0.id, "collection": $0.collection, "start": $0.start, "duration": $0.duration, "text": $0.text, "selected": $0.selected] as [String: Any]
            },
            "visibleRows": frame.rows.map { ["id": $0.id, "name": $0.name, "top": $0.top, "height": $0.height, "open": $0.open, "selected": $0.selected, "active": $0.active] as [String: Any] },
            "undo": c.session().undoDescriptions(),
            "redo": c.session().redoDescriptions(),
            "renderStatus": ["waiting": c.session().renderStatus().waiting, "failed": c.session().renderStatus().failed],
            "pictureBytes": canvas.pictures.memoryUse,
            "pictures": canvas.pictures.rasterized,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys]) else { return }
        try? data.write(to: outDir.appending(path: "timeline-state.json"))
    }

    static func bench() {
        guard let canvas = TimelineCanvas.current else { return }
        TimelineBench.start(canvas: canvas, outDir: outDir)
    }
}
