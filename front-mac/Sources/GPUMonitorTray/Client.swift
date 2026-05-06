import Foundation
import OSLog

enum ClientUpdate: Sendable {
    case connecting
    case connected(Snapshot)
    case disconnected(String)
}

actor SSEClient {
    private let backendURL: String
    private let logger: Logger
    private var task: Task<Void, Never>?

    init(backendURL: String) {
        self.backendURL = backendURL
        self.logger = Logger(subsystem: "com.maximofn.gpu-monitor", category: "client")
    }

    func start(onUpdate: @escaping @Sendable (ClientUpdate) async -> Void) {
        task?.cancel()
        let url = backendURL
        let log = logger
        task = Task.detached { [weak self] in
            await self?.runLoop(streamURL: Self.streamURL(from: url), logger: log, onUpdate: onUpdate)
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    private nonisolated func runLoop(
        streamURL: URL,
        logger: Logger,
        onUpdate: @escaping @Sendable (ClientUpdate) async -> Void
    ) async {
        var backoffSeconds: UInt64 = 1
        await onUpdate(.connecting)

        while !Task.isCancelled {
            do {
                try await openOnce(streamURL: streamURL, logger: logger, backoff: &backoffSeconds, onUpdate: onUpdate)
                // Stream finished cleanly (server closed without error). Treat as disconnect and reconnect.
                await onUpdate(.disconnected("stream closed by server"))
            } catch is CancellationError {
                return
            } catch {
                logger.warning("SSE session ended: \(error.localizedDescription, privacy: .public)")
                await onUpdate(.disconnected(error.localizedDescription))
            }

            do {
                try await Task.sleep(nanoseconds: backoffSeconds * 1_000_000_000)
            } catch {
                return
            }
            backoffSeconds = min(backoffSeconds * 2, 5)
        }
    }

    /// One SSE connection lifetime. Resets backoff on the first successful event.
    private nonisolated func openOnce(
        streamURL: URL,
        logger: Logger,
        backoff: inout UInt64,
        onUpdate: @escaping @Sendable (ClientUpdate) async -> Void
    ) async throws {
        logger.info("connecting to \(streamURL.absoluteString, privacy: .public)")
        var req = URLRequest(url: streamURL)
        req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        req.timeoutInterval = 30

        let (bytes, response) = try await URLSession.shared.bytes(for: req)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw NSError(
                domain: "SSEClient",
                code: http.statusCode,
                userInfo: [NSLocalizedDescriptionKey: "HTTP \(http.statusCode)"]
            )
        }

        let decoder = JSONDecoder()
        // SSE frames events with one or more `data:` lines followed by a blank line.
        // The current backend ships each Snapshot in a single `data:` line, but the
        // accumulator handles the multi-line case for free.
        var buffer = ""

        for try await line in bytes.lines {
            if Task.isCancelled { throw CancellationError() }

            // Note: Foundation's `bytes.lines` collapses consecutive `\n`s, so
            // the blank line between SSE events is NEVER yielded. That means we
            // can't rely on the empty line as a flush trigger. Instead we try
            // to decode after every `data:` line — gpu-monitord ships one
            // complete Snapshot per line, so the JSON is always self-contained.
            if line.hasPrefix(":") { continue } // comment line per SSE spec
            if line.hasPrefix("data:") {
                var payload = line.dropFirst(5)
                if payload.first == " " { payload = payload.dropFirst() }
                if !buffer.isEmpty { buffer.append("\n") }
                buffer.append(String(payload))

                if let data = buffer.data(using: .utf8) {
                    do {
                        let snap = try decoder.decode(Snapshot.self, from: data)
                        backoff = 1
                        await onUpdate(.connected(snap))
                        buffer.removeAll(keepingCapacity: true)
                    } catch {
                        // Probably a multi-line `data:` continuation; keep
                        // accumulating until the next attempt parses cleanly.
                        // If after several lines it still doesn't, log once.
                        if buffer.count > 1_000_000 {
                            logger.warning("SSE buffer >1MB without decode; resetting")
                            buffer.removeAll(keepingCapacity: false)
                        }
                    }
                }
            }
            // Other field names (event:, id:, retry:) are ignored — the server does not use them.
        }
    }

    private static func streamURL(from base: String) -> URL {
        var trimmed = base
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + "/v1/stream")!
    }

    static func snapshotURL(from base: String) -> URL {
        var trimmed = base
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + "/v1/snapshot")!
    }
}
