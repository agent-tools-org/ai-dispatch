// Demo mission prompts and event lines for the brief panel.
// Exports: DemoMissionContent.

import Foundation

enum DemoMissionContent {
    static func prompt(for mission: Mission) -> String {
        switch mission.id {
        case "t-561886a0":
            return "Advisory sweep of the filler quote path — trace venue selection and quote assembly."
        case "t-85e75668":
            return "Fill-rate regression pass across nine venues; capture latency and success deltas."
        case "t-9f10c3d2":
            return "Cut release v8.78.0 — bump version, changelog entry, tag, and publish notes."
        case "t-6690cdb4":
            return "Joint audit of two accounting changes — verify rounding and withdrawal paths."
        case "t-0c41aa19":
            return "Batch progress plumbing — wire progress events through the store layer."
        default:
            return mission.title
        }
    }

    static func events(for mission: Mission) -> [MissionEvent] {
        switch mission.state {
        case .run:
            return [
                MissionEvent(id: "1", message: "agent started"),
                MissionEvent(id: "2", message: "tool: read \(mission.title.prefix(24))…"),
                MissionEvent(id: "3", message: "still running · \(FleetFormatters.elapsed(seconds: mission.elapsedSeconds))"),
            ]
        case .done:
            return [
                MissionEvent(id: "1", message: "agent started"),
                MissionEvent(id: "2", message: "verify passed"),
                MissionEvent(id: "3", message: "mission complete"),
            ]
        case .fail:
            return [
                MissionEvent(id: "1", message: "agent started"),
                MissionEvent(id: "2", message: "verify failed"),
                MissionEvent(id: "3", message: "mission lost"),
            ]
        case .stop:
            return [
                MissionEvent(id: "1", message: "agent started"),
                MissionEvent(id: "2", message: "stop requested"),
                MissionEvent(id: "3", message: "mission held"),
            ]
        }
    }
}
