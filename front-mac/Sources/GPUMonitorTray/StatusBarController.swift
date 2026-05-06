import AppKit
import Foundation

private let repoURL = URL(string: "https://github.com/maximofn/gpu_monitor")!
private let coffeeURL = URL(string: "https://www.buymeacoffee.com/maximofn")!

enum TrayState: Sendable {
    case connecting
    case connected(Snapshot)
    case disconnected(String)
}

@MainActor
final class StatusBarController: NSObject {
    private let statusItem: NSStatusItem
    private let renderer: IconRenderer
    private let backendURL: String
    private var state: TrayState = .connecting
    private var lastAppearance: IconAppearance = .dark
    private var lastRenderedKey: String = ""

    init(renderer: IconRenderer, backendURL: String) {
        self.renderer = renderer
        self.backendURL = backendURL
        // Variable-length: accommodate multi-GPU icons without the system
        // squashing them. NSStatusItem.variableLength is the canonical setting.
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        super.init()
        if let button = statusItem.button {
            button.imagePosition = .imageLeft
            button.toolTip = "GPU Monitor — connecting to \(backendURL)"
        }
        // Subscribe to the system-wide light/dark toggle. We deliberately do
        // NOT KVO `effectiveAppearance` on the button: AppKit re-evaluates that
        // property during normal repaints, and any churn there would re-trigger
        // refreshIcon → set image → repaint → KVO… in a tight feedback loop.
        DistributedNotificationCenter.default.addObserver(
            self,
            selector: #selector(appearanceChanged),
            name: Notification.Name("AppleInterfaceThemeChangedNotification"),
            object: nil
        )
        lastAppearance = currentAppearance
        applyState(.connecting)
    }

    deinit {
        DistributedNotificationCenter.default.removeObserver(self)
    }

    @objc private func appearanceChanged() {
        Task { @MainActor in
            self.lastAppearance = self.currentAppearance
            self.lastRenderedKey = ""  // invalidate so the next refreshIcon redraws
            self.refreshIcon()
        }
    }

    func applyState(_ new: TrayState) {
        state = new
        refreshIcon()
        refreshMenu()
        refreshTooltip()
    }

    private var currentAppearance: IconAppearance {
        let appearance = statusItem.button?.effectiveAppearance ?? NSApp.effectiveAppearance
        let match = appearance.bestMatch(from: [.darkAqua, .vibrantDark, .aqua, .vibrantLight])
        switch match {
        case .darkAqua, .vibrantDark: return .dark
        default: return .light
        }
    }

    private func refreshIcon() {
        let (gpus, connected): ([GPU], Bool) = {
            switch state {
            case .connected(let snap): return (snap.gpus, true)
            default: return ([], false)
            }
        }()
        // Dedupe identical renders — when the snapshot's visible fields haven't
        // changed, skip both the CGContext work and the AppKit image swap. The
        // backend ticks at ~1 Hz; most ticks have identical visible state.
        let key = renderKey(gpus: gpus, connected: connected, appearance: lastAppearance)
        if key == lastRenderedKey { return }
        lastRenderedKey = key
        if let img = renderer.renderImage(gpus: gpus, connected: connected, appearance: lastAppearance) {
            statusItem.button?.image = img
        }
    }

    private func renderKey(gpus: [GPU], connected: Bool, appearance: IconAppearance) -> String {
        var parts: [String] = ["\(connected)", "\(appearance)"]
        for g in gpus {
            // Visible inputs only: index, temp, used percent rounded.
            let pct = Int(g.memory.usedPercent.rounded())
            parts.append("\(g.index):\(g.temperatureC ?? 0):\(pct)")
        }
        return parts.joined(separator: "|")
    }

    private func refreshTooltip() {
        guard let button = statusItem.button else { return }
        switch state {
        case .connecting:
            button.toolTip = "GPU Monitor — connecting to \(backendURL)"
        case .connected(let snap):
            let header = "\(snap.gpus.count) GPU(s) — \(snap.driverVersion ?? "driver: unknown")"
            let body = snap.gpus.map { g in
                let used = bytesToGiB(g.memory.usedBytes)
                let total = bytesToGiB(g.memory.totalBytes)
                return "GPU \(g.index) \(g.name) — \(g.temperatureC ?? 0)°C, \(used)/\(total) GiB (\(Int(g.memory.usedPercent.rounded()))%)"
            }.joined(separator: "\n")
            button.toolTip = "\(header)\n\(body)"
        case .disconnected(let err):
            button.toolTip = "Backend offline: \(err)"
        }
    }

    private func refreshMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false

        switch state {
        case .connecting:
            menu.addItem(disabledItem("Connecting to \(backendURL)…"))
            menu.addItem(.separator())
        case .disconnected(let err):
            menu.addItem(disabledItem("Backend offline: \(err)"))
            menu.addItem(disabledItem("Backend: \(backendURL)"))
            menu.addItem(.separator())
        case .connected(let snap):
            for gpu in snap.gpus {
                let item = NSMenuItem(title: "GPU \(gpu.index) — \(gpu.name)", action: nil, keyEquivalent: "")
                item.submenu = gpuSubmenu(for: gpu)
                menu.addItem(item)
            }
            menu.addItem(.separator())
            var backendLine = "Backend: \(backendURL)"
            if let driver = snap.driverVersion {
                backendLine += " — driver \(driver)"
            }
            menu.addItem(disabledItem(backendLine))
            menu.addItem(disabledItem("Updated: \(shortTime(snap.timestamp))"))
            menu.addItem(.separator())
        }

        let repo = NSMenuItem(title: "Repository", action: #selector(openRepo), keyEquivalent: "")
        repo.target = self
        menu.addItem(repo)
        let coffee = NSMenuItem(title: "Buy me a coffee", action: #selector(openCoffee), keyEquivalent: "")
        coffee.target = self
        menu.addItem(coffee)
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)

        statusItem.menu = menu
    }

    private func gpuSubmenu(for gpu: GPU) -> NSMenu {
        let m = NSMenu()
        m.autoenablesItems = false
        m.addItem(disabledItem("Temperature: \(gpu.temperatureC ?? 0)°C"))
        m.addItem(disabledItem(
            "Utilization: GPU \(gpu.utilization.gpuPercent)% / Mem \(gpu.utilization.memoryPercent)%"
        ))
        m.addItem(disabledItem("Memory used: \(formatBytes(gpu.memory.usedBytes))"))
        m.addItem(disabledItem("Memory free: \(formatBytes(gpu.memory.freeBytes))"))
        m.addItem(disabledItem(
            "Memory total: \(formatBytes(gpu.memory.totalBytes)) (\(Int(gpu.memory.usedPercent.rounded()))% used)"
        ))
        if let draw = gpu.powerDrawW, let limit = gpu.powerLimitW {
            m.addItem(disabledItem(String(format: "Power: %.0fW / %.0fW", draw, limit)))
        }

        m.addItem(.separator())
        if gpu.processes.isEmpty {
            m.addItem(disabledItem("No GPU processes"))
        } else {
            m.addItem(disabledItem("Processes (\(gpu.processes.count))"))
            for proc in gpu.processes {
                let line = String(
                    format: "  %6d %-7@ %@ (%@)",
                    proc.pid,
                    kindLabel(proc.kind) as NSString,
                    proc.name as NSString,
                    formatBytes(proc.usedMemoryBytes) as NSString
                )
                m.addItem(disabledItem(line))
            }
        }
        return m
    }

    @objc private func openRepo() { NSWorkspace.shared.open(repoURL) }
    @objc private func openCoffee() { NSWorkspace.shared.open(coffeeURL) }
    @objc private func quit() { NSApp.terminate(nil) }
}

// MARK: - Helpers

private func disabledItem(_ title: String) -> NSMenuItem {
    let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
    item.isEnabled = false
    return item
}

private func bytesToGiB(_ bytes: UInt64) -> UInt64 {
    bytes / (1024 * 1024 * 1024)
}

private func formatBytes(_ bytes: UInt64) -> String {
    let gib: Double = 1024 * 1024 * 1024
    let mib: Double = 1024 * 1024
    let b = Double(bytes)
    if b >= gib { return String(format: "%.2f GiB", b / gib) }
    if b >= mib { return String(format: "%.0f MiB", b / mib) }
    return "\(bytes) B"
}

private func kindLabel(_ kind: ProcessKind) -> String {
    switch kind {
    case .compute: return "compute"
    case .graphics: return "graphic"
    case .mixed: return "mixed"
    }
}

/// "2026-05-06T10:11:12.345Z" → "10:11:12". Mirrors the rust short_time helper.
private func shortTime(_ rfc3339: String) -> String {
    guard let tIdx = rfc3339.firstIndex(of: "T") else { return rfc3339 }
    let after = rfc3339[rfc3339.index(after: tIdx)...]
    if let dot = after.firstIndex(of: ".") {
        return String(after[..<dot])
    }
    if let plus = after.firstIndex(where: { $0 == "+" || $0 == "Z" || $0 == "-" }) {
        return String(after[..<plus])
    }
    return String(after)
}
