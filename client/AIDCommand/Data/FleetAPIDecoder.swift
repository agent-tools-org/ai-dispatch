// Codable DTOs and mapping from GET /api/fleet into FleetSnapshot models.
// Exports: FleetAPIDecoder.

import Foundation

enum FleetAPIDecoder {
    static func decodeSnapshot(from data: Data, connection: ConnectionState) throws -> FleetSnapshot {
        let dto = try JSONDecoder().decode(FleetDTO.self, from: data)
        return map(dto, connection: connection)
    }

    static func decodeAction(from data: Data) throws -> MissionActionResult {
        let dto = try JSONDecoder().decode(ActionDTO.self, from: data)
        return MissionActionResult(ok: dto.ok, message: dto.error ?? (dto.ok ? "ok" : "failed"))
    }

    static func decodeDiff(from data: Data) throws -> String {
        try JSONDecoder().decode(DiffDTO.self, from: data).diff
    }

    static func decodeResult(from data: Data) throws -> String {
        try JSONDecoder().decode(ResultDTO.self, from: data).result
    }

    static func decodeTaskDetail(from data: Data) throws -> (mission: Mission, prompt: String, events: [MissionEvent]) {
        let dto = try JSONDecoder().decode(TaskDTO.self, from: data)
        let mission = mapTask(dto, medians: [:])
        let events = dto.latest_events?.map {
            MissionEvent(id: "\($0.timestamp)-\($0.event_kind)", message: $0.detail)
        } ?? []
        return (mission, dto.prompt ?? dto.prompt_excerpt ?? "—", events)
    }

    static func map(_ dto: FleetDTO, connection: ConnectionState) -> FleetSnapshot {
        var sectors: [Sector] = []
        for (index, sector) in dto.sectors.enumerated() {
            let tag = "SEC-\(String(format: "%02d", index + 1))"
            let missions = sector.tasks.map { mapTask($0, medians: [:]) }
            let wg = sector.workgroup_id.map(stripWG) ?? "——"
            sectors.append(Sector(
                id: sector.id,
                tag: tag,
                name: sector.name,
                workgroupID: wg,
                missions: missions
            ))
        }
        let medians = ProgressDeriver.medianDurations(from: sectors)
        for s in sectors.indices {
            sectors[s].missions = sectors[s].missions.map { remapped($0, medians: medians) }
        }
        let summary = FleetSummary(
            window: dto.summary.window,
            running: dto.summary.running,
            done: dto.summary.done,
            failed: dto.summary.failed,
            spendUSD: formatSpend(dto.summary.spend_usd),
            memoryMB: Int(dto.summary.memory_mb ?? 0),
            sectorCount: sectors.count
        )
        let agents = dto.agents.map {
            AgentInfo(
                id: $0.name,
                busy: $0.busy,
                quotaOK: ($0.quota?.state ?? "unknown") == "ok",
                taskCount: Int($0.task_count ?? 0)
            )
        }
        return FleetSnapshot(
            sectors: sectors,
            summary: summary,
            serverVersion: dto.server.version,
            connection: connection,
            agents: agents,
            tick: 0
        )
    }

    static func mapTask(_ dto: TaskDTO, medians: [String: [Int]]) -> Mission {
        let state = StatusMapper.displayState(from: dto.status)
        let elapsed = elapsedSeconds(dto)
        let progress: Double
        switch state {
        case .done: progress = 1
        case .run: progress = ProgressDeriver.progress(
            elapsedSeconds: elapsed, agent: dto.agent, completedDurations: medians
        )
        case .fail, .stop: progress = 0
        }
        return Mission(
            id: dto.id,
            title: title(for: dto),
            agent: dto.agent,
            model: dto.observed_model ?? dto.requested_model,
            state: state,
            threat: threat(from: dto.difficulty),
            progress: progress,
            elapsedSeconds: elapsed,
            tokens: formatTokens(dto.tokens),
            cost: formatCost(dto.cost_usd),
            memoryMB: dto.memory_mb.map { "\($0)M" },
            verifyTag: StatusMapper.verifyTag(from: dto.outcome),
            awaitingReason: dto.awaiting_reason
        )
    }

    private static func remapped(_ mission: Mission, medians: [String: [Int]]) -> Mission {
        guard mission.state == .run else { return mission }
        let progress = ProgressDeriver.progress(
            elapsedSeconds: mission.elapsedSeconds,
            agent: mission.agent,
            completedDurations: medians
        )
        return Mission(
            id: mission.id, title: mission.title, agent: mission.agent, model: mission.model,
            state: mission.state, threat: mission.threat, progress: progress,
            elapsedSeconds: mission.elapsedSeconds, tokens: mission.tokens, cost: mission.cost,
            memoryMB: mission.memoryMB, verifyTag: mission.verifyTag,
            awaitingReason: mission.awaitingReason
        )
    }

    private static func title(for dto: TaskDTO) -> String {
        if let excerpt = dto.prompt_excerpt, !excerpt.isEmpty { return excerpt }
        if let milestone = dto.latest_milestone, !milestone.isEmpty { return milestone }
        return dto.id
    }

    private static func elapsedSeconds(_ dto: TaskDTO) -> Int {
        if let ms = dto.duration_ms { return max(0, Int(ms / 1000)) }
        return 0
    }

    private static func threat(from difficulty: String?) -> Int? {
        guard let difficulty else { return nil }
        switch difficulty.lowercased() {
        case "trivial": return 1
        case "simple": return 2
        case "moderate": return 3
        case "complex": return 4
        default: return nil
        }
    }

    private static func formatSpend(_ value: Double?) -> String? {
        guard let value else { return nil }
        return String(format: "$%.2f", value)
    }

    private static func formatCost(_ value: Double?) -> String? {
        guard let value else { return nil }
        return String(format: "$%.2f", value)
    }

    private static func formatTokens(_ value: Int64?) -> String? {
        guard let value else { return nil }
        if value >= 1_000_000 {
            return String(format: "%.1fM", Double(value) / 1_000_000)
        }
        if value >= 1_000 {
            return String(format: "%.1fK", Double(value) / 1_000)
        }
        return "\(value)"
    }

    private static func stripWG(_ id: String) -> String {
        id.hasPrefix("wg-") ? String(id.dropFirst(3)) : id
    }
}

struct FleetDTO: Decodable {
    let server: ServerDTO
    let summary: SummaryDTO
    let sectors: [SectorDTO]
    let agents: [AgentDTO]
}

struct ServerDTO: Decodable {
    let version: String
    let host: String
    let port: Int
    let started_at: String
    let aid_home: String
}

struct SummaryDTO: Decodable {
    let running: Int
    let done: Int
    let failed: Int
    let stopped: Int?
    let spend_usd: Double?
    let tokens: Int64?
    let memory_mb: Int64?
    let window: String
}

struct SectorDTO: Decodable {
    let id: String
    let name: String
    let repo_path: String?
    let workgroup_id: String?
    let tasks: [TaskDTO]
}

struct TaskDTO: Decodable {
    let id: String
    let agent: String
    let status: String
    let outcome: String?
    let prompt: String?
    let prompt_excerpt: String?
    let requested_model: String?
    let observed_model: String?
    let tokens: Int64?
    let cost_usd: Double?
    let duration_ms: Int64?
    let difficulty: String?
    let memory_mb: Int64?
    let awaiting_reason: String?
    let latest_milestone: String?
    let latest_events: [EventDTO]?
}

struct EventDTO: Decodable {
    let timestamp: String
    let event_kind: String
    let detail: String
}

struct AgentDTO: Decodable {
    let name: String
    let busy: Bool
    let task_count: UInt64?
    let quota: QuotaDTO?
}

struct QuotaDTO: Decodable {
    let state: String?
}

struct ActionDTO: Decodable {
    let ok: Bool
    let error: String?
    let new_task_id: String?
}

struct DiffDTO: Decodable { let diff: String }
struct ResultDTO: Decodable { let result: String }
