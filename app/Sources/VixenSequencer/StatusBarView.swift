import SwiftUI
import VixenBridge
import VixenKit

/// `statusStrip`, F256: Current Position, Sequence Length, Play Delayed (hidden unless a delay is set), the status message taking all spare width, then Rendering Elements with
/// its bar (hidden unless rendering).
struct StatusBarView: View {
    let model: SequencerModel

    var body: some View {
        HStack(spacing: 6) {
            // STUDIO-BEGIN status-bar: position and length live in the toolbar clock; the status bar keeps messages and progress
            if let transport = model.ui?.transport, transport.delayVisible {
                Text("Play Delayed:")
                Text(transport.delayText)
                    .monospacedDigit()
                    .accessibilityIdentifier("toolStripStatusLabel_delayPlay")
                Divider().frame(height: 14)
            }
            Text(model.loadingText ?? model.ui?.statusMessage ?? "")
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("toolStripStatusLabel4")
            if let ui = model.ui, ui.renderingVisible {
                Text("Rendering effects…")
                    .foregroundStyle(.secondary)
                ProgressView(value: Double(max(0, min(100, ui.renderingPercent))), total: 100)
                    .progressViewStyle(.linear)
                    .frame(width: 120)
                    .accessibilityIdentifier("toolStripProgressBar_RenderingElements")
            }
            // STUDIO-END status-bar
        }
        .font(.callout)
        .lineLimit(1)
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .frame(minHeight: 22) // STUDIO: keeps its height when the message is empty
        .background(.bar)
    }
}

/// Its own view, so a position changing at display rate redraws only this.
private struct PositionField: View {
    let model: SequencerModel

    var body: some View {
        Text("Current Position:")
        Text(model.positionText)
            .monospacedDigit()
            .frame(minWidth: 56, alignment: .leading)
            .accessibilityIdentifier("toolStripStatusLabel_currentTime")
    }
}
