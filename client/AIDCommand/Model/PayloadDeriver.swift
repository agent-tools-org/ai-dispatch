// Client-side payload kind and priority derivation from mission facts.
// Exports: PayloadKind, Payload, PayloadDeriver.

import Foundation

enum PayloadKind: String, Sendable, CaseIterable {
    case release = "RELEASE"
    case report = "REPORT"
    case patch = "PATCH"
    case audit = "AUDIT"
    case bench = "BENCH"
    case fixture = "FIXTURE"
    case dataset = "DATASET"
    case scrap = "SCRAP"
}

struct Payload: Identifiable, Sendable, Equatable {
    let id: MissionID
    let name: String
    let kind: PayloadKind
    let rarity: PayloadRarity
    let sectorTag: String
    let manifest: String
}

enum PayloadDeriver {
    static func payloads(from sectors: [Sector]) -> [Payload] {
        sectors.flatMap { sector in
            sector.missions.compactMap { derive(from: $0, sectorTag: sector.tag) }
        }
    }

    static func derive(from mission: Mission, sectorTag: String) -> Payload? {
        switch mission.state {
        case .fail, .stop:
            return Payload(
                id: mission.id,
                name: "Salvaged worktree",
                kind: .scrap,
                rarity: .salvage,
                sectorTag: sectorTag,
                manifest: manifest(for: mission)
            )
        case .done:
            let kind = kind(for: mission)
            return Payload(
                id: mission.id,
                name: payloadName(for: mission, kind: kind),
                kind: kind,
                rarity: rarity(for: mission, kind: kind),
                sectorTag: sectorTag,
                manifest: manifest(for: mission)
            )
        case .run:
            return nil
        }
    }

    static func countCritical(in payloads: [Payload]) -> Int {
        payloads.filter { $0.rarity == .legendary || $0.rarity == .epic }.count
    }

    private static func kind(for mission: Mission) -> PayloadKind {
        switch mission.id {
        case "t-85e75668": return .report
        case "t-9f10c3d2": return .release
        case "t-b1930f5c": return .fixture
        default: break
        }
        let title = mission.title.lowercased()
        if title.contains("release") || title.contains("cut release") { return .release }
        if title.contains("audit") { return .audit }
        if title.contains("review") || title.contains("regression") { return .report }
        if title.contains("measurement") || title.contains("bench") { return .bench }
        if title.contains("fix") || title.contains("type") { return .fixture }
        if title.contains("patch") || title.contains("plumbing") { return .patch }
        if title.contains("dataset") || title.contains("discovery") { return .dataset }
        return .report
    }

    private static func payloadName(for mission: Mission, kind: PayloadKind) -> String {
        switch mission.id {
        case "t-85e75668": return "Fill-rate regression"
        case "t-9f10c3d2": return "aid v8.78.0"
        case "t-b1930f5c": return "Store type fixes"
        default:
            if kind == .release, let title = mission.title.split(separator: " ").last {
                return String(title)
            }
            return mission.title
        }
    }

    private static func rarity(for mission: Mission, kind: PayloadKind) -> PayloadRarity {
        switch mission.id {
        case "t-85e75668", "t-9f10c3d2": return .legendary
        case "t-b1930f5c": return .common
        default: break
        }
        if kind == .scrap { return .salvage }
        let threat = mission.threat ?? 1
        if threat >= 5 { return .legendary }
        if threat >= 4 { return .epic }
        if threat >= 3 { return .rare }
        if threat >= 2 { return .uncommon }
        return .common
    }

    private static func manifest(for mission: Mission) -> String {
        let tokens = FleetFormatters.tokens(mission.tokens)
        let cost = FleetFormatters.cost(mission.cost)
        switch mission.id {
        case "t-85e75668": return "9 venues · \(tokens) · \(cost)"
        case "t-9f10c3d2": return "3.4 MB bin · 32 commands"
        default:
            return "\(tokens) · \(cost)"
        }
    }
}
