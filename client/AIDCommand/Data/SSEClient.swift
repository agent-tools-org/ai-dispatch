// Minimal SSE reader over URLSession for /api/events.
// Exports: SSEClient.

import Foundation

struct SSEMessage: Sendable {
    let event: String
    let data: String
}

final class SSEClient: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let onMessage: @Sendable (SSEMessage) -> Void
    private let onFailure: @Sendable (Error) -> Void
    private var session: URLSession?
    private var task: URLSessionDataTask?
    private var buffer = Data()
    private var currentEvent = "message"
    private var dataLines: [String] = []

    init(
        onMessage: @escaping @Sendable (SSEMessage) -> Void,
        onFailure: @escaping @Sendable (Error) -> Void
    ) {
        self.onMessage = onMessage
        self.onFailure = onFailure
    }

    func start(request: URLRequest) {
        stop()
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 0
        config.timeoutIntervalForResource = 0
        let session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        self.session = session
        let task = session.dataTask(with: request)
        self.task = task
        task.resume()
    }

    func stop() {
        task?.cancel()
        task = nil
        session?.invalidateAndCancel()
        session = nil
        buffer.removeAll()
        currentEvent = "message"
        dataLines.removeAll()
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive data: Data
    ) {
        buffer.append(data)
        while let range = buffer.range(of: Data("\n".utf8)) {
            let lineData = buffer.subdata(in: buffer.startIndex..<range.lowerBound)
            buffer.removeSubrange(buffer.startIndex..<range.upperBound)
            handleLine(String(data: lineData, encoding: .utf8) ?? "")
        }
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didCompleteWithError error: Error?
    ) {
        if let error {
            onFailure(error)
            return
        }
        if let response = task.response as? HTTPURLResponse, response.statusCode >= 400 {
            onFailure(SSEError.httpStatus(response.statusCode))
        }
    }

    private func handleLine(_ raw: String) {
        let line = raw.hasSuffix("\r") ? String(raw.dropLast()) : raw
        if line.isEmpty {
            flushEvent()
            return
        }
        if line.hasPrefix(":") { return }
        if line.hasPrefix("event:") {
            currentEvent = String(line.dropFirst(6)).trimmingCharacters(in: .whitespaces)
            return
        }
        if line.hasPrefix("data:") {
            dataLines.append(String(line.dropFirst(5)).trimmingCharacters(in: .whitespaces))
        }
    }

    private func flushEvent() {
        guard !dataLines.isEmpty else {
            currentEvent = "message"
            return
        }
        let message = SSEMessage(event: currentEvent, data: dataLines.joined(separator: "\n"))
        currentEvent = "message"
        dataLines.removeAll()
        onMessage(message)
    }
}

enum SSEError: Error {
    case httpStatus(Int)
}
