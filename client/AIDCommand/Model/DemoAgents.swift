// Demo agent roster derived from the canvas dataset.
// Exports: DemoAgents.

import Foundation

enum DemoAgents {
    static func roster(from sectors: [Sector]) -> [AgentInfo] {
        let missions = sectors.flatMap(\.missions)
        let names = Set(missions.map(\.agent))
        return names.sorted().map { agent in
            let flown = missions.filter { $0.agent == agent && ($0.state == .done || $0.state == .fail) }.count
            let busy = missions.contains { $0.agent == agent && $0.state == .run }
            return AgentInfo(id: agent, busy: busy, quotaOK: agent != "gemini", taskCount: flown + (busy ? 1 : 0))
        }
    }
}
