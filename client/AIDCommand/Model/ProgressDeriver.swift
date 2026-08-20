// Derives running-mission progress from elapsed vs agent median duration.
// Exports: ProgressDeriver.

import Foundation

enum ProgressDeriver {
    static func progress(
        elapsedSeconds: Int,
        agent: String,
        completedDurations: [String: [Int]]
    ) -> Double {
        let durations = completedDurations[agent] ?? []
        guard !durations.isEmpty else { return min(0.5, Double(elapsedSeconds) / 3600) }
        let sorted = durations.sorted()
        let median = sorted[sorted.count / 2]
        guard median > 0 else { return 0.5 }
        return min(0.97, Double(elapsedSeconds) / Double(median))
    }

    static func medianDurations(from sectors: [Sector]) -> [String: [Int]] {
        var map: [String: [Int]] = [:]
        for sector in sectors {
            for mission in sector.missions where mission.state == .done {
                map[mission.agent, default: []].append(mission.elapsedSeconds)
            }
        }
        return map
    }
}
