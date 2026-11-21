import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Studio's effects library: Vixen's effect tree with search and a short how-to, under the user's Favorites and Recently
/// Used (`effects_library`, UI roadmap R38). Choosing an effect switches to Draw mode, so the next drag on a row paints
/// it; choosing it again returns to Select. Dragging an effect onto a row places it in either mode, and the star on a
/// row pins it to Favorites. Replaces `EffectsPanel`.
struct StudioEffectsLibrary: View {
    let model: SequencerModel
    @State private var expanded: Set<String> = [Self.favoritesKey, Self.recentKey, "Basic Lighting", "Pixel Lighting"]
    @State private var search = ""
    @State private var notice: String?
    @State private var library = StudioEffectsLibraryState()

    private static let favoritesKey = "Favorites"
    private static let recentKey = "Recently Used"
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
                    if search.isEmpty {
                        DisclosureGroup(isExpanded: expansion(Self.favoritesKey)) {
                            if favorites.isEmpty {
                                Text("Click an effect's star to pin it here.")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(3)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            rows(favorites)
                        } label: {
                            header(Self.favoritesKey)
                        }
                        if !recent.isEmpty {
                            DisclosureGroup(isExpanded: expansion(Self.recentKey)) {
                                rows(recent)
                            } label: {
                                HStack {
                                    header(Self.recentKey)
                                    Spacer(minLength: 0)
                                    Button("Clear") { change(["clearRecent": true]) }
                                        .buttonStyle(.borderless)
                                        .font(.caption)
                                        .help("Empty Recently Used")
                                        .accessibilityLabel("Clear Recently Used")
                                }
                            }
                        }
                    }
                    ForEach(visibleGroups, id: \.0) { _, title, effects in
                        DisclosureGroup(isExpanded: expansion(title)) {
                            rows(effects)
                        } label: {
                            header(title)
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
        .task(id: model.studioEffectsLibraryRevision) {
            if let state = model.studioEffectsLibrary() { library = state }
        }
    }

    private func header(_ title: String) -> some View {
        Text(title)
            .font(.callout.weight(.semibold))
            .foregroundStyle(.secondary)
    }

    private func rows(_ effects: [ShellEffect]) -> some View {
        ForEach(effects, id: \.typeId) { effect in
            let favorite = library.favorites.contains { $0.typeId == effect.typeId }
            StudioEffectRow(model: model, effect: effect, favorite: favorite) {
                choose(effect)
            } toggleFavorite: {
                change(["favorite": effect.typeId, "on": !favorite])
            }
        }
    }

    private var favorites: [ShellEffect] { effects(library.favorites) }
    private var recent: [ShellEffect] { effects(library.recent) }

    /// The library's entries as the sequencer's effects, in the library's order.
    private func effects(_ entries: [StudioEffectsLibraryState.Entry]) -> [ShellEffect] {
        entries.compactMap { entry in model.effects.first { $0.typeId == entry.typeId } }
    }

    private func change(_ arguments: [String: Any]) {
        if let state = model.studioEffectsLibrary(arguments) { library = state }
        model.refreshStudioEffectsLibrary()
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

    private func expansion(_ key: String) -> Binding<Bool> {
        Binding(
            get: { !search.isEmpty || expanded.contains(key) },
            set: { open in
                if open { expanded.insert(key) } else { expanded.remove(key) }
            }
        )
    }

    private func choose(_ effect: ShellEffect) {
        if effect.group == .device {
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

/// One effect, with its picture and its Favorites star.
private struct StudioEffectRow: View {
    let model: SequencerModel
    let effect: ShellEffect
    let favorite: Bool
    let click: () -> Void
    let toggleFavorite: () -> Void

    private var chosen: Bool {
        model.ui?.selectedEffect == effect.typeId
    }

    var body: some View {
        HStack(spacing: StudioMetrics.spacingXS) {
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
            .overlay { EffectDragSource(typeId: effect.typeId, icon: effect.icon, clicked: click) }
            .help(chosen ? "Drawing \(effect.name). Click to stop." : "Drag \(effect.name) onto a row, or click to draw it")
            .accessibilityAddTraits(chosen ? [.isSelected, .isButton] : .isButton)
            .accessibilityLabel(effect.name)
            Button(action: toggleFavorite) {
                Image(systemName: favorite ? "star.fill" : "star")
                    .foregroundStyle(favorite ? AnyShapeStyle(.yellow) : AnyShapeStyle(.tertiary))
            }
            .buttonStyle(.borderless)
            .help(favorite ? "Remove \(effect.name) from Favorites" : "Add \(effect.name) to Favorites")
            .accessibilityLabel(favorite ? "Remove \(effect.name) from Favorites" : "Add \(effect.name) to Favorites")
        }
        .padding(.vertical, 2)
        .padding(.horizontal, StudioMetrics.spacingXS)
        .background(chosen ? AnyShapeStyle(.tint.opacity(0.22)) : AnyShapeStyle(.clear), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
    }
}

/// What `effects_library` returns: the user's favorites, then recents that aren't favorites.
struct StudioEffectsLibraryState: Decodable, Equatable {
    struct Entry: Decodable, Equatable {
        let typeId: String
        let name: String
    }
    var favorites: [Entry] = []
    var recent: [Entry] = []
}

extension SequencerModel {
    /// Runs `effects_library` (no arguments only reads), the same command an agent calls.
    func studioEffectsLibrary(_ arguments: [String: Any] = [:]) -> StudioEffectsLibraryState? {
        guard let core = app.core,
              let json = try? JSONSerialization.data(withJSONObject: arguments),
              let reply = try? core.studioSequenceCommand(command: "effects_library", argumentsJson: String(decoding: json, as: UTF8.self))
        else { return nil }
        return try? JSONDecoder().decode(StudioEffectsLibraryState.self, from: Data(reply.utf8))
    }

    /// Follows favorites and recents changed by an agent, a placement or another window, on the window's poll.
    func refreshStudioEffectsLibrary() {
        guard let core = app.core else { return }
        let revision = core.studioEffectsLibraryRevision()
        guard revision != studioEffectsLibraryRevision else { return }
        if studioEffectsLibraryRevision == 0, UITestHarness.isActive {
            registerEffectsLibraryHook()
        }
        studioEffectsLibraryRevision = revision
    }

    /// `sequencer.effectsLibrary`: what Favorites and Recently Used hold, as `effects_library` returns it, for the harness.
    private func registerEffectsLibraryHook() {
        UITestHarness.hooks["sequencer.effectsLibrary"] = { [weak self] _ in
            guard let self, let core = self.app.core,
                  let reply = try? core.studioSequenceCommand(command: "effects_library", argumentsJson: "{}") else { return "FAIL no library" }
            return reply
        }
    }
}
