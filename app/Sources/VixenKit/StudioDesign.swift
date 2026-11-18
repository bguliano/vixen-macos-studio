import AppKit
import SwiftUI

/// Studio's design system: spacing, shapes, appearance and the small shared pieces every window uses.
/// Prefer system styles first; these fill only the gaps. See docs/UI_GUIDE.md.
public enum StudioMetrics {
    public static let spacingXS: CGFloat = 4
    public static let spacingS: CGFloat = 8
    public static let spacingM: CGFloat = 12
    public static let spacingL: CGFloat = 16
    public static let spacingXL: CGFloat = 24
    public static let cornerRadius: CGFloat = 12
    public static let smallCornerRadius: CGFloat = 8
}

/// The appearance the user picked in Settings. `system` follows macOS.
public enum StudioAppearance: String, CaseIterable, Identifiable, Sendable {
    case system, light, dark
    public var id: Self { self }
    public static let storageKey = "studio.appearance"

    public var title: String {
        switch self {
        case .system: "Match System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    public var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

private struct StudioAppearanceModifier: ViewModifier {
    @AppStorage(StudioAppearance.storageKey) private var appearance = StudioAppearance.system.rawValue

    func body(content: Content) -> some View {
        content.preferredColorScheme(StudioAppearance(rawValue: appearance)?.colorScheme)
    }
}

extension View {
    /// Applies the user's chosen appearance (Settings ▸ Appearance). Put it on every window's root view.
    public func studioAppearance() -> some View {
        modifier(StudioAppearanceModifier())
    }

    /// Always dark, for canvases that show light against black: the timeline, previews and waveforms.
    public func studioCanvasAppearance() -> some View {
        environment(\.colorScheme, .dark)
    }

    /// A rounded, softly filled container for grouped content outside a `Form`.
    public func studioCard(padding: CGFloat = StudioMetrics.spacingM) -> some View {
        self.padding(padding)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous)
                    .strokeBorder(.separator.opacity(0.6), lineWidth: 0.5)
            )
    }
}

extension NSView {
    /// Keeps an AppKit canvas dark whatever the app's appearance.
    public func studioUseCanvasAppearance() {
        appearance = NSAppearance(named: .darkAqua)
    }
}

/// A compact status indicator: a coloured dot, or symbol, and a label. Used in toolbars and status rows.
public struct StudioStatusBadge: View {
    public enum Tone: Sendable { case neutral, active, warning, error }

    private let title: String
    private let systemImage: String?
    private let tone: Tone

    public init(_ title: String, systemImage: String? = nil, tone: Tone = .neutral) {
        self.title = title
        self.systemImage = systemImage
        self.tone = tone
    }

    public var body: some View {
        HStack(spacing: 6) {
            if let systemImage {
                Image(systemName: systemImage)
                    .foregroundStyle(color)
                    .imageScale(.small)
            } else {
                Circle()
                    .fill(color)
                    .frame(width: 7, height: 7)
            }
            Text(title)
                .lineLimit(1)
        }
        .font(.callout)
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .background(color.opacity(0.14), in: Capsule())
        .accessibilityElement(children: .combine)
    }

    private var color: Color {
        switch tone {
        case .neutral: .secondary
        case .active: .green
        case .warning: .orange
        case .error: .red
        }
    }
}

/// Formats a length in milliseconds for people: `2:36`, or `1:02:03` past an hour.
public func studioFormatDuration(ms: Double) -> String {
    let total = Int((ms / 1000).rounded())
    let hours = total / 3600, minutes = (total % 3600) / 60, seconds = total % 60
    return hours > 0 ? String(format: "%d:%02d:%02d", hours, minutes, seconds) : String(format: "%d:%02d", minutes, seconds)
}

/// A sidebar/detail window layout: `NavigationSplitView`, except in UI test snapshots, where the glass sidebar can't
/// be captured; there it lays the two columns out side by side on an opaque background so their content can be checked.
public struct StudioSplitView<Sidebar: View, Detail: View>: View {
    private let sidebar: Sidebar
    private let detail: Detail

    public init(@ViewBuilder sidebar: () -> Sidebar, @ViewBuilder detail: () -> Detail) {
        self.sidebar = sidebar()
        self.detail = detail()
    }

    public var body: some View {
        if UITestHarness.isActive, ProcessInfo.processInfo.environment["VIXEN_UI_TEST_ONSCREEN"] == nil {
            HStack(spacing: 0) {
                sidebar
                    .frame(width: 220)
                    .background(Color(nsColor: .underPageBackgroundColor))
                Divider()
                NavigationStack { detail }
            }
        } else {
            NavigationSplitView {
                sidebar
                    .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 300)
            } detail: {
                detail
            }
        }
    }
}
