import AppKit
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

// Mirror of crates/gpu-monitor-tray/src/icon/render.rs for the donut geometry,
// adapted to macOS conventions for text:
//
// 1. The label text is always white. The macOS menu bar is consistently dark
//    enough (translucent over wallpaper, never plain white) that white text
//    reads everywhere — and it matches the system convention used by the
//    clock, battery, wifi, etc. We tried adapting to NSAppearance but
//    `NSStatusBarButton.effectiveAppearance` can disagree with the visible
//    bar color (it inherits from NSApp.effectiveAppearance, which is `.aqua`
//    even when the bar appears dark thanks to wallpaper translucency).
// 2. The donut colors stay saturated and convey state on their own — that's
//    where the "memory critical" signal lives, not in the text.
enum IconAppearance: Sendable {
    case dark
    case light
}

enum IconColors {
    // Donut "memory free" / "memory used" colors — same on both appearances.
    static let free   = CGColor(red: 0x66/255.0, green: 0xb3/255.0, blue: 0xff/255.0, alpha: 1.0)
    static let ok     = CGColor(red: 0x33/255.0, green: 0xb0/255.0, blue: 0x33/255.0, alpha: 1.0)
    static let warn2Donut = CGColor(red: 0xff/255.0, green: 0xa0/255.0, blue: 0x40/255.0, alpha: 1.0)
    static let warn1Donut = CGColor(red: 0xe6/255.0, green: 0xb8/255.0, blue: 0x00/255.0, alpha: 1.0)
    static let highDonut  = CGColor(red: 0xe0/255.0, green: 0x33/255.0, blue: 0x33/255.0, alpha: 1.0)

    static let dimFree = CGColor(red: 0x80/255.0, green: 0x80/255.0, blue: 0x80/255.0, alpha: 1.0)
    static let dimUsed = CGColor(red: 0x60/255.0, green: 0x60/255.0, blue: 0x60/255.0, alpha: 1.0)

    // Text colors: white for connected, dim grey when disconnected. The
    // appearance argument is kept for API compatibility (and a future light-bar
    // variant if needed) but currently both branches return the same value.
    static func text(_ a: IconAppearance) -> CGColor {
        CGColor(red: 1, green: 1, blue: 1, alpha: 1)
    }
    static func dimText(_ a: IconAppearance) -> CGColor {
        CGColor(red: 0xbb/255.0, green: 0xbb/255.0, blue: 0xbb/255.0, alpha: 1.0)
    }
}

private let perGPUGap: CGFloat = 4
private let donutPadding: CGFloat = 2
private let innerLabelSize: CGFloat = 8

/// Picks the donut "used" color from memory utilization, matching `render.rs::used_color`.
private func usedColor(_ pct: Float) -> CGColor {
    if pct >= 90 { return IconColors.highDonut }
    if pct >= 80 { return IconColors.warn2Donut }
    if pct >= 70 { return IconColors.warn1Donut }
    return IconColors.ok
}

/// Label color: always neutral (matches the system convention for menu bar
/// items — clock, battery and the like never use semantic colors). The donut
/// already conveys urgency via its used-arc color, so the text staying neutral
/// avoids visual noise. On the linux panel we kept threshold colors because
/// the panel has no other strong visual element; on macOS the standard is
/// to let the icon do the talking.
private func tempLabelColor(connected: Bool, appearance: IconAppearance) -> CGColor {
    connected ? IconColors.text(appearance) : IconColors.dimText(appearance)
}

/// Text size matches the rust tray: floor(0.45 * height), clamped to [8, 16].
/// Fractional sizes produce blurry glyphs — the linux tray learned this the hard way.
private func textSize(forHeight h: CGFloat) -> CGFloat {
    let raw = (h * 0.45).rounded()
    return max(8, min(16, raw))
}

struct IconRenderer {
    /// Logical height in points (status bar shows ~22 pt). The bitmap is rendered
    /// at 2× this for Retina; macOS scales the NSImage back down by its `size`.
    let height: CGFloat
    let baseIcon: CGImage?

    init(height: CGFloat) {
        self.height = height
        self.baseIcon = Self.loadBaseIcon(targetHeight: height)
    }

    /// Renders the status bar icon at 2× resolution and wraps it as an NSImage
    /// whose logical size equals (totalLogicalWidth, height). Must run on the
    /// main actor because it constructs an NSImage.
    @MainActor
    func renderImage(gpus: [GPU], connected: Bool, appearance: IconAppearance) -> NSImage? {
        guard let result = renderCGImage(gpus: gpus, connected: connected, appearance: appearance) else {
            return nil
        }
        let img = NSImage(cgImage: result.cgImage, size: result.logicalSize)
        img.isTemplate = false
        return img
    }

    /// Render a snapshot to a PNG file via ImageIO. Runs without AppKit so it
    /// is safe from any thread (used by --dump-icon). Defaults to the dark
    /// palette so the dump matches the linux tray's look on a dark panel.
    func renderPNG(
        gpus: [GPU],
        connected: Bool,
        to path: String,
        appearance: IconAppearance = .dark
    ) throws {
        guard let result = renderCGImage(gpus: gpus, connected: connected, appearance: appearance) else {
            throw NSError(domain: "IconRenderer", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "render failed"])
        }
        let url = URL(fileURLWithPath: path)
        guard let dest = CGImageDestinationCreateWithURL(
            url as CFURL, UTType.png.identifier as CFString, 1, nil
        ) else {
            throw NSError(domain: "IconRenderer", code: 2,
                          userInfo: [NSLocalizedDescriptionKey: "could not create PNG destination"])
        }
        CGImageDestinationAddImage(dest, result.cgImage, nil)
        guard CGImageDestinationFinalize(dest) else {
            throw NSError(domain: "IconRenderer", code: 3,
                          userInfo: [NSLocalizedDescriptionKey: "PNG encode failed"])
        }
    }

    private struct RenderResult {
        let cgImage: CGImage
        let logicalSize: CGSize
    }

    /// Pure Core Graphics: no AppKit, no MainActor. Returns nil only on
    /// allocation failures (effectively never).
    private func renderCGImage(gpus: [GPU], connected: Bool, appearance: IconAppearance) -> RenderResult? {
        let scale: CGFloat = 2
        let layout = self.layout(gpus: gpus, scale: scale, connected: connected, appearance: appearance)
        let pxW = max(1, Int(layout.totalLogicalWidth * scale))
        let pxH = max(1, Int(height * scale))

        guard let ctx = CGContext(
            data: nil,
            width: pxW,
            height: pxH,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return nil }

        // Core Graphics origin is bottom-left. We want our coordinate system to
        // match the rust renderer (top-left origin), so flip vertically. Then
        // multiply by `scale` so all subsequent draw calls can use logical points.
        ctx.translateBy(x: 0, y: CGFloat(pxH))
        ctx.scaleBy(x: scale, y: -scale)

        draw(layout: layout, ctx: ctx, connected: connected)

        guard let cg = ctx.makeImage() else { return nil }
        return RenderResult(
            cgImage: cg,
            logicalSize: CGSize(width: layout.totalLogicalWidth, height: height)
        )
    }

    // MARK: - Layout

    private struct Layout {
        let totalLogicalWidth: CGFloat
        let perGPUWidth: CGFloat
        let donutSize: CGFloat
        let iconWidth: CGFloat
        let textWidth: CGFloat
        let textPx: CGFloat
        let gpus: [GPU]
        let connected: Bool
        let appearance: IconAppearance
    }

    private func layout(gpus: [GPU], scale: CGFloat, connected: Bool, appearance: IconAppearance) -> Layout {
        let textPx = textSize(forHeight: height)
        let probeWidth = measureText("0(00ºC)", size: textPx)
        let donutSize = max(8, height - donutPadding * 2)
        let iconW: CGFloat = baseIcon.map { CGFloat($0.width) / scale } ?? 0
        let perGPU = iconW + 2 + probeWidth + 2 + donutSize
        let total: CGFloat
        if gpus.isEmpty {
            // Empty state: just icon + small gap + donut. Keeps the bar narrow
            // and avoids reserving space for text we no longer draw.
            total = iconW + 4 + donutSize
        } else {
            let count = CGFloat(gpus.count)
            total = perGPU * count + perGPUGap * max(0, count - 1)
        }
        return Layout(
            totalLogicalWidth: max(height, total),
            perGPUWidth: perGPU,
            donutSize: donutSize,
            iconWidth: iconW,
            textWidth: probeWidth,
            textPx: textPx,
            gpus: gpus,
            connected: connected,
            appearance: appearance
        )
    }

    // MARK: - Drawing

    private func draw(layout: Layout, ctx: CGContext, connected: Bool) {
        if layout.gpus.isEmpty {
            // Connecting / disconnected / no GPUs: show only the base icon
            // (dimmed) plus a grey donut. We deliberately do NOT draw any text
            // here — older versions wrote "no GPUs" at x=0 which got covered
            // by the donut at x=0, leaving a clipped "GPUs" visible. The menu
            // already explains the state in plain text.
            var x: CGFloat = 0
            if let icon = baseIcon {
                let iconHpt = CGFloat(icon.height) / 2.0
                let iconWpt = CGFloat(icon.width) / 2.0
                let iconY = (height - iconHpt) / 2.0
                ctx.saveGState()
                ctx.interpolationQuality = .high
                ctx.setAlpha(0.4)
                ctx.translateBy(x: x, y: iconY + iconHpt)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(icon, in: CGRect(x: 0, y: 0, width: iconWpt, height: iconHpt))
                ctx.restoreGState()
                x += iconWpt + 4
            }
            drawDonut(
                ctx: ctx,
                x: x,
                y: donutPadding,
                size: layout.donutSize,
                usedPercent: 0,
                connected: false
            )
            return
        }

        var x: CGFloat = 0
        for (i, gpu) in layout.gpus.enumerated() {
            if i > 0 { x += perGPUGap }
            drawGPUBlock(ctx: ctx, originX: x, gpu: gpu, layout: layout)
            x += layout.perGPUWidth
        }
    }

    private func drawGPUBlock(ctx: CGContext, originX x: CGFloat, gpu: GPU, layout: Layout) {
        if let icon = baseIcon {
            let iconHpt = CGFloat(icon.height) / 2.0
            let iconWpt = CGFloat(icon.width) / 2.0
            let iconY = (height - iconHpt) / 2.0
            let rect = CGRect(x: x, y: iconY, width: iconWpt, height: iconHpt)
            ctx.saveGState()
            ctx.interpolationQuality = .high
            // We're in a flipped coordinate system: draw with a temporary flip
            // for the image so it lands right-side-up.
            ctx.translateBy(x: rect.origin.x, y: rect.origin.y + rect.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(icon, in: CGRect(origin: .zero, size: rect.size))
            ctx.restoreGState()
        }

        let temp = gpu.temperatureC ?? 0
        let label = String(format: "%d(%2dºC)", gpu.index, temp)
        let labelColor = tempLabelColor(connected: layout.connected, appearance: layout.appearance)
        let textX = x + layout.iconWidth + 2
        drawText(
            label,
            ctx: ctx,
            x: textX,
            size: layout.textPx,
            color: labelColor,
            blockHeight: height
        )

        let donutX = x + layout.iconWidth + 2 + layout.textWidth + 2
        let usedPct = gpu.memory.usedPercent
        drawDonut(
            ctx: ctx,
            x: donutX,
            y: donutPadding,
            size: layout.donutSize,
            usedPercent: usedPct,
            connected: layout.connected
        )

        // Memory percent centered inside the donut hole. Same idea as the linux
        // tray: 8 pt fits "100" in the inner diameter without touching the ring.
        let pctText = "\(Int(usedPct.rounded()))"
        let pctW = measureText(pctText, size: innerLabelSize)
        let pctX = donutX + layout.donutSize / 2 - pctW / 2
        let pctColor = layout.connected
            ? IconColors.text(layout.appearance)
            : IconColors.dimText(layout.appearance)
        drawText(
            pctText,
            ctx: ctx,
            x: pctX,
            size: innerLabelSize,
            color: pctColor,
            blockHeight: height
        )
    }

    private func drawDonut(
        ctx: CGContext,
        x: CGFloat,
        y: CGFloat,
        size: CGFloat,
        usedPercent: Float,
        connected: Bool
    ) {
        let cx = x + size / 2
        let cy = y + size / 2
        let rOuter = size / 2
        let rInner = rOuter * 0.78

        let freeColor = connected ? IconColors.free : IconColors.dimFree
        ctx.saveGState()
        ctx.setFillColor(freeColor)
        ctx.addArc(center: CGPoint(x: cx, y: cy), radius: rOuter, startAngle: 0, endAngle: .pi * 2, clockwise: false)
        ctx.fillPath()
        ctx.restoreGState()

        if usedPercent > 0.5 {
            let color = connected ? usedColor(usedPercent) : IconColors.dimUsed
            let sweep = CGFloat(min(100, max(0, usedPercent))) / 100.0 * (.pi * 2)
            // Match the rust renderer: start at -90° (12 o'clock) sweeping clockwise.
            // In a flipped y-axis context, "clockwise on screen" corresponds to
            // CG's `clockwise: false` argument. The rust code uses
            // [-90°, -90° + sweep] in degrees with CCW math → same visual.
            let start = -CGFloat.pi / 2
            let end = start + sweep
            ctx.saveGState()
            ctx.setFillColor(color)
            ctx.move(to: CGPoint(x: cx, y: cy))
            ctx.addArc(
                center: CGPoint(x: cx, y: cy),
                radius: rOuter,
                startAngle: start,
                endAngle: end,
                clockwise: false
            )
            ctx.closePath()
            ctx.fillPath()
            ctx.restoreGState()
        }

        // Punch the hole. `clear` blend mode wipes alpha through whatever is
        // already painted, leaving the ring.
        ctx.saveGState()
        ctx.setBlendMode(.clear)
        ctx.addArc(center: CGPoint(x: cx, y: cy), radius: rInner, startAngle: 0, endAngle: .pi * 2, clockwise: false)
        ctx.fillPath()
        ctx.restoreGState()
    }

    // MARK: - Text (CoreText, SF Mono)

    private static func font(size: CGFloat) -> CTFont {
        // monospacedDigit, not monospaced: the system menu bar (clock,
        // battery) uses SF Pro with monospaced digits — proportional letters
        // for natural readability, fixed-width digits so the bar item doesn't
        // jiggle as the temperature changes from 99 to 100.
        let nsf = NSFont.monospacedDigitSystemFont(ofSize: size, weight: .regular)
        return nsf as CTFont
    }

    private func measureText(_ text: String, size: CGFloat) -> CGFloat {
        // Color is irrelevant for measurement, but CTLine still needs one.
        let line = makeLine(text: text, size: size, color: IconColors.text(.dark))
        let width = CTLineGetTypographicBounds(line, nil, nil, nil)
        return CGFloat(width)
    }

    private func makeLine(text: String, size: CGFloat, color: CGColor) -> CTLine {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: Self.font(size: size),
            .foregroundColor: color,
        ]
        let attributed = NSAttributedString(string: text, attributes: attrs)
        return CTLineCreateWithAttributedString(attributed)
    }

    private func drawText(
        _ text: String,
        ctx: CGContext,
        x: CGFloat,
        size: CGFloat,
        color: CGColor,
        blockHeight: CGFloat
    ) {
        let line = makeLine(text: text, size: size, color: color)
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        var leading: CGFloat = 0
        _ = CTLineGetTypographicBounds(line, &ascent, &descent, &leading)

        // Center vertically in the block. The rust renderer uses
        // ((blockH - sizePx)/2 + ascent_px) as baseline; we want the visual
        // line to sit at the same midpoint.
        let baselineFromTop = ((blockHeight - size) / 2.0).rounded() + ascent

        ctx.saveGState()
        // CoreText draws with origin at the baseline, in the current CTM.
        // Our context is flipped (y grows downward). Re-flip locally so the
        // glyphs come out right-side-up but still positioned by the top-left
        // logical coordinate we passed in.
        ctx.translateBy(x: x, y: baselineFromTop)
        ctx.scaleBy(x: 1, y: -1)
        ctx.textPosition = .zero
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }

    // MARK: - Base icon loading

    private static func loadBaseIcon(targetHeight: CGFloat) -> CGImage? {
        guard let url = Bundle.module.url(forResource: "tarjeta-de-video", withExtension: "png"),
              let src = CGImageSourceCreateWithURL(url as CFURL, nil),
              let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
            return nil
        }

        // Resize to 2× target height so it composites cleanly at Retina.
        let pxH = Int(targetHeight * 2)
        let aspect = CGFloat(cg.width) / CGFloat(cg.height)
        let pxW = max(1, Int((targetHeight * 2 * aspect).rounded()))

        guard let ctx = CGContext(
            data: nil,
            width: pxW,
            height: pxH,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return nil }

        ctx.interpolationQuality = .high
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: pxW, height: pxH))
        return ctx.makeImage()
    }
}
