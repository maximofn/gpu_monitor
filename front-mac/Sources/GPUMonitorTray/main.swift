import AppKit
import Foundation

let config = Config.parse(CommandLine.arguments)

if let dumpPath = config.dumpIcon {
    // One-shot mode: fetch a single snapshot via /v1/snapshot, render to PNG,
    // exit. Mirrors the linux tray's --dump-icon for visual diffing.
    //
    // Using synchronous URL/Data here on purpose: spawning Swift Tasks pinned
    // to MainActor while the main thread blocks on a semaphore deadlocks (the
    // MainActor executor IS the main thread). The renderer's PNG path is
    // pure Core Graphics so it doesn't need AppKit isolation.
    let url = SSEClient.snapshotURL(from: config.backendURL)
    let renderer = IconRenderer(height: config.iconHeight)
    do {
        let data = try Data(contentsOf: url)
        let snap = try JSONDecoder().decode(Snapshot.self, from: data)
        try renderer.renderPNG(gpus: snap.gpus, connected: true, to: dumpPath)
        print("wrote \(dumpPath)")
        exit(0)
    } catch {
        // Fall back to the disconnected look so --dump-icon doubles as a way to
        // preview the offline state (point it at a bogus URL on purpose).
        FileHandle.standardError.write(
            Data("backend unreachable (\(error.localizedDescription)) — dumping disconnected icon\n".utf8)
        )
        do {
            try renderer.renderPNG(gpus: [], connected: false, to: dumpPath)
            print("wrote \(dumpPath) (disconnected)")
            exit(0)
        } catch {
            FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            exit(1)
        }
    }
}

let app = NSApplication.shared
let delegate = AppDelegate(config: config)
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
