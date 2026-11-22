import AppKit
import SwiftUI
import VixenBridge
import VixenKit

// Edit ▸ Effect Gallery… (#62): looping thumbnails of the chosen effects on the selected row's prop, drawn from its own
// preview geometry by the effect_gallery code. Clicking one places it at the cursor on the selected rows, one step on the
// window's Undo, through the same effect_add the MCP tool uses.

extension SequencerModel {
    func openEffectGallery() {
        sheet = .effectGallery
    }
}

struct EffectGallerySheet: View {
    let model: SequencerModel

    /// Effects that read differently on a prop, chosen to start with when the catalog has them.
    private static let starters = ["Chase", "Spin", "Wipe", "Butterfly", "Twinkle", "Meteors", "Snowflakes", "Plasma", "Curtain", "Bars", "Spirals", "Fire"]

    private struct Thumbnail: Identifiable {
        let id: Int
        let label: String
        let typeId: String
        let frames: [NSImage]
        let dark: Bool
    }

    @State private var targets: [(id: String, name: String)] = []
    @State private var chosen: Set<String> = []
    @State private var seconds = 2.0
    @State private var thumbnails: [Thumbnail] = []
    @State private var previewName = ""
    @State private var problem: String?
    @State private var rendering = false
    @State private var placing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Effect Gallery")
                .font(.headline)
            if targets.isEmpty {
                ContentUnavailableView(
                    "No Rows Selected",
                    systemImage: "rectangle.stack",
                    description: Text("Select the row to try effects on in the timeline, then choose Edit ▸ Effect Gallery… again.")
                )
                .frame(minHeight: 160)
            } else {
                HStack(alignment: .top, spacing: 16) {
                    picker
                    gallery
                }
                footer
            }
            HStack {
                Spacer()
                Button("Done") { model.sheet = nil }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 820)
        .task { load() }
        .task(id: request) { await render() }
    }

    private var picker: some View {
        VStack(alignment: .leading, spacing: 8) {
            List(model.effects, id: \.typeId) { effect in
                Toggle(effect.name, isOn: Binding(
                    get: { chosen.contains(effect.typeId) },
                    set: { on in
                        if on { chosen.insert(effect.typeId) } else { chosen.remove(effect.typeId) }
                    }
                ))
                .disabled(!chosen.contains(effect.typeId) && chosen.count >= 24)
            }
            .frame(width: 200, height: 420)
            LabeledContent("Seconds") {
                Stepper(value: $seconds, in: 0.5...10, step: 0.5) {
                    Text(seconds, format: .number.precision(.fractionLength(1)))
                        .monospacedDigit()
                }
            }
            .frame(width: 200)
            .help("How long each effect plays in its thumbnail, and how long it lasts when placed.")
        }
    }

    private var gallery: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 12)], spacing: 12) {
                ForEach(thumbnails) { thumbnail in
                    Button { place(thumbnail) } label: {
                        VStack(spacing: 4) {
                            TimelineView(.animation(minimumInterval: 1.0 / 10)) { context in
                                let frames = thumbnail.frames
                                let step = Int(context.date.timeIntervalSinceReferenceDate * Double(frames.count) / seconds)
                                Image(nsImage: frames.isEmpty ? NSImage() : frames[step % frames.count])
                                    .interpolation(.high)
                                    .resizable()
                                    .aspectRatio(contentMode: .fit)
                            }
                            .frame(height: 150)
                            .frame(maxWidth: .infinity)
                            .background(.black, in: RoundedRectangle(cornerRadius: 6))
                            Text(thumbnail.label)
                                .font(.caption)
                                .lineLimit(1)
                            if thumbnail.dark {
                                Text("Lit nothing")
                                    .font(.caption2)
                                    .foregroundStyle(.orange)
                            }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(placing)
                    .help("Place \(thumbnail.label) at the cursor on \(targets.map(\.name).joined(separator: ", "))")
                    .accessibilityLabel("\(thumbnail.label)\(thumbnail.dark ? ", lit nothing" : "")")
                }
            }
            .padding(2)
            .overlay {
                if thumbnails.isEmpty && !rendering {
                    ContentUnavailableView("Choose Effects", systemImage: "sparkles", description: Text("Turn on the effects to compare in the list."))
                }
            }
        }
        .frame(height: 470)
    }

    @ViewBuilder private var footer: some View {
        if let problem {
            Label(problem, systemImage: "exclamationmark.triangle")
                .foregroundStyle(.orange)
                .fixedSize(horizontal: false, vertical: true)
        } else if rendering {
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text("Rendering on \(targets[0].name)…")
                    .foregroundStyle(.secondary)
            }
        } else if !thumbnails.isEmpty {
            Label("On \(targets[0].name) in \(previewName). Click one to place it at the cursor\(targets.count > 1 ? " on the \(targets.count) selected rows" : "").", systemImage: "hand.tap")
                .foregroundStyle(.secondary)
        } else {
            Text(" ")
        }
    }

    private func load() {
        targets = model.placeOnMarksTargets
        let names = Set(Self.starters)
        chosen = Set(model.effects.filter { names.contains($0.name) }.map(\.typeId))
    }

    /// What to render: the first selected row and the chosen effects in catalog order; nil when there is nothing to show.
    private var request: String? {
        guard let target = targets.first else { return nil }
        let variants = model.effects.filter { chosen.contains($0.typeId) }.map { ["typeId": $0.typeId] }
        guard !variants.isEmpty else { return nil }
        let args: [String: Any] = ["targetId": target.id, "variants": variants, "durationMs": seconds * 1000]
        guard let data = try? JSONSerialization.data(withJSONObject: args, options: [.sortedKeys]) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private func render() async {
        guard let session = model.session, let json = request else {
            thumbnails = []
            problem = nil
            return
        }
        try? await Task.sleep(for: .milliseconds(250))
        guard !Task.isCancelled else { return }
        rendering = true
        let outcome = await Task.detached { () -> Result<StudioGallery, Error> in
            Result { try session.studioEffectGallery(argumentsJson: json, frames: 12, tileWidth: 240) }
        }.value
        guard !Task.isCancelled else { return }
        rendering = false
        switch outcome {
        case .success(let gallery):
            previewName = gallery.previewName
            thumbnails = gallery.variants.enumerated().map { i, v in
                Thumbnail(id: i, label: v.label, typeId: v.typeId, frames: v.frames.compactMap { NSImage(data: $0) }, dark: v.illuminated == 0)
            }
            problem = nil
        case .failure(let error):
            thumbnails = []
            problem = placeOnMarksMessage(error)
        }
    }

    /// effect_add at the cursor on every selected row, shortened to fit when the cursor is near the end.
    private func place(_ thumbnail: Thumbnail) {
        guard let session = model.session, let shell = model.shell else { return }
        let length = shell.lengthMs()
        let start = min(max(0, shell.cursorMs()), max(0, length - 1))
        let duration = min(seconds * 1000, length - start)
        let args: [String: Any] = ["typeId": thumbnail.typeId, "targetIds": targets.map(\.id), "startMs": start, "durationMs": duration]
        guard let data = try? JSONSerialization.data(withJSONObject: args), let json = String(data: data, encoding: .utf8) else { return }
        placing = true
        Task {
            let outcome = await Task.detached { () -> Result<String, Error> in
                Result { try session.studioCommand(command: "effect_add", argumentsJson: json) }
            }.value
            placing = false
            switch outcome {
            case .success:
                model.sheet = nil
                model.refresh()
            case .failure(let error):
                problem = placeOnMarksMessage(error)
            }
        }
    }
}
