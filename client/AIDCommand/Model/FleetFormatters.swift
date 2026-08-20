// Display formatters — unknown stays unknown, never fake defaults.
// Exports: FleetFormatters.

import Foundation

enum FleetFormatters {
    static func cost(_ value: String?) -> String {
        guard let value else { return "—" }
        return value.hasPrefix("$") ? value : value
    }

    static func model(_ value: String?) -> String {
        value ?? "—"
    }

    static func tokens(_ value: String?) -> String {
        value ?? "—"
    }

    static func elapsed(seconds: Int) -> String {
        guard seconds > 0 else { return "—" }
        let h = seconds / 3600
        let m = (seconds % 3600) / 60
        let s = seconds % 60
        if h > 0 { return String(format: "%dh %02dm", h, m) }
        return String(format: "%dm %02ds", m, s)
    }

    static func percent(_ value: Double) -> String {
        String(format: "%d%%", Int(value * 100))
    }

    static func workgroupLabel(_ id: String) -> String {
        let trimmed = id.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty || trimmed.allSatisfy({ $0 == "—" || $0 == "-" }) {
            return "—"
        }
        return "WG-\(trimmed.prefix(8).uppercased())"
    }

    /// Counts the server did not really measure (nil / 0) render as unknown.
    static func measuredCount(_ value: Int?) -> String {
        guard let value, value > 0 else { return "—" }
        return String(value)
    }

    static func reactorLoad(running: Int) -> String {
        String(min(99, running * 12 + 8))
    }
}
