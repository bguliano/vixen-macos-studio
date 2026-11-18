import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Studio's effects library: Vixen's effect tree with search and a short how-to. Choosing an effect switches to Draw
/// mode, so the next drag on a row paints it; choosing it again returns to Select. Dragging an effect onto a row places
/// it in either mode. Replaces `EffectsPanel`.
struct StudioEffectsLibrary: View {
    let model: SequencerModel
    @State private var expanded: Set<ShellEffectGroup> = [.basic, .pixel]
    @State private var search = ""
    @State private var notice: String?

    private static let groups: [(ShellEffectGroup, String)] = [
        (.basic, "Basic Lighting"),
        (.pixel, "Pixel Lighting"),
        (.device, "Device Action"),
        (.intelligentFixture, "Intelligent Fixture"),
    ]

    var body: some View {
        VStack(spacing: 0) {
            TextField("Search effects", text: $search)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, StudioMetrics.spacingS)
                .padding(.vertical, StudioMetrics.spacingS - 2)
            if visibleGroups.isEmpty {
                ContentUnavailableView.search(text: search)
            } else {
                List {
                    ForEach(visibleGroups, id: \.0) { group, title, effects in
                        DisclosureGroup(isExpanded: expansion(group)) {
                            ForEach(effects, id: \.typeId) { effect in
                                StudioEffectRow(model: model, effect: effect) { choose(effect, group: group) }
                            }
                        } label: {
                            Text(title)
                                .font(.callout.weight(.semibold))
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .listStyle(.sidebar)
                .scrollContentBackground(.hidden)
                .environment(\.defaultMinListRowHeight, 24)
            }
            Divider()
            Text(notice ?? hint)
                .font(.caption)
                .foregroundStyle(notice == nil ? .secondary : .primary)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(StudioMetrics.spacingS)
                .animation(.default, value: notice)
        }
    }

    private var hint: String {
        if let chosen = model.ui?.selectedEffect, let effect = model.effects.first(where: { $0.typeId == chosen }) {
            return "Drag across a row to draw \(effect.name). Choose it again to stop drawing."
        }
        return "Drag an effect onto a row, or choose one and drag across rows to draw it."
    }

    private var visibleGroups: [(ShellEffectGroup, String, [ShellEffect])] {
        Self.groups.compactMap { group, title in
            let effects = model.effects.filter {
                $0.group == group && (search.isEmpty || $0.name.localizedCaseInsensitiveContains(search))
            }
            return effects.isEmpty ? nil : (group, title, effects)
        }
    }

    private func expansion(_ group: ShellEffectGroup) -> Binding<Bool> {
        Binding(
            get: { !search.isEmpty || expanded.contains(group) },
            set: { open in
                if open { expanded.insert(group) } else { expanded.remove(group) }
            }
        )
    }

    private func choose(_ effect: ShellEffect, group: ShellEffectGroup) {
        if group == .device {
            show("\(effect.name) can only be placed by dragging it onto a row.")
            return
        }
        let chosen = model.shell?.toggleSelectedEffect(typeId: effect.typeId) != nil
        model.refresh()
        let drawing = model.ui?.drawMode ?? false
        if chosen {
            if !drawing { model.run(.drawMode) }
        } else if drawing {
            model.run(.selectionMode)
        }
    }

    private func show(_ text: String) {
        notice = text
        Task {
            try? await Task.sleep(for: .seconds(4))
            if notice == text { notice = nil }
        }
    }
}

/// One effect, with its picture.
private struct StudioEffectRow: View {
    let model: SequencerModel
    let effect: ShellEffect
    let click: () -> Void

    private var chosen: Bool {
        model.ui?.selectedEffect == effect.typeId
    }

    var body: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            ToolIcon(image: .effect(effect.icon), size: 18)
            Text(effect.name)
                .lineLimit(1)
            Spacer(minLength: 0)
            if chosen {
                Image(systemName: "pencil")
                    .foregroundStyle(.tint)
                    .accessibilityLabel("Drawing")
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
        .padding(.horizontal, StudioMetrics.spacingXS)
        .background(chosen ? AnyShapeStyle(.tint.opacity(0.22)) : AnyShapeStyle(.clear), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        .overlay { EffectDragSource(typeId: effect.typeId, icon: effect.icon, clicked: click) }
        .help(chosen ? "Drawing \(effect.name). Click to stop." : "Drag \(effect.name) onto a row, or click to draw it")
        .accessibilityAddTraits(chosen ? [.isSelected, .isButton] : .isButton)
        .accessibilityLabel(effect.name)
    }
}
