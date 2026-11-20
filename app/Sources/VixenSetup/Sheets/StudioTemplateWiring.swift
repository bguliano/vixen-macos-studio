import SwiftUI
import VixenBridge
import VixenKit

/// How a Megatree or Pixel Grid is wired: the start corner, "Reverse every other string", hidden pixels per string, and a
/// live diagram of the result. Rust works it all out (`studio_wiring.rs`), the same code that generates the elements and
/// that the MCP `element_template` tool runs, so the diagram shows what gets built.
enum StudioTemplateWiring {
    nonisolated struct Plan: Decodable, Sendable {
        let applies: Bool
        var note: String?
        var layout: String?
        var strings: Int?
        var pixels: Int?
        var hidden: Int?
        var sequence: [[Int]]?
        var firstLabel: String?
        var lastLabel: String?
        var cornerLabels: [String: String]?
        var summary: String?
        var hiddenText: String?
        var totals: String?

        var rows: Bool { layout == "rows" }

        /// (string index, runs from its Px1 end), in wire order.
        var wire: [(Int, Bool)] { (sequence ?? []).compactMap { $0.count == 2 ? ($0[0], $0[1] != 0) : nil } }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            applies = try c.decode(Bool.self, forKey: .applies)
            note = try c.decodeIfPresent(String.self, forKey: .note)
            layout = try c.decodeIfPresent(String.self, forKey: .layout)
            strings = try c.decodeIfPresent(Int.self, forKey: .strings)
            pixels = try c.decodeIfPresent(Int.self, forKey: .pixels)
            hidden = try c.decodeIfPresent(Int.self, forKey: .hidden)
            firstLabel = try c.decodeIfPresent(String.self, forKey: .firstLabel)
            lastLabel = try c.decodeIfPresent(String.self, forKey: .lastLabel)
            cornerLabels = try c.decodeIfPresent([String: String].self, forKey: .cornerLabels)
            summary = try c.decodeIfPresent(String.self, forKey: .summary)
            hiddenText = try c.decodeIfPresent(String.self, forKey: .hiddenText)
            totals = try c.decodeIfPresent(String.self, forKey: .totals)
            // Each entry is [index, forward] with a JSON bool, so decode the pair loosely.
            if var list = try? c.nestedUnkeyedContainer(forKey: .sequence) {
                var out: [[Int]] = []
                while !list.isAtEnd {
                    var pair = try list.nestedUnkeyedContainer()
                    let index = try pair.decode(Int.self)
                    let forward = try pair.decode(Bool.self)
                    out.append([index, forward ? 1 : 0])
                }
                sequence = out
            }
        }

        private enum CodingKeys: String, CodingKey {
            case applies, note, layout, strings, pixels, hidden, sequence, firstLabel, lastLabel, cornerLabels, summary, hiddenText, totals
        }
    }

    /// Each template's last hidden-pixel count, for the app's life, as the templates remember their other fields.
    @MainActor static var chosenHidden: [SetupTemplateKind: Int] = [:]

    static func supports(_ kind: SetupTemplateKind) -> Bool {
        kind == .megatree || kind == .pixelGrid
    }

    @MainActor static func hidden(for kind: SetupTemplateKind) -> UInt32 {
        supports(kind) ? UInt32(max(0, chosenHidden[kind] ?? 0)) : 0
    }

    static func plan(_ draft: SetupTemplateDraft, hidden: Int) -> Plan? {
        try? JSONDecoder().decode(Plan.self, from: Data(studioTemplateWiring(draft: draft, hiddenPixels: UInt32(max(0, hidden))).utf8))
    }
}

/// The wiring section of the Megatree and Pixel Grid sheets.
struct StudioTemplateWiringSection: View {
    @Binding var draft: SetupTemplateDraft
    @Binding var hidden: Int
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let plan = StudioTemplateWiring.plan(draft, hidden: hidden)
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 10) {
                    GroupBox {
                        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 8) {
                            GridRow {
                                corner(plan, "topLeft", .topLeft)
                                corner(plan, "topRight", .topRight)
                            }
                            GridRow {
                                corner(plan, "bottomLeft", .bottomLeft)
                                corner(plan, "bottomRight", .bottomRight)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(4)
                    } label: {
                        SheetFieldLabel("Wire Starts At", enabled: isEnabled)
                    }
                    Toggle("Reverse every other string", isOn: $draft.zigZag)
                        .toggleStyle(.checkbox)
                        .help("Zig-zag wiring: each string runs back the way the one before it came, so the wire never jumps back to the start side.")
                    HStack(spacing: 6) {
                        SheetFieldLabel("Hidden pixels per string:", enabled: isEnabled)
                        SheetNumberField("Hidden pixels per string:", value: $hidden, range: 0...1000)
                    }
                    .help("Pixels at the start (Px1 end) of every string that should stay dark, such as lights tucked under a tree's base. They get a group of their own, outside the prop.")
                }
                StudioWiringDiagram(plan: plan)
                    .frame(width: 210, height: 150)
                    .background(.black.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
                    .overlay {
                        if plan?.applies != true {
                            Text("No wiring to show").font(.caption).foregroundStyle(.white.opacity(0.5))
                        }
                    }
                    .accessibilityElement()
                    .accessibilityLabel("Wiring diagram")
                    .accessibilityValue(plan?.summary ?? plan?.note ?? "")
            }
            if let plan, plan.applies {
                VStack(alignment: .leading, spacing: 3) {
                    if let summary = plan.summary { Text(summary) }
                    if let text = plan.hiddenText { Text(text) }
                    if let totals = plan.totals { Text(totals).monospacedDigit() }
                }
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            } else if let note = plan?.note, !note.isEmpty {
                Text(note).font(.callout).foregroundStyle(.secondary)
            }
        }
    }

    private func corner(_ plan: StudioTemplateWiring.Plan?, _ key: String, _ location: SetupStartLocation) -> some View {
        SheetRadioButton(title: plan?.cornerLabels?[key] ?? key, isOn: draft.start == location) { draft.start = location }
    }
}

/// The strings as lines in wire order: a green dot where the wire starts, an arrow along each string the way the data
/// runs, the hidden pixels dimmed at each string's Px1 end, and thin links from one string's end to the next one's start.
/// One Canvas, however many strings.
struct StudioWiringDiagram: View {
    let plan: StudioTemplateWiring.Plan?

    var body: some View {
        Canvas { context, size in
            guard let plan, plan.applies, let count = plan.strings, count > 0 else { return }
            let wire = plan.wire
            let pixels = Double(plan.pixels ?? 1), hidden = Double(plan.hidden ?? 0)
            let hiddenShare = hidden > 0 ? max(0.06, hidden / (hidden + pixels)) : 0
            let inset: CGFloat = 18
            let area = CGRect(x: inset, y: 14, width: size.width - inset * 2, height: size.height - 44)
            let step = count > 1 ? (plan.rows ? area.height : area.width) / CGFloat(count - 1) : 0
            // A string's Px1 end and far end, on the drawing.
            func ends(_ index: Int) -> (CGPoint, CGPoint) {
                if plan.rows {
                    let y = count > 1 ? area.maxY - CGFloat(index) * step : area.midY
                    return (CGPoint(x: area.minX, y: y), CGPoint(x: area.maxX, y: y))
                }
                let x = count > 1 ? area.minX + CGFloat(index) * step : area.midX
                return (CGPoint(x: x, y: area.maxY), CGPoint(x: x, y: area.minY))
            }
            func lerp(_ a: CGPoint, _ b: CGPoint, _ t: Double) -> CGPoint {
                CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
            }
            let width: CGFloat = step >= 8 || count == 1 ? 2 : 1
            let lit = Color.accentColor, dark = Color.gray.opacity(0.55), link = Color.white.opacity(0.35)
            var previousEnd: CGPoint?
            for (run, (index, forward)) in wire.enumerated() {
                let (px1, far) = ends(index)
                let split = lerp(px1, far, hiddenShare)
                var visible = Path(); visible.move(to: split); visible.addLine(to: far)
                context.stroke(visible, with: .color(lit), lineWidth: width)
                if hiddenShare > 0 {
                    var tucked = Path(); tucked.move(to: px1); tucked.addLine(to: split)
                    context.stroke(tucked, with: .color(dark), style: StrokeStyle(lineWidth: width + 1, dash: [2, 2]))
                }
                let (from, to) = forward ? (px1, far) : (far, px1)
                if let previousEnd {
                    var join = Path(); join.move(to: previousEnd); join.addLine(to: from)
                    context.stroke(join, with: .color(link), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                }
                previousEnd = to
                if step >= 7 || count <= 2 || run < 4 {
                    // An arrowhead two thirds of the way along the run; packed strings show the first few runs' only.
                    let tip = lerp(from, to, 0.7)
                    let angle = atan2(to.y - from.y, to.x - from.x)
                    let size: CGFloat = min(5, max(3, step / 3))
                    var head = Path()
                    head.move(to: tip)
                    head.addLine(to: CGPoint(x: tip.x - size * cos(angle - 0.5), y: tip.y - size * sin(angle - 0.5)))
                    head.move(to: tip)
                    head.addLine(to: CGPoint(x: tip.x - size * cos(angle + 0.5), y: tip.y - size * sin(angle + 0.5)))
                    context.stroke(head, with: .color(.white), lineWidth: 1.5)
                }
            }
            if let (first, forward) = wire.first {
                let (px1, far) = ends(first)
                let start = forward ? px1 : far
                context.fill(Path(ellipseIn: CGRect(x: start.x - 4, y: start.y - 4, width: 8, height: 8)), with: .color(.green))
            }
            let font = Font.system(size: 9, weight: .medium)
            if let label = plan.firstLabel {
                let (px1, _) = ends(0)
                let point = plan.rows ? CGPoint(x: px1.x - 10, y: px1.y) : CGPoint(x: px1.x, y: px1.y + 10)
                context.draw(Text(label).font(font).foregroundStyle(.white.opacity(0.8)), at: point)
            }
            if count > 1, let label = plan.lastLabel {
                let (px1, _) = ends(count - 1)
                let point = plan.rows ? CGPoint(x: px1.x - 10, y: px1.y) : CGPoint(x: px1.x, y: px1.y + 10)
                context.draw(Text(label).font(font).foregroundStyle(.white.opacity(0.8)), at: point)
            }
            let legend = Text("●").foregroundStyle(.green) + Text(hidden > 0 ? " start    ┅ hidden" : " start").foregroundStyle(.white.opacity(0.6))
            context.draw(
                legend.font(font),
                at: CGPoint(x: size.width / 2, y: size.height - 7)
            )
        }
    }
}
