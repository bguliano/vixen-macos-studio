import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Commissioning tests in the Display pane: a Test submenu on one element, group or controller runs a moving pixel, R/G/B/W
/// color bars or every light on, at a chosen brightness (25% unless changed), without a sequence. Tests need Live Output on;
/// Rust plays them (`studio_commission.rs`), the same code as the MCP `output_test_pattern` tool, and the pane shows the
/// running test with a Stop Test button until it's stopped or Live Output goes off.
extension SetupModel {
    private struct TestMenu: Decodable {
        let name: String?
        let lights: Int
        let problem: String?
        let block: String?
        let running: StudioCommissionStatus?
        let defaultBrightnessPct: Int
        let targetId: String?
        let controllerId: String?
    }

    static let studioTestBrightnessKey = "studio.testBrightness"
    private static let studioTestBrightnessChoices = [10, 25, 50, 75, 100]

    /// The Test item for the element menu: one element or group selected, in the live pane.
    func studioElementTestMenuItem() -> NSMenuItem? {
        let selection = elementTree.selectionInDisplayOrder
        guard selection.count == 1 else { return nil }
        return studioTestMenuItem(elementRow: selection[0], controllerId: nil)
    }

    /// The Test item for the controller menu: one controller selected, in the live pane.
    func studioControllerTestMenuItem() -> NSMenuItem? {
        guard let session, controllerSelection.count == 1,
              let id = session.firstControllerInfo(rows: controllerSelection)?.id else { return nil }
        return studioTestMenuItem(elementRow: nil, controllerId: id)
    }

    private func studioTestMenuItem(elementRow: String?, controllerId: String?) -> NSMenuItem? {
        let live = StudioDisplayLive.shared
        guard live.paneVisible, let session, let core = app.core, StudioDisplayLive.model === self else { return nil }
        let text = core.studioCommissionMenu(session: session, elementRow: elementRow, controllerId: controllerId)
        guard let info = try? JSONDecoder().decode(TestMenu.self, from: Data(text.utf8)) else { return nil }
        let stored = UserDefaults.standard.integer(forKey: Self.studioTestBrightnessKey)
        let brightness = stored > 0 ? stored : info.defaultBrightnessPct
        let runnable = info.block == nil && info.problem == nil

        let menu = NSMenu()
        menu.autoenablesItems = false
        if let block = info.block {
            menu.addItem(SetupMenuItem(block, enabled: false))
        } else if let problem = info.problem {
            menu.addItem(SetupMenuItem(problem, enabled: false))
        } else if let name = info.name {
            menu.addItem(SetupMenuItem("\(name): \(info.lights == 1 ? "1 light" : "\(info.lights) lights")", enabled: false))
        }
        menu.addItem(.separator())
        let patterns: [(String, String, String)] = [
            ("Moving Pixel", "movingPixel", "Light one light at a time in wire order, to find a skipped, swapped or reversed light"),
            ("Color Bars (R, G, B, W)", "colorBars", "Show red, green, blue, then white on every light, to check the color order"),
            ("All On", "allOn", "Light every light white, to check power and where the boundaries are"),
        ]
        for (title, pattern, tooltip) in patterns {
            menu.addItem(SetupMenuItem(title, enabled: runnable, tooltip: tooltip) {
                var arguments: [String: Any] = ["pattern": pattern, "brightnessPct": brightness]
                if let id = info.targetId { arguments["targetId"] = id }
                if let id = info.controllerId { arguments["controllerId"] = id }
                StudioDisplayLive.shared.startTest(arguments: arguments, core: core)
            })
        }
        menu.addItem(.separator())
        let levels = NSMenu()
        levels.autoenablesItems = false
        for level in Self.studioTestBrightnessChoices {
            let item = SetupMenuItem("\(level)%") {
                UserDefaults.standard.set(level, forKey: Self.studioTestBrightnessKey)
            }
            item.state = level == brightness ? .on : .off
            levels.addItem(item)
        }
        let levelsItem = SetupMenuItem("Brightness", tooltip: "How bright the next test runs; 25% keeps a first power-up gentle on supplies")
        levelsItem.submenu = levels
        menu.addItem(levelsItem)
        menu.addItem(SetupMenuItem("Stop Test", enabled: info.running != nil) {
            StudioDisplayLive.shared.stopTest(core: core)
        })

        let item = SetupMenuItem("Test")
        item.submenu = menu
        return item
    }
}

/// `studio_commission::status`: the test running now.
struct StudioCommissionStatus: Decodable, Equatable, Sendable {
    let title: String
    let name: String
    let lights: Int
    let brightnessPct: Int
    let remainingMs: Int?
    let agent: Bool
}

/// Above the element and controller trees while a test runs: what's lit, and the way to stop it.
struct StudioCommissionBanner: View {
    let status: StudioCommissionStatus
    let stop: () -> Void

    var body: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            Image(systemName: "lightbulb.max.fill")
                .foregroundStyle(.yellow)
                .imageScale(.large)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("Testing \(status.name): \(status.title) at \(status.brightnessPct)%")
                    .font(.headline)
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Stop Test", action: stop)
                .keyboardShortcut(.cancelAction)
        }
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
        .background(.yellow.opacity(0.1))
    }

    private var detail: String {
        var parts = [status.lights == 1 ? "1 light" : "\(status.lights) lights"]
        if status.agent { parts.append("started by an agent") }
        if let remaining = status.remainingMs {
            let seconds = (remaining + 999) / 1000
            parts.append("stops by itself in \(seconds / 60):\(String(format: "%02d", seconds % 60))")
        }
        return parts.joined(separator: " · ")
    }
}
