import AppKit
import QuartzCore
import SwiftUI

/// A view whose layer is a `CAMetalLayer` for the Rust renderer to draw into. Swift only forwards size changes.
public final class MetalLayerNSView: NSView {
    public var onResize: ((_ widthPx: UInt32, _ heightPx: UInt32, _ scale: Float) -> Void)?

    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public override func makeBackingLayer() -> CALayer {
        let layer = CAMetalLayer()
        layer.isOpaque = true
        return layer
    }

    public var metalLayer: CAMetalLayer? {
        layer as? CAMetalLayer
    }

    /// The layer's address, as the Rust side takes it.
    public var layerAddress: UInt64 {
        guard let layer else { return 0 }
        return UInt64(UInt(bitPattern: Unmanaged.passUnretained(layer).toOpaque()))
    }

    public var scale: Float {
        Float(window?.backingScaleFactor ?? 2)
    }

    public var pixelSize: (width: UInt32, height: UInt32) {
        let backing = convertToBacking(bounds.size)
        return (UInt32(max(backing.width, 1)), UInt32(max(backing.height, 1)))
    }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        reportSize()
    }

    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        metalLayer?.contentsScale = window?.backingScaleFactor ?? 2
        reportSize()
    }

    private func reportSize() {
        let size = pixelSize
        onResize?(size.width, size.height, scale)
    }
}

/// `MetalLayerNSView` in SwiftUI. `onReady` runs once the view is in a window, when its layer and scale are final.
public struct MetalLayerView: NSViewRepresentable {
    public let onReady: (MetalLayerNSView) -> Void

    public init(onReady: @escaping (MetalLayerNSView) -> Void) {
        self.onReady = onReady
    }

    public func makeNSView(context: Context) -> MetalLayerNSView {
        let view = MetalLayerNSView(frame: .init(x: 0, y: 0, width: 400, height: 300))
        view.studioUseCanvasAppearance() // STUDIO: previews stay dark in light mode
        let ready = onReady
        DispatchQueue.main.async {
            view.metalLayer?.contentsScale = view.window?.backingScaleFactor ?? 2
            ready(view)
        }
        return view
    }

    public func updateNSView(_ view: MetalLayerNSView, context: Context) {}
}
