// Client-side payload kind and priority derivation from mission facts.
// Named DESIGN.md §7 payloads win; generic heuristics cover the rest.
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
            if let named = namedPayload(for: mission, sectorTag: sectorTag) {
                return named
            }
            let kind = heuristicKind(for: mission)
            return Payload(
                id: mission.id,
                name: heuristicName(for: mission, kind: kind),
                kind: kind,
                rarity: heuristicRarity(for: mission, kind: kind),
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

    /// Spec-named payloads from DESIGN.md §7 (+ canvas call-outs for kind/rarity).
    private static func namedPayload(for mission: Mission, sectorTag: String) -> Payload? {
        switch mission.id {
        case "t-85e75668":
            return Payload(
                id: mission.id, name: "Fill-rate regression", kind: .report,
                rarity: .legendary, sectorTag: sectorTag,
                manifest: "9 venues · \(FleetFormatters.tokens(mission.tokens)) · \(FleetFormatters.cost(mission.cost))"
            )
        case "t-9f10c3d2":
            return Payload(
                id: mission.id, name: "aid v8.78.0", kind: .release,
                rarity: .legendary, sectorTag: sectorTag,
                manifest: "3.4 MB bin · 32 commands"
            )
        case "t-b1930f5c":
            return Payload(
                id: mission.id, name: "Store type fixes", kind: .fixture,
                rarity: .common, sectorTag: sectorTag, manifest: manifest(for: mission)
            )
        case "t-22c5346d":
            return Payload(
                id: mission.id, name: "Ladder-gas study", kind: .bench,
                rarity: .epic, sectorTag: sectorTag, manifest: manifest(for: mission)
            )
        case "t-d5a7f26e":
            return Payload(
                id: mission.id, name: "Call-site rescue", kind: .dataset,
                rarity: .rare, sectorTag: sectorTag, manifest: manifest(for: mission)
            )
        case "t-77b2e004":
            return Payload(
                id: mission.id, name: "Rename watch engine symbols", kind: .patch,
                rarity: .common, sectorTag: sectorTag, manifest: manifest(for: mission)
            )
        default:
            return nil
        }
    }

    private static func heuristicKind(for mission: Mission) -> PayloadKind {
        let title = mission.title.lowercased()
        if title.contains("release") || title.contains("cut release") { return .release }
        if title.contains("audit") { return .audit }
        if title.contains("review") || title.contains("regression") { return .report }
        if title.contains("measurement") || title.contains("bench") || title.contains("study") {
            return .bench
        }
        if title.contains("fix") || title.contains("type") { return .fixture }
        if title.contains("patch") || title.contains("rename") || title.contains("plumbing") {
            return .patch
        }
        if title.contains("dataset") || title.contains("discovery") || title.contains("call-site") {
            return .dataset
        }
        return .report
    }

    private static func heuristicName(for mission: Mission, kind: PayloadKind) -> String {
        if kind == .release, let title = mission.title.split(separator: " ").last {
            return String(title)
        }
        return mission.title
    }

    private static func heuristicRarity(for mission: Mission, kind: PayloadKind) -> PayloadRarity {
        if kind == .scrap { return .salvage }
        let threat = mission.threat ?? 1
        if threat >= 5 { return .legendary }
        if threat >= 4 { return .epic }
        if threat >= 3 { return .rare }
        if threat >= 2 { return .uncommon }
        return .common
    }

    private static func manifest(for mission: Mission) -> String {
        "\(FleetFormatters.tokens(mission.tokens)) · \(FleetFormatters.cost(mission.cost))"
    }
}
