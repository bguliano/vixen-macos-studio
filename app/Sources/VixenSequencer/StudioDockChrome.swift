import SwiftUI
import VixenBridge
import VixenKit

extension ShellPanel {
    /// Each panel's symbol, used beside its name.
    var studioSymbol: String {
        switch self {
        case .effects: "sparkles"
        case .effectEditor: "slider.horizontal.3"
        case .colorLibrary: "paintpalette"
        case .curveLibrary: "point.topleft.down.to.point.bottomright.curvepath"
        case .gradientLibrary: "swatchpalette"
        case .marks: "flag"
        case .layers: "square.3.layers.3d"
        case .findEffects: "magnifyingglass"
        }
    }
}

/// A side panel's header: the panels docked on that side as one scrolling row of labelled tabs along the top. Right-click a tab to float
/// it in its own window or close it; panels are shown again from the toolbar or the View menu.
struct StudioPanelTabs: View {
    let tabs: [ShellPanelTab]
    let selected: ShellPanel?
    let choose: (ShellPanel) -> Void
    let detach: (ShellPanel) -> Void
    let close: (ShellPanel) -> Void

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: StudioMetrics.spacingXS) {
                    ForEach(tabs, id: \.panel) { tab in
                        tabButton(tab)
                            .id(tab.panel)
                    }
                }
                .padding(.horizontal, StudioMetrics.spacingS)
            }
            .mask(LinearGradient(stops: [.init(color: .black, location: 0.88), .init(color: .clear, location: 1)], startPoint: .leading, endPoint: .trailing))
            .onAppear { if let selected { proxy.scrollTo(selected) } }
            .onChange(of: selected) { _, new in
                if let new { withAnimation { proxy.scrollTo(new) } }
            }
        }
        .padding(.vertical, StudioMetrics.spacingS - 2)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func tabButton(_ tab: ShellPanelTab) -> some View {
        let isSelected = tab.panel == selected
        return Button {
            choose(tab.panel)
        } label: {
            Label(tab.title, systemImage: tab.panel.studioSymbol)
                .labelStyle(.titleAndIcon)
                .font(.callout.weight(isSelected ? .semibold : .regular))
                .lineLimit(1)
                .padding(.horizontal, 9)
                .padding(.vertical, 4)
                .foregroundStyle(isSelected ? Color.primary : Color.secondary)
                .background {
                    if isSelected {
                        Capsule().fill(.quaternary)
                    }
                }
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .help(tab.title)
        .contextMenu {
            Button("Float in Window") { detach(tab.panel) }
            Button("Close Panel") { close(tab.panel) }
        }
    }
}

/// The sequencer's toolbar shows labels under its icons by default, so no control is icon-only. Applied once; after
/// that the user's own choice (View ▸ Customize Toolbar…) is kept.
enum StudioToolbarDefaults {
    private static let key = "studio.sequencerToolbarDefaulted"

    static func applyOnce(to window: NSWindow) {
        guard !UserDefaults.standard.bool(forKey: key) else { return }
        DispatchQueue.main.async {
            guard let toolbar = window.toolbar else { return }
            toolbar.displayMode = .iconAndLabel
            UserDefaults.standard.set(true, forKey: key)
        }
    }
}
