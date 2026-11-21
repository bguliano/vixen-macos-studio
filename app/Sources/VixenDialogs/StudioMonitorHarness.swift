import AppKit
import VixenBridge
import VixenKit

/// Opens the Controller Output monitors under the UI test harness, which keeps output muted so no controller starts by
/// itself (B25). `{ "action": "hook", "name": "studio.monitor", "path": [...] }`:
/// - `open dummy|debug`: starts a Dummy Lighting or Debugging Values controller on a sample frame, adding one to the profile
///   first if it has none, and answers the title of the window that opens. Only these software controllers can start this
///   way (`test_start_monitor` refuses anything with hardware), so a run never reaches real lights.
/// - `close`: stops every controller `open` started; their windows close themselves.
enum StudioMonitorHarness {
    nonisolated(unsafe) private static var started: [String] = []

    static func installHarnessHook(model: AppModel) {
        guard UITestHarness.isActive, UITestHarness.hooks["studio.monitor"] == nil else { return }
        UITestHarness.hooks["studio.monitor"] = { [weak model] args in
            guard let context = model?.core?.dialogContext() else { return "FAIL no profile" }
            switch args.first ?? "" {
            case "open":
                let kind: DialogMonitorKind = args.count > 1 && args[1] == "debug" ? .debug : .dummy
                return await open(kind, context: context)
            case "close":
                for controller in started {
                    try? context.testStopMonitor(controller: controller)
                }
                defer { started = [] }
                return "stopped \(started.count)"
            default:
                return "FAIL unknown monitor step"
            }
        }
    }

    private static func open(_ kind: DialogMonitorKind, context: DialogContext) async -> String {
        let name = kind == .dummy ? "Dummy Lighting" : "Debugging Values"
        let controller: String
        if let existing = context.controllers().first(where: { $0.typeName.hasPrefix(name) }) {
            controller = existing.id
        } else {
            guard let type = context.testControllerTypes().map({ $0.split(separator: "|", maxSplits: 1).map(String.init) })
                .first(where: { $0.count == 2 && $0[1].hasPrefix(name) }) else { return "FAIL no \(name) module" }
            do {
                controller = try context.testAddController(typeId: type[0], name: "Harness \(name)", outputs: 64)
            } catch {
                return "FAIL \(error.userMessage)"
            }
        }
        do {
            try context.testStartMonitor(controller: controller, value: 40)
        } catch {
            return "FAIL \(error.userMessage)"
        }
        started.append(controller)
        // The monitor center notices a started controller within half a second.
        for _ in 0..<20 {
            try? await Task.sleep(for: .milliseconds(250))
            if let monitor = context.runningMonitors().first(where: { $0.controller == controller }),
               let title = await MainActor.run(body: { NSApp.windows.first { $0.isVisible && $0.title == monitor.title }?.title }) {
                return title
            }
        }
        return "FAIL no \(name) window"
    }
}
